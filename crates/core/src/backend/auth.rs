//! Signing in to the sync backend's Auth API over plain HTTPS, and staying signed in.
//!
//! The access token is kept in memory only. The refresh token, user id and email are saved to
//! `session.json` in the data directory. Nothing here logs, and nothing that holds a password
//! or a token has a `Debug`.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::Mutex;

use super::BackendConfig;

/// Shown when the server could not be asked at all.
pub(crate) const UNREACHABLE: &str = "Could not reach the server. Check your connection.";
/// Shown when the server no longer accepts the saved session.
pub(crate) const SIGN_IN_AGAIN: &str = "Please sign in again.";

const SESSION_FILE: &str = "session.json";
/// An access token with less time left than this is refreshed before it is handed out.
const REFRESH_MARGIN: Duration = Duration::from_secs(60);

/// Who is signed in to the **account**.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Account {
    pub(crate) user_id: String,
    pub(crate) email: String,
}

/// Why [`Auth::access_token`] gave no token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TokenError {
    /// No one is signed in.
    SignedOut,
    /// The server refused the refresh token. The session is gone, `session.json` included.
    Rejected,
    /// The server could not be reached, or failed. The session is kept; try again later.
    Unavailable,
}

/// What `session.json` holds.
// shortcut: refresh token in a plain file, move it to the OS keyring before the app is shared beyond trusted people
#[derive(Serialize, Deserialize)]
struct SavedSession {
    refresh_token: String,
    user_id: String,
    email: String,
}

/// The tokens of the person signed in.
struct Tokens {
    saved: SavedSession,
    /// With the moment it expires. `None` after a restart, until the first refresh.
    access: Option<(String, Instant)>,
}

/// A successful answer of the sign-up and token endpoints.
#[derive(Deserialize)]
struct TokenAnswer {
    access_token: String,
    refresh_token: String,
    expires_in: u64,
    user: UserAnswer,
}

#[derive(Deserialize)]
struct UserAnswer {
    id: String,
    email: Option<String>,
}

pub(crate) struct Auth {
    client: reqwest::Client,
    config: BackendConfig,
    session_path: PathBuf,
    /// Every call that reads or changes the tokens holds this for its whole length, requests
    /// included, so two refreshes never race: each refresh token can be used only once.
    tokens: Mutex<Option<Tokens>>,
}

impl Auth {
    pub(crate) fn new(client: reqwest::Client, config: BackendConfig, data_dir: &Path) -> Auth {
        Auth {
            client,
            config,
            session_path: data_dir.join(SESSION_FILE),
            tokens: Mutex::new(None),
        }
    }

    /// The person saved in `session.json`, if any. They count as signed in from now on; their
    /// access token is fetched by the first [`Auth::access_token`].
    pub(crate) async fn restore(&self) -> Option<Account> {
        let mut tokens = self.tokens.lock().await;
        let path = self.session_path.clone();
        let saved = tokio::task::spawn_blocking(move || read_session(&path))
            .await
            .ok()
            .flatten()?;
        let signed_in = Account {
            user_id: saved.user_id.clone(),
            email: saved.email.clone(),
        };
        *tokens = Some(Tokens {
            saved,
            access: None,
        });
        Some(signed_in)
    }

    /// Creates an **account** and signs in to it. The error is text for the person.
    pub(crate) async fn sign_up(&self, email: &str, password: &str) -> Result<Account, String> {
        self.start_session("/auth/v1/signup", email, password).await
    }

    /// Signs in to an existing **account**. The error is text for the person.
    pub(crate) async fn sign_in(&self, email: &str, password: &str) -> Result<Account, String> {
        self.start_session("/auth/v1/token?grant_type=password", email, password)
            .await
    }

    async fn start_session(
        &self,
        path: &str,
        email: &str,
        password: &str,
    ) -> Result<Account, String> {
        let mut tokens = self.tokens.lock().await;
        let body = json!({ "email": email, "password": password });
        let response = self
            .post(path, Some(body), None)
            .await
            .map_err(|_| UNREACHABLE.to_string())?;
        if !response.status().is_success() {
            return Err(server_message(response).await);
        }
        let text = response.text().await.map_err(|_| UNREACHABLE.to_string())?;
        let Ok(answer) = serde_json::from_str::<TokenAnswer>(&text) else {
            // With "Confirm email" on, sign-up answers with the new user and no tokens.
            return Err(if path.ends_with("/signup") {
                "The account was created. Confirm it from the email the server sent, \
                 then sign in."
                    .to_string()
            } else {
                "The server's answer could not be read.".to_string()
            });
        };

        let access = (answer.access_token, expires_at(answer.expires_in));
        let saved = SavedSession {
            refresh_token: answer.refresh_token,
            user_id: answer.user.id,
            email: answer
                .user
                .email
                .unwrap_or_else(|| email.trim().to_string()),
        };
        self.write(&saved)
            .await
            .map_err(|error| format!("Could not save the session: {error}"))?;
        let signed_in = Account {
            user_id: saved.user_id.clone(),
            email: saved.email.clone(),
        };
        *tokens = Some(Tokens {
            saved,
            access: Some(access),
        });
        Ok(signed_in)
    }

