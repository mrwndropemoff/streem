//! The Real-Debrid key never leaves `Core` in text: not in a log record, a notice, an error or
//! the `{:?}` of the state.
//!
//! A test binary of its own, because it installs a process-wide logger. `tracing` is built with
//! its `log` feature here, so the events of the HTTP stack reach that logger too.

use std::sync::Mutex;
use std::time::Duration;

use streem_core::{Command, Core, Load, State};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Contains `/` and a space, so it travels percent-encoded. Both spellings contain this.
const KEY: &str = "SECRETKEY/42 x";
const KEY_MARK: &str = "SECRETKEY";
const ENCODED_KEY: &str = "SECRETKEY%2F42%20x";

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

async fn wait_for(core: &Core, what: &str, done: impl Fn(&State) -> bool) -> std::sync::Arc<State> {
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

async fn serve(server: &MockServer, request_path: &str, status: u16, body: &str) {
    Mock::given(method("GET"))
        .and(path(request_path))
        .respond_with(ResponseTemplate::new(status).set_body_string(body))
        .mount(server)
        .await;
}

/// An **addon** under `/realdebrid={key}/` that offers every resource for `tt1`. Its stream
/// list fails with a 500, so an error from a templated request reaches the state too.
async fn serve_debrid_addon(server: &MockServer) {
    let base = format!("/realdebrid={ENCODED_KEY}");
    let manifest = r#"{"id": "org.example.debrid", "version": "1.0.0", "name": "Debrid",
        "types": ["movie"], "idPrefixes": ["tt"],
        "resources": ["catalog", "meta", "stream", "subtitles"],
        "catalogs": [{"type": "movie", "id": "top"}]}"#;
    serve(server, &format!("{base}/manifest.json"), 200, manifest).await;
    let catalog = r#"{"metas": [{"id": "tt1", "type": "movie", "name": "A Film"}]}"#;
    serve(
        server,
        &format!("{base}/catalog/movie/top.json"),
        200,
        catalog,
    )
    .await;
    let meta = r#"{"meta": {"id": "tt1", "type": "movie", "name": "A Film"}}"#;
    serve(server, &format!("{base}/meta/movie/tt1.json"), 200, meta).await;
    serve(server, &format!("{base}/stream/movie/tt1.json"), 500, "").await;
}

#[tokio::test]
async fn the_key_appears_in_no_log_record_notice_or_state() {
    log::set_logger(&Capture).unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    log::info!(target: "canary", "capture works");

    let server = MockServer::start().await;
    serve_debrid_addon(&server).await;
    let dir = tempfile::tempdir().unwrap();
    let core = Core::start_with_defaults(dir.path().to_path_buf(), || {}, Vec::new());
    let mut printed = Vec::new();

    core.send(Command::SetRealDebridKey(Some(KEY.into())));
    wait_for(&core, "key saved", |s| s.settings.key() == Some(KEY)).await;
    core.send(Command::InstallAddon {
        url: format!(
            "{}/realdebrid={{real_debrid_key}}/manifest.json",
            server.uri()
        ),
    });
    let state = wait_for(&core, "home row ready", |s| {
        matches!(s.home.first().map(|row| &row.items), Some(Load::Ready(_)))
    })
    .await;
    printed.push(format!("{state:?}"));

    core.send(Command::OpenTitle {
        meta_type: "movie".into(),
        meta_id: "tt1".into(),
    });
    core.send(Command::LoadStreams {
        video_id: "tt1".into(),
    });
    let state = wait_for(&core, "stream request failed", |s| {
        let groups = s.details.as_ref().and_then(|d| d.streams.as_ref());
        groups.is_some_and(|list| matches!(list.groups[0].streams, Load::Failed(_)))
    })
    .await;
    printed.push(format!("{state:?}"));

    // Nothing listens on port 9, so the request fails before any answer.
    core.send(Command::InstallAddon {
        url: "http://127.0.0.1:9/realdebrid={real_debrid_key}/manifest.json".into(),
    });
    let state = wait_for(&core, "a notice", |s| s.notice.is_some()).await;
    let notice = state.notice.as_ref().unwrap().text.clone();
    assert!(notice.contains("could not reach the addon"), "{notice}");
    printed.push(notice);
    printed.push(format!("{state:?}"));

    drop(core);
    let records = RECORDS.lock().unwrap().clone();
    let requested = server.received_requests().await.unwrap_or_default();
    assert!(
        requested
            .iter()
            .any(|request| request.url.path().contains(ENCODED_KEY)),
        "the addon was asked with the key"
    );
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
        assert!(!text.contains(KEY_MARK), "the key leaked: {text}");
    }
}
