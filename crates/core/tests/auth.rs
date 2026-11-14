//! Signing up, signing in and staying signed in, against a `wiremock` server standing in for
//! Supabase Auth.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use serde_json::json;
use streem_core::{BackendConfig, Command, Core, Session, State};
use wiremock::matchers::{body_partial_json, header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const KEY: &str = "test-public-key";
const PASSWORD: &str = "right password";

const MANIFEST: &str = r#"{"id": "org.example.one", "version": "1.0.0", "name": "Example One",
    "types": ["movie"], "resources": ["catalog"], "catalogs": []}"#;

fn backend(server: &MockServer) -> BackendConfig {
    BackendConfig {
        url: server.uri(),
        key: KEY.into(),
    }
}

/// Signed in with `backend`, and no default **addons**, so nothing reaches the internet.
fn start(dir: &Path, backend: BackendConfig) -> Core {
    Core::start_with_options(dir.to_path_buf(), Some(backend), || {}, Vec::new())
}

async fn wait_for(core: &Core, what: &str, done: impl Fn(&State) -> bool) -> Arc<State> {
    for _ in 0..250 {
        let state = core.state();
        if done(&state) {
            return state;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!(
        "timed out waiting for: {what}; state was {:?}",
        core.state()
    );
}

fn signed_in(state: &State) -> bool {
    matches!(state.session, Session::SignedIn { .. })
}

fn signed_out(state: &State) -> bool {
    matches!(state.session, Session::SignedOut { .. })
}

fn tokens(user_id: &str, email: &str, refresh_token: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "access_token": format!("access-{refresh_token}"), "token_type": "bearer",
        "expires_in": 3600, "expires_at": 4_000_000_000_u64, "refresh_token": refresh_token,
        "user": { "id": user_id, "aud": "authenticated", "email": email },
    }))
}

fn auth_mock(endpoint: &str) -> wiremock::MockBuilder {
    Mock::given(method("POST"))
        .and(path(endpoint))
        .and(header("apikey", KEY))
}

/// Supabase Auth with two **accounts**, `a@example.com` (`user-a`) and `b@example.com`
/// (`user-b`), both with [`PASSWORD`]. Any other password is refused. A refresh always works.
/// Also serves an **addon** at `/one/manifest.json` and `/two/manifest.json`.
async fn supabase() -> MockServer {
    let server = MockServer::start().await;
    auth_mock("/auth/v1/signup")
        .respond_with(tokens("user-a", "a@example.com", "refresh-a"))
        .mount(&server)
        .await;
    for (user_id, email) in [("user-a", "a@example.com"), ("user-b", "b@example.com")] {
        auth_mock("/auth/v1/token")
            .and(query_param("grant_type", "password"))
            .and(body_partial_json(
                json!({ "email": email, "password": PASSWORD }),
            ))
            .respond_with(tokens(user_id, email, &format!("refresh-{user_id}")))
            .with_priority(1)
            .mount(&server)
            .await;
    }
    auth_mock("/auth/v1/token")
        .and(query_param("grant_type", "password"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "code": 400, "error_code": "invalid_credentials", "msg": "Invalid login credentials",
        })))
        .with_priority(2)
        .mount(&server)
        .await;
    auth_mock("/auth/v1/token")
        .and(query_param("grant_type", "refresh_token"))
        .respond_with(tokens("user-a", "a@example.com", "refresh-next"))
        .mount(&server)
        .await;
    auth_mock("/auth/v1/logout")
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;
    for name in ["one", "two"] {
        Mock::given(method("GET"))
            .and(path(format!("/{name}/manifest.json")))
            .respond_with(ResponseTemplate::new(200).set_body_string(MANIFEST))
            .mount(&server)
            .await;
    }
    server
}

async fn requests_to(server: &MockServer, endpoint: &str) -> Vec<wiremock::Request> {
    let requests = server.received_requests().await.unwrap_or_default();
    requests
        .into_iter()
        .filter(|request| request.url.path() == endpoint)
        .collect()
}

async fn refresh_requests(server: &MockServer) -> usize {
    requests_to(server, "/auth/v1/token")
        .await
        .iter()
        .filter(|request| request.url.query() == Some("grant_type=refresh_token"))
        .count()
}