    /// A valid access token for requests to the sync backend. It is refreshed first when it
    /// has fewer than 60 seconds left, or was never fetched since a restart. The rest of
    /// `streem-core` gets its token only from here.
    pub(crate) async fn access_token(&self) -> Result<String, TokenError> {
        let mut guard = self.tokens.lock().await;
        let tokens = guard.as_mut().ok_or(TokenError::SignedOut)?;
        if let Some((token, expires_at)) = &tokens.access
            && expires_at.saturating_duration_since(Instant::now()) >= REFRESH_MARGIN
        {
            return Ok(token.clone());
        }

        let body = json!({ "refresh_token": tokens.saved.refresh_token });
        let response = self
            .post("/auth/v1/token?grant_type=refresh_token", Some(body), None)
            .await
            .map_err(|_| TokenError::Unavailable)?;
        let status = response.status();
        // A 4xx other than "too many requests" is the server refusing this refresh token.
        // A paused project or a server error must not sign anyone out.
        if status.is_client_error() && status != reqwest::StatusCode::TOO_MANY_REQUESTS {
            *guard = None;
            self.delete().await;
            return Err(TokenError::Rejected);
        }
        if !status.is_success() {
            return Err(TokenError::Unavailable);
        }
        let answer: TokenAnswer = response.json().await.map_err(|_| TokenError::Unavailable)?;

        tokens.saved.refresh_token = answer.refresh_token;
        if let Some(email) = answer.user.email {
            tokens.saved.email = email;
        }
        tokens.access = Some((answer.access_token.clone(), expires_at(answer.expires_in)));
        // The old refresh token no longer works, so the new one must be saved. If that fails
        // the person stays signed in until the app is closed.
        let _ = self.write(&tokens.saved).await;
        Ok(answer.access_token)
    }

    /// Tells the server this device signs out, ignoring any failure, then forgets the session
    /// and deletes `session.json`. Only this device's session ends; others stay signed in.
    pub(crate) async fn sign_out(&self) {
        let mut tokens = self.tokens.lock().await;
        if let Some(Tokens {
            access: Some((access, _)),
            ..
        }) = tokens.take()
        {
            let _ = self
                .post("/auth/v1/logout?scope=local", None, Some(&access))
                .await;
        }
        self.delete().await;
    }

    async fn post(
        &self,
        path: &str,
        body: Option<serde_json::Value>,
        bearer: Option<&str>,
    ) -> Result<reqwest::Response, reqwest::Error> {
        let mut request = self
            .client
            .post(format!("{}{path}", self.config.url))
            .header("apikey", &self.config.key);
        if let Some(body) = body {
            request = request.json(&body);
        }
        if let Some(token) = bearer {
            request = request.bearer_auth(token);
        }
        request.send().await
    }

    async fn write(&self, saved: &SavedSession) -> std::io::Result<()> {
        let text = serde_json::to_vec(saved)?;
        let path = self.session_path.clone();
        tokio::task::spawn_blocking(move || write_session(&path, &text))
            .await
            .map_err(std::io::Error::other)?
    }

    async fn delete(&self) {
        let path = self.session_path.clone();
        let _ = tokio::task::spawn_blocking(move || std::fs::remove_file(path)).await;
    }
}

fn expires_at(expires_in: u64) -> Instant {
    Instant::now() + Duration::from_secs(expires_in)
}

/// The message in the server's JSON error body, or the status when there is none.
async fn server_message(response: reqwest::Response) -> String {
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    let body: Option<serde_json::Value> = serde_json::from_str(&text).ok();
    // Newer Supabase Auth errors have `msg`; older ones `error_description`.
    ["msg", "error_description", "message", "error"]
        .iter()
        .find_map(|field| {
            let text = body.as_ref()?.get(field)?.as_str()?.trim();
            (!text.is_empty()).then(|| text.to_string())
        })
        .unwrap_or_else(|| format!("The server answered {status}."))
}

/// `None` when there is no file, or it cannot be read.
fn read_session(path: &Path) -> Option<SavedSession> {
    let text = std::fs::read(path).ok()?;
    serde_json::from_slice(&text).ok()
}

