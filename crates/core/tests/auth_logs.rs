//! Signing in, refreshing and signing out log neither the password nor a token.
//!
//! A test binary of its own, because it installs a process-wide logger. `tracing` is built with
//! its `log` feature here, so the events of the HTTP stack reach that logger too.

use std::sync::Mutex;
use std::time::Duration;

use serde_json::json;
use streem_core::{BackendConfig, Command, Core, Session, State};
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const PASSWORD: &str = "PASSWORDMARK-7";
const ACCESS_TOKEN: &str = "ACCESSMARK-1";
const REFRESH_TOKEN: &str = "REFRESHMARK-1";
const NEXT_ACCESS_TOKEN: &str = "ACCESSMARK-2";
const NEXT_REFRESH_TOKEN: &str = "REFRESHMARK-2";
const SECRETS: &[&str] = &["PASSWORDMARK", "ACCESSMARK", "REFRESHMARK"];

/// Every log record, as "level target: message".
static RECORDS: Mutex<Vec<String>> = Mutex::new(Vec::new());

struct Capture;

impl log::Log for Capture {
    fn enabled(&self, _: &log::Metadata<'_>) -> bool {
        true
    }

    fn log(&self, record: &log::Record<'_>) {
        let line = format!("{} {}: {}", record.level(), record.target(), record.args());
        RECORDS.lock().unwrap().push(line);
    }

    fn flush(&self) {}
}

async fn wait_for(core: &Core, what: &str, done: impl Fn(&State) -> bool) {
    for _ in 0..250 {
        if done(&core.state()) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!(
        "timed out waiting for: {what}; state was {:?}",
        core.state()
    );
}

fn tokens(access: &str, refresh: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "access_token": access, "token_type": "bearer", "expires_in": 3600,
        "refresh_token": refresh, "user": { "id": "user-a", "email": "a@example.com" },
    }))
}

#[tokio::test]
async fn no_log_record_holds_the_password_or_a_token() {
    log::set_logger(&Capture).unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    log::info!(target: "canary", "capture works");

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/auth/v1/token"))
        .and(query_param("grant_type", "password"))
        .respond_with(tokens(ACCESS_TOKEN, REFRESH_TOKEN))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/auth/v1/token"))
        .and(query_param("grant_type", "refresh_token"))
        .respond_with(tokens(NEXT_ACCESS_TOKEN, NEXT_REFRESH_TOKEN))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/auth/v1/logout"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;
    let backend = BackendConfig {
        url: server.uri(),
        key: "test-public-key".into(),
    };
    let dir = tempfile::tempdir().unwrap();
    let start = || {
        Core::start_with_options(
            dir.path().to_path_buf(),
            Some(backend.clone()),
            || {},
            Vec::new(),
        )
    };
    let signed_in = |s: &State| matches!(s.session, Session::SignedIn { .. });
    let signed_out = |s: &State| matches!(s.session, Session::SignedOut { .. });
    let mut printed = Vec::new();

    // Sign in, then start again, which refreshes, then sign out.
    {
        let core = start();
        wait_for(&core, "signed out", signed_out).await;
        let command = Command::SignIn {
            email: "a@example.com".into(),
            password: PASSWORD.into(),
        };
        printed.push(format!("{command:?}"));
        core.send(command);
        wait_for(&core, "signed in", signed_in).await;
        printed.push(format!("{:?}", core.state()));
    }
    let core = start();
    wait_for(&core, "signed in", signed_in).await;
    for _ in 0..250 {
        let saved = std::fs::read_to_string(dir.path().join("session.json")).unwrap_or_default();
        if saved.contains(NEXT_REFRESH_TOKEN) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    core.send(Command::SignOut);
    wait_for(&core, "signed out", signed_out).await;
    printed.push(format!("{:?}", core.state()));
    drop(core);

    // Each secret really travelled, so the log had the chance to show it.
    let requests = server.received_requests().await.unwrap();
    let sent: Vec<String> = requests
        .iter()
        .map(|request| {
            let headers = format!("{:?}", request.headers);
            format!("{headers} {}", String::from_utf8_lossy(&request.body))
        })
        .collect();
    for secret in [PASSWORD, REFRESH_TOKEN, NEXT_ACCESS_TOKEN] {
        assert!(
            sent.iter().any(|text| text.contains(secret)),
            "{secret} was never sent"
        );
    }

    let records = RECORDS.lock().unwrap().clone();
    assert!(records.iter().any(|line| line.contains(" canary: ")));
    // The HTTP client and its connection pool logged, so their records were captured, not
    // missed.
    for target in [" reqwest", " hyper_util"] {
        assert!(
            records.iter().any(|line| line.contains(target)),
            "nothing from{target}: {records:#?}"
        );
    }
    for text in records.iter().chain(&printed) {
        for secret in SECRETS {
            assert!(!text.contains(secret), "{secret} leaked: {text}");
        }
    }
}