async fn sign_in(core: &Core, email: &str) -> Arc<State> {
    core.send(Command::SignIn {
        email: email.into(),
        password: PASSWORD.into(),
    });
    wait_for(core, "signed in", signed_in).await
}

/// A data directory where `a@example.com` signed in, then the app was closed.
async fn signed_in_before(server: &MockServer) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let core = start(dir.path(), backend(server));
    wait_for(&core, "signed out", signed_out).await;
    sign_in(&core, "a@example.com").await;
    dir
}

#[tokio::test]
async fn without_a_backend_the_session_is_not_configured() {
    let dir = tempfile::tempdir().unwrap();
    let core = Core::start_with_defaults(dir.path().to_path_buf(), || {}, Vec::new());
    assert_eq!(core.state().session, Session::NotConfigured);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(core.state().session, Session::NotConfigured);
}

#[tokio::test]
async fn with_no_session_file_the_app_starts_signed_out() {
    let server = supabase().await;
    let dir = tempfile::tempdir().unwrap();
    let core = start(dir.path(), backend(&server));

    let state = wait_for(&core, "signed out", signed_out).await;
    assert_eq!(state.session, Session::SignedOut { error: None });
}

#[tokio::test]
async fn sign_up_reaches_signed_in_and_writes_the_session_file() {
    let server = supabase().await;
    let dir = tempfile::tempdir().unwrap();
    let core = start(dir.path(), backend(&server));
    wait_for(&core, "signed out", signed_out).await;

    core.send(Command::SignUp {
        email: "a@example.com".into(),
        password: PASSWORD.into(),
    });
    let state = wait_for(&core, "signed in", signed_in).await;

    assert_eq!(
        state.session,
        Session::SignedIn {
            email: "a@example.com".into()
        }
    );
    let saved: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.path().join("session.json")).unwrap()).unwrap();
    assert_eq!(saved["refresh_token"], "refresh-a");
    assert_eq!(saved["user_id"], "user-a");
    assert_eq!(saved["email"], "a@example.com");
    let signups = requests_to(&server, "/auth/v1/signup").await;
    let body: serde_json::Value = serde_json::from_slice(&signups[0].body).unwrap();
    assert_eq!(
        body,
        json!({ "email": "a@example.com", "password": PASSWORD })
    );
}

#[tokio::test]
async fn a_rejected_password_gives_signed_out_with_the_servers_message() {
    let server = supabase().await;
    let dir = tempfile::tempdir().unwrap();
    let core = start(dir.path(), backend(&server));
    wait_for(&core, "signed out", signed_out).await;

    core.send(Command::SignIn {
        email: "a@example.com".into(),
        password: "wrong".into(),
    });
    let state = wait_for(&core, "an error", |s| {
        matches!(s.session, Session::SignedOut { error: Some(_) })
    })
    .await;

    assert_eq!(
        state.session,
        Session::SignedOut {
            error: Some("Invalid login credentials".into())
        }
    );
    assert!(!dir.path().join("session.json").exists());
}

#[tokio::test]
async fn a_server_that_cannot_be_reached_gives_the_connection_message() {
    let dir = tempfile::tempdir().unwrap();
    // Nothing listens on port 9.
    let nowhere = BackendConfig {
        url: "http://127.0.0.1:9".into(),
        key: KEY.into(),
    };
    let core = start(dir.path(), nowhere);
    wait_for(&core, "signed out", signed_out).await;

    core.send(Command::SignIn {
        email: "a@example.com".into(),
        password: PASSWORD.into(),
    });
    let state = wait_for(&core, "an error", |s| {
        matches!(s.session, Session::SignedOut { error: Some(_) })
    })
    .await;
    assert_eq!(
        state.session,
        Session::SignedOut {
            error: Some("Could not reach the server. Check your connection.".into())
        }
    );
}