/// Writes `session.json` through a new file, created readable by its owner only on Unix, that
/// then replaces the old one.
fn write_session(path: &Path, text: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let temp = path.with_extension("json.tmp");
    // A leftover file would keep its own permissions.
    let _ = std::fs::remove_file(&temp);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temp)?;
    file.write_all(text)?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(&temp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_partial_json, header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const KEY: &str = "public-key";

    fn answer(access: &str, refresh: &str, expires_in: u64) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({
            "access_token": access, "token_type": "bearer", "expires_in": expires_in,
            "refresh_token": refresh,
            "user": { "id": "user-1", "email": "a@example.com" },
        }))
    }

    async fn serve_sign_in(server: &MockServer, expires_in: u64) {
        Mock::given(method("POST"))
            .and(path("/auth/v1/token"))
            .and(query_param("grant_type", "password"))
            .and(header("apikey", KEY))
            .respond_with(answer("access-1", "refresh-1", expires_in))
            .mount(server)
            .await;
    }

    async fn serve_refresh(server: &MockServer, from: &str, to: &str) {
        Mock::given(method("POST"))
            .and(path("/auth/v1/token"))
            .and(query_param("grant_type", "refresh_token"))
            .and(header("apikey", KEY))
            .and(body_partial_json(json!({ "refresh_token": from })))
            .respond_with(answer(&format!("access-{to}"), to, 3600))
            .mount(server)
            .await;
    }

    fn auth(server: &MockServer, dir: &Path) -> Auth {
        let config = BackendConfig {
            url: server.uri(),
            key: KEY.into(),
        };
        Auth::new(reqwest::Client::new(), config, dir)
    }

    async fn refreshes(server: &MockServer) -> usize {
        let requests = server.received_requests().await.unwrap_or_default();
        requests
            .iter()
            .filter(|request| request.url.query() == Some("grant_type=refresh_token"))
            .count()
    }

    #[tokio::test]
    async fn a_token_with_30_seconds_left_is_refreshed_before_use() {
        let server = MockServer::start().await;
        serve_sign_in(&server, 30).await;
        serve_refresh(&server, "refresh-1", "refresh-2").await;
        let dir = tempfile::tempdir().unwrap();
        let auth = auth(&server, dir.path());
        auth.sign_in("a@example.com", "pw").await.unwrap();

        assert_eq!(auth.access_token().await.unwrap(), "access-refresh-2");
        assert_eq!(refreshes(&server).await, 1);
    }

    #[tokio::test]
    async fn a_token_with_time_left_is_used_as_it_is() {
        let server = MockServer::start().await;
        serve_sign_in(&server, 3600).await;
        let dir = tempfile::tempdir().unwrap();
        let auth = auth(&server, dir.path());
        auth.sign_in("a@example.com", "pw").await.unwrap();

        assert_eq!(auth.access_token().await.unwrap(), "access-1");
        assert_eq!(refreshes(&server).await, 0);
    }

    #[tokio::test]
    async fn each_restart_refreshes_with_the_token_saved_by_the_last_refresh() {
        let server = MockServer::start().await;
        serve_sign_in(&server, 3600).await;
        serve_refresh(&server, "refresh-1", "refresh-2").await;
        serve_refresh(&server, "refresh-2", "refresh-3").await;
        let dir = tempfile::tempdir().unwrap();
        auth(&server, dir.path())
            .sign_in("a@example.com", "pw")
            .await
            .unwrap();

        for expected in ["access-refresh-2", "access-refresh-3"] {
            let restarted = auth(&server, dir.path());
            let signed_in = restarted.restore().await.unwrap();
            assert_eq!(signed_in.user_id, "user-1");
            assert_eq!(restarted.access_token().await.unwrap(), expected);
        }
    }

    #[tokio::test]
    async fn a_server_error_on_refresh_keeps_the_session() {
        let server = MockServer::start().await;
        serve_sign_in(&server, 0).await;
        Mock::given(method("POST"))
            .and(path("/auth/v1/token"))
            .and(query_param("grant_type", "refresh_token"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let auth = auth(&server, dir.path());
        auth.sign_in("a@example.com", "pw").await.unwrap();

        assert_eq!(auth.access_token().await, Err(TokenError::Unavailable));
        assert!(dir.path().join(SESSION_FILE).exists());
    }

    #[tokio::test]
    async fn an_error_body_gives_the_servers_message_or_the_status() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/auth/v1/signup"))
            .respond_with(ResponseTemplate::new(422).set_body_json(
                json!({ "code": 422, "error_code": "weak_password", "msg": "Password is too short" }),
            ))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/auth/v1/token"))
            .respond_with(ResponseTemplate::new(502).set_body_string("<html>bad gateway"))
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let auth = auth(&server, dir.path());

        let error = auth.sign_up("a@example.com", "pw").await.unwrap_err();
        assert_eq!(error, "Password is too short");
        let error = auth.sign_in("a@example.com", "pw").await.unwrap_err();
        assert!(error.contains("502"), "{error}");
    }

    #[tokio::test]
    async fn a_sign_up_answered_without_tokens_asks_for_the_confirmation_email() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/auth/v1/signup"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({ "id": "user-1", "email": "a@example.com" })),
            )
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();

        let error = auth(&server, dir.path())
            .sign_up("a@example.com", "pw")
            .await
            .unwrap_err();
        assert!(error.contains("Confirm"), "{error}");
        assert!(!dir.path().join(SESSION_FILE).exists());
    }
}