#[tokio::test]
async fn a_new_core_on_the_same_data_directory_starts_signed_in_and_refreshes() {
    let server = supabase().await;
    let dir = signed_in_before(&server).await;
    assert_eq!(refresh_requests(&server).await, 0);

    let core = start(dir.path(), backend(&server));
    let state = wait_for(&core, "signed in", signed_in).await;
    assert_eq!(
        state.session,
        Session::SignedIn {
            email: "a@example.com".into()
        }
    );
    for _ in 0..250 {
        if refresh_requests(&server).await > 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let refreshes = requests_to(&server, "/auth/v1/token").await;
    let refresh = refreshes
        .iter()
        .find(|request| request.url.query() == Some("grant_type=refresh_token"))
        .expect("the refresh endpoint was called");
    let body: serde_json::Value = serde_json::from_slice(&refresh.body).unwrap();
    assert_eq!(body, json!({ "refresh_token": "refresh-user-a" }));
}

#[tokio::test]
async fn a_rejected_refresh_token_gives_signed_out() {
    let server = supabase().await;
    let dir = signed_in_before(&server).await;
    let rejecting = MockServer::start().await;
    auth_mock("/auth/v1/token")
        .and(query_param("grant_type", "refresh_token"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "code": 400, "error_code": "refresh_token_not_found",
            "msg": "Invalid Refresh Token: Refresh Token Not Found",
        })))
        .mount(&rejecting)
        .await;

    let core = start(dir.path(), backend(&rejecting));
    let state = wait_for(&core, "an error", |s| {
        matches!(s.session, Session::SignedOut { error: Some(_) })
    })
    .await;

    assert_eq!(
        state.session,
        Session::SignedOut {
            error: Some("Please sign in again.".into())
        }
    );
    assert!(!dir.path().join("session.json").exists());
}

#[tokio::test]
async fn a_refresh_answered_with_a_retry_later_status_stays_signed_in() {
    for status in [408, 425, 429, 404] {
        let server = supabase().await;
        let dir = signed_in_before(&server).await;
        let busy = MockServer::start().await;
        auth_mock("/auth/v1/token")
            .and(query_param("grant_type", "refresh_token"))
            .respond_with(ResponseTemplate::new(status))
            .mount(&busy)
            .await;

        let core = start(dir.path(), backend(&busy));
        wait_for(&core, "signed in", signed_in).await;
        for _ in 0..250 {
            if refresh_requests(&busy).await > 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(refresh_requests(&busy).await > 0, "{status}: never tried");
        // Time for the answer to reach `Core`.
        tokio::time::sleep(Duration::from_millis(300)).await;

        assert!(
            signed_in(&core.state()),
            "{status}: {:?}",
            core.state().session
        );
        assert!(dir.path().join("session.json").exists(), "{status}");
    }
}

#[tokio::test]
async fn a_refresh_that_cannot_connect_stays_signed_in() {
    let server = supabase().await;
    let dir = signed_in_before(&server).await;
    // Closes every connection as soon as it is made, so the refresh fails without an answer.
    // Counting the connections tells when it was tried; a refused port gives no such sign, and
    // on Windows takes seconds to fail.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let attempts = Arc::new(AtomicUsize::new(0));
    let counted = attempts.clone();
    std::thread::spawn(move || {
        for connection in listener.incoming() {
            counted.fetch_add(1, Ordering::SeqCst);
            drop(connection);
        }
    });
    let closing = BackendConfig {
        url: format!("http://{address}"),
        key: KEY.into(),
    };

    let core = start(dir.path(), closing);
    wait_for(&core, "signed in", signed_in).await;
    for _ in 0..250 {
        if attempts.load(Ordering::SeqCst) > 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(
        attempts.load(Ordering::SeqCst) > 0,
        "the refresh was never tried"
    );
    // Time for the failure to reach `Core`.
    tokio::time::sleep(Duration::from_millis(300)).await;

    assert!(signed_in(&core.state()), "{:?}", core.state().session);
    assert!(dir.path().join("session.json").exists());
}

#[tokio::test]
async fn an_account_that_cannot_be_opened_on_this_device_signs_out_with_the_reason() {
    let server = supabase().await;
    let dir = signed_in_before(&server).await;
    // Breaks the switch to the **account**: the table it records the **account** in is gone.
    rusqlite::Connection::open(dir.path().join("streem.db"))
        .unwrap()
        .execute("DROP TABLE local_state", [])
        .unwrap();

    let core = start(dir.path(), backend(&server));
    let state = wait_for(&core, "an error", |s| {
        matches!(s.session, Session::SignedOut { error: Some(_) })
    })
    .await;

    let Session::SignedOut { error: Some(error) } = &state.session else {
        unreachable!()
    };
    assert!(error.starts_with("Could not open this account"), "{error}");
    assert!(state.addons.is_empty());
    assert!(!dir.path().join("session.json").exists());
}

#[tokio::test]
async fn sign_out_calls_logout_deletes_the_session_file_and_goes_to_signed_out() {
    let server = supabase().await;
    let dir = tempfile::tempdir().unwrap();
    let core = start(dir.path(), backend(&server));
    wait_for(&core, "signed out", signed_out).await;
    sign_in(&core, "a@example.com").await;
    assert!(dir.path().join("session.json").exists());

    core.send(Command::SignOut);
    let state = wait_for(&core, "signed out", signed_out).await;

    assert_eq!(state.session, Session::SignedOut { error: None });
    assert!(!dir.path().join("session.json").exists());
    let logouts = requests_to(&server, "/auth/v1/logout").await;
    assert_eq!(logouts.len(), 1);
    assert_eq!(
        logouts[0].headers.get("authorization").unwrap(),
        "Bearer access-refresh-user-a"
    );
}

#[tokio::test]
async fn sign_out_still_signs_out_when_logout_fails() {
    let server = supabase().await;
    let dir = tempfile::tempdir().unwrap();
    let core = start(dir.path(), backend(&server));
    wait_for(&core, "signed out", signed_out).await;
    sign_in(&core, "a@example.com").await;
    server.reset().await;
    auth_mock("/auth/v1/logout")
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    core.send(Command::SignOut);
    let state = wait_for(&core, "signed out", signed_out).await;
    assert_eq!(state.session, Session::SignedOut { error: None });
    assert!(!dir.path().join("session.json").exists());
}

/// `(account_id, url)` of every row in the `addons` table.
fn addon_rows(dir: &Path) -> Vec<(String, String)> {
    let connection = rusqlite::Connection::open(dir.join("streem.db")).unwrap();
    let mut statement = connection
        .prepare("SELECT account_id, url FROM addons ORDER BY url")
        .unwrap();
    statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

#[tokio::test]
async fn signing_in_as_a_second_user_id_empties_the_addons_table() {
    let server = supabase().await;
    let dir = tempfile::tempdir().unwrap();
    let core = start(dir.path(), backend(&server));
    wait_for(&core, "signed out", signed_out).await;
    sign_in(&core, "a@example.com").await;
    let one = format!("{}/one/manifest.json", server.uri());
    core.send(Command::InstallAddon { url: one.clone() });
    wait_for(&core, "installed", |s| s.addons.len() == 1).await;
    assert_eq!(addon_rows(dir.path()), [("user-a".to_string(), one)]);

    core.send(Command::SignOut);
    wait_for(&core, "signed out", signed_out).await;
    let state = sign_in(&core, "b@example.com").await;
    assert_eq!(
        state.session,
        Session::SignedIn {
            email: "b@example.com".into()
        }
    );
    let two = format!("{}/two/manifest.json", server.uri());
    core.send(Command::InstallAddon { url: two.clone() });
    let state = wait_for(&core, "installed", |s| s.addons.len() == 1).await;

    assert_eq!(state.addons[0].url, two);
    assert_eq!(addon_rows(dir.path()), [("user-b".to_string(), two)]);
}

#[tokio::test]
async fn the_same_user_signing_in_again_keeps_their_addons() {
    let server = supabase().await;
    let dir = tempfile::tempdir().unwrap();
    let core = start(dir.path(), backend(&server));
    wait_for(&core, "signed out", signed_out).await;
    sign_in(&core, "a@example.com").await;
    let one = format!("{}/one/manifest.json", server.uri());
    core.send(Command::InstallAddon { url: one.clone() });
    wait_for(&core, "installed", |s| s.addons.len() == 1).await;

    core.send(Command::SignOut);
    let state = wait_for(&core, "signed out", signed_out).await;
    assert!(state.addons.is_empty(), "signing out hides the addons");
    sign_in(&core, "a@example.com").await;

    let state = wait_for(&core, "addons loaded", |s| s.addons.len() == 1).await;
    assert_eq!(state.addons[0].url, one);
}

#[tokio::test]
async fn the_default_addons_are_installed_after_sign_in_not_before() {
    let server = supabase().await;
    let one = format!("{}/one/manifest.json", server.uri());
    let dir = tempfile::tempdir().unwrap();
    let core = Core::start_with_options(
        dir.path().to_path_buf(),
        Some(backend(&server)),
        || {},
        vec![one.clone()],
    );
    wait_for(&core, "signed out", signed_out).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(requests_to(&server, "/one/manifest.json").await.is_empty());

    sign_in(&core, "a@example.com").await;
    let state = wait_for(&core, "default installed", |s| s.addons.len() == 1).await;
    assert_eq!(state.addons[0].url, one);
    assert_eq!(addon_rows(dir.path()), [("user-a".to_string(), one)]);
}

#[tokio::test]
async fn every_request_carries_the_apikey_header() {
    let server = supabase().await;
    let dir = tempfile::tempdir().unwrap();
    {
        let core = start(dir.path(), backend(&server));
        wait_for(&core, "signed out", signed_out).await;
        core.send(Command::SignUp {
            email: "a@example.com".into(),
            password: PASSWORD.into(),
        });
        wait_for(&core, "signed in", signed_in).await;
        core.send(Command::SignOut);
        wait_for(&core, "signed out", signed_out).await;
        core.send(Command::SignIn {
            email: "a@example.com".into(),
            password: "wrong".into(),
        });
        wait_for(&core, "an error", |s| {
            matches!(s.session, Session::SignedOut { error: Some(_) })
        })
        .await;
        sign_in(&core, "a@example.com").await;
    }
    let core = start(dir.path(), backend(&server));
    wait_for(&core, "signed in", signed_in).await;
    for _ in 0..250 {
        if refresh_requests(&server).await > 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    let requests: Vec<_> = server
        .received_requests()
        .await
        .unwrap()
        .into_iter()
        .filter(|request| request.url.path().starts_with("/auth/v1/"))
        .collect();
    let endpoints: std::collections::BTreeSet<_> =
        requests.iter().map(|request| request.url.path()).collect();
    assert_eq!(
        endpoints.into_iter().collect::<Vec<_>>(),
        ["/auth/v1/logout", "/auth/v1/signup", "/auth/v1/token"]
    );
    assert!(refresh_requests(&server).await > 0);
    for request in &requests {
        assert_eq!(
            request
                .headers
                .get("apikey")
                .map(|value| value.to_str().unwrap()),
            Some(KEY),
            "{} {}",
            request.method,
            request.url
        );
    }
}

#[tokio::test]
async fn the_password_never_shows_up_in_debug_output() {
    let command = Command::SignIn {
        email: "a@example.com".into(),
        password: PASSWORD.into(),
    };
    let printed = format!("{command:?}");
    assert!(!printed.contains(PASSWORD), "{printed}");
    assert!(printed.contains("a@example.com"));
    let command = Command::SignUp {
        email: "a@example.com".into(),
        password: PASSWORD.into(),
    };
    assert!(!format!("{command:?}").contains(PASSWORD));
}

#[cfg(unix)]
#[tokio::test]
async fn the_session_file_is_readable_by_its_owner_only() {
    use std::os::unix::fs::PermissionsExt;

    let server = supabase().await;
    let dir = signed_in_before(&server).await;
    let mode = std::fs::metadata(dir.path().join("session.json"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600, "{mode:o}");

    // A refresh rewrites the file, still only for its owner.
    let core = start(dir.path(), backend(&server));
    wait_for(&core, "signed in", signed_in).await;
    for _ in 0..250 {
        let saved = std::fs::read_to_string(dir.path().join("session.json")).unwrap_or_default();
        if saved.contains("refresh-next") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let mode = std::fs::metadata(dir.path().join("session.json"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600, "{mode:o}");
}
