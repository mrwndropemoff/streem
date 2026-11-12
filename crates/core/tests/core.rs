//! Black-box tests of `Core`: commands in, `State` out.

use std::path::Path;
use std::time::Duration;

use streem_core::{Command, Core, State};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const MANIFEST: &str = r#"{
    "id": "org.example.one", "version": "1.0.0", "name": "Example One",
    "description": "First test addon",
    "types": ["movie"], "resources": ["catalog", {"name": "meta", "types": ["movie"]}],
    "catalogs": [{"type": "movie", "id": "top"}]
}"#;

fn start(dir: &Path) -> Core {
    // No default addons: the tests must not touch the internet.
    Core::start_with_defaults(dir.to_path_buf(), || {}, Vec::new())
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

async fn serve(body: &str, manifest_path: &str) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(manifest_path))
        .respond_with(ResponseTemplate::new(200).set_body_string(body))
        .mount(&server)
        .await;
    server
}

#[tokio::test]
async fn installs_an_addon_from_a_url() {
    let server = serve(MANIFEST, "/one/manifest.json").await;
    let url = format!("{}/one/manifest.json", server.uri());
    let dir = tempfile::tempdir().unwrap();
    let core = start(dir.path());

    core.send(Command::InstallAddon { url: url.clone() });
    let state = wait_for(&core, "addon installed", |s| !s.addons.is_empty()).await;

    assert_eq!(state.addons.len(), 1);
    let addon = &state.addons[0];
    assert_eq!(addon.url, url);
    assert_eq!(addon.id, "org.example.one");
    assert_eq!(addon.name, "Example One");
    assert_eq!(addon.version, "1.0.0");
    assert_eq!(addon.description, "First test addon");
    assert!(state.notice.is_none());
}

#[tokio::test]
async fn invalid_json_gives_a_notice_and_no_entry() {
    let server = serve("this is not json", "/bad/manifest.json").await;
    let dir = tempfile::tempdir().unwrap();
    let core = start(dir.path());

    core.send(Command::InstallAddon {
        url: format!("{}/bad/manifest.json", server.uri()),
    });
    let state = wait_for(&core, "a notice", |s| s.notice.is_some()).await;

    assert!(state.addons.is_empty());
    assert!(state.notice.as_ref().unwrap().text.contains("not valid"));
}

#[tokio::test]
async fn a_url_that_is_not_a_manifest_url_gives_a_notice() {
    let dir = tempfile::tempdir().unwrap();
    let core = start(dir.path());

    core.send(Command::InstallAddon {
        url: "hello".into(),
    });
    let state = wait_for(&core, "a notice", |s| s.notice.is_some()).await;
    assert!(state.addons.is_empty());
}

#[tokio::test]
async fn a_missing_manifest_gives_a_notice() {
    let server = MockServer::start().await; // answers 404 to everything
    let dir = tempfile::tempdir().unwrap();
    let core = start(dir.path());

    core.send(Command::InstallAddon {
        url: format!("{}/manifest.json", server.uri()),
    });
    let state = wait_for(&core, "a notice", |s| s.notice.is_some()).await;
    assert!(state.addons.is_empty());
    assert!(state.notice.as_ref().unwrap().text.contains("404"));
}

#[tokio::test]
async fn dismissing_clears_the_notice() {
    let dir = tempfile::tempdir().unwrap();
    let core = start(dir.path());
    core.send(Command::InstallAddon {
        url: "hello".into(),
    });
    wait_for(&core, "a notice", |s| s.notice.is_some()).await;

    core.send(Command::DismissNotice);
    wait_for(&core, "notice gone", |s| s.notice.is_none()).await;
}

#[tokio::test]
async fn removes_an_addon() {
    let server = serve(MANIFEST, "/one/manifest.json").await;
    let url = format!("{}/one/manifest.json", server.uri());
    let dir = tempfile::tempdir().unwrap();
    let core = start(dir.path());
    core.send(Command::InstallAddon { url: url.clone() });
    wait_for(&core, "addon installed", |s| !s.addons.is_empty()).await;

    core.send(Command::RemoveAddon { url });
    wait_for(&core, "addon removed", |s| s.addons.is_empty()).await;
}

#[tokio::test]
async fn removing_while_the_install_is_still_downloading_cancels_it() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/slow/manifest.json"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(MANIFEST)
                .set_delay(Duration::from_millis(400)),
        )
        .mount(&server)
        .await;
    let url = format!("{}/slow/manifest.json", server.uri());
    let dir = tempfile::tempdir().unwrap();
    let core = start(dir.path());

    core.send(Command::InstallAddon { url: url.clone() });
    core.send(Command::RemoveAddon { url });
    tokio::time::sleep(Duration::from_millis(900)).await;

    assert!(core.state().addons.is_empty());
}

#[tokio::test]
async fn a_removed_addon_can_be_installed_again() {
    let server = serve(MANIFEST, "/one/manifest.json").await;
    let url = format!("{}/one/manifest.json", server.uri());
    let dir = tempfile::tempdir().unwrap();
    let core = start(dir.path());
    core.send(Command::InstallAddon { url: url.clone() });
    wait_for(&core, "installed", |s| s.addons.len() == 1).await;
    core.send(Command::RemoveAddon { url: url.clone() });
    wait_for(&core, "removed", |s| s.addons.is_empty()).await;

    core.send(Command::InstallAddon { url });
    wait_for(&core, "installed again", |s| s.addons.len() == 1).await;
}

#[tokio::test]
async fn installed_addons_survive_a_restart() {
    let server = serve(MANIFEST, "/one/manifest.json").await;
    let url = format!("{}/one/manifest.json", server.uri());
    let dir = tempfile::tempdir().unwrap();
    {
        let core = start(dir.path());
        core.send(Command::InstallAddon { url: url.clone() });
        wait_for(&core, "installed", |s| s.addons.len() == 1).await;
    } // dropping `Core` stops it

    let core = start(dir.path());
    let state = wait_for(&core, "addon loaded", |s| !s.addons.is_empty()).await;
    assert_eq!(state.addons[0].url, url);
    assert_eq!(state.addons[0].name, "Example One");
}

#[tokio::test]
async fn a_removal_survives_a_restart() {
    let server = serve(MANIFEST, "/one/manifest.json").await;
    let url = format!("{}/one/manifest.json", server.uri());
    let dir = tempfile::tempdir().unwrap();
    {
        let core = start(dir.path());
        core.send(Command::InstallAddon { url: url.clone() });
        wait_for(&core, "installed", |s| s.addons.len() == 1).await;
        core.send(Command::RemoveAddon { url });
        wait_for(&core, "removed", |s| s.addons.is_empty()).await;
    }

    let core = start(dir.path());
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(core.state().addons.is_empty());
}

#[tokio::test]
async fn a_command_sent_just_before_dropping_core_is_still_saved() {
    let server = MockServer::start().await;
    for name in ["one", "two"] {
        Mock::given(method("GET"))
            .and(path(format!("/{name}/manifest.json")))
            .respond_with(ResponseTemplate::new(200).set_body_string(MANIFEST))
            .mount(&server)
            .await;
    }
    let one = format!("{}/one/manifest.json", server.uri());
    let two = format!("{}/two/manifest.json", server.uri());

    // Timing decides whether a lost write shows up, so try many times.
    for round in 0..25 {
        let dir = tempfile::tempdir().unwrap();
        {
            let core = start(dir.path());
            core.send(Command::InstallAddon { url: one.clone() });
            wait_for(&core, "installed", |s| s.addons.len() == 1).await;
            core.send(Command::RemoveAddon { url: one.clone() });
        } // dropped straight after sending

        // Installing a second addon proves the saved list was loaded first (the disk worker
        // runs jobs in order), so the first addon being gone means its removal was saved.
        let core = start(dir.path());
        core.send(Command::InstallAddon { url: two.clone() });
        let state = wait_for(&core, "second addon", |s| !s.addons.is_empty()).await;
        let urls: Vec<_> = state.addons.iter().map(|a| a.url.as_str()).collect();
        assert_eq!(urls, [two.as_str()], "round {round}");
    }
}

#[tokio::test]
async fn first_launch_installs_the_default_addons_once() {
    let server = serve(MANIFEST, "/one/manifest.json").await;
    let url = format!("{}/one/manifest.json", server.uri());
    let dir = tempfile::tempdir().unwrap();
    {
        let core = Core::start_with_defaults(dir.path().to_path_buf(), || {}, vec![url.clone()]);
        wait_for(&core, "default installed", |s| s.addons.len() == 1).await;
        core.send(Command::RemoveAddon { url: url.clone() });
        wait_for(&core, "removed", |s| s.addons.is_empty()).await;
    }

    // The data directory already has rows (one removed), so this is not a first launch.
    let core = Core::start_with_defaults(dir.path().to_path_buf(), || {}, vec![url]);
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(core.state().addons.is_empty());
}

#[tokio::test]
async fn on_change_is_called_after_state_changes() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let dir = tempfile::tempdir().unwrap();
    let core = Core::start_with_defaults(
        dir.path().to_path_buf(),
        move || {
            counter.fetch_add(1, Ordering::SeqCst);
        },
        Vec::new(),
    );
    core.send(Command::InstallAddon {
        url: "hello".into(),
    });
    wait_for(&core, "a notice", |s| s.notice.is_some()).await;
    assert!(calls.load(Ordering::SeqCst) >= 1);
}

/// Needs internet access: `cargo test -p streem-core -- --ignored`.
#[tokio::test]
#[ignore]
async fn installs_the_real_cinemeta_manifest() {
    let dir = tempfile::tempdir().unwrap();
    let core = start(dir.path());
    core.send(Command::InstallAddon {
        url: "stremio://v3-cinemeta.strem.io/manifest.json".into(),
    });
    let state = wait_for(&core, "Cinemeta installed", |s| {
        !s.addons.is_empty() || s.notice.is_some()
    })
    .await;
    assert!(state.notice.is_none(), "{:?}", state.notice);
    assert_eq!(state.addons[0].id, "com.linvo.cinemeta");
    assert_eq!(
        state.addons[0].url,
        "https://v3-cinemeta.strem.io/manifest.json"
    );
}

// ---- Home screen: catalog rows -------------------------------------------------------------

use streem_core::Load;

fn catalog_manifest(name: &str, catalogs: &str) -> String {
    format!(
        r#"{{"id": "org.example.{name}", "version": "1.0.0", "name": "{name}",
            "types": ["movie"], "resources": ["catalog"], "catalogs": {catalogs}}}"#
    )
}

async fn serve_addon(server: &MockServer, name: &str, catalogs: &str) -> String {
    Mock::given(method("GET"))
        .and(path(format!("/{name}/manifest.json")))
        .respond_with(ResponseTemplate::new(200).set_body_string(catalog_manifest(name, catalogs)))
        .mount(server)
        .await;
    format!("{}/{name}/manifest.json", server.uri())
}

async fn serve_catalog(server: &MockServer, request_path: &str, status: u16, body: &str) {
    Mock::given(method("GET"))
        .and(path(request_path))
        .respond_with(ResponseTemplate::new(status).set_body_string(body))
        .mount(server)
        .await;
}

fn rows_settled(state: &State, count: usize) -> bool {
    state.home.len() == count
        && state
            .home
            .iter()
            .all(|row| !matches!(row.items, Load::Loading))
}

#[tokio::test]
async fn two_catalogs_make_two_rows_that_go_from_loading_to_ready() {
    let server = MockServer::start().await;
    let url = serve_addon(
        &server,
        "one",
        r#"[{"type": "movie", "id": "top", "name": "Popular"},
            {"type": "series", "id": "top", "name": "Shows"}]"#,
    )
    .await;
    // The first catalog answers slowly so its `Loading` state can be observed.
    Mock::given(method("GET"))
        .and(path("/one/catalog/movie/top.json"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(r#"{"metas": [{"id": "tt1", "type": "movie", "name": "A Film", "poster": "http://x/p.jpg"}]}"#)
                .set_delay(Duration::from_millis(300)),
        )
        .mount(&server)
        .await;
    serve_catalog(
        &server,
        "/one/catalog/series/top.json",
        200,
        r#"{"metas": [{"id": "tt2", "type": "series", "name": "A Show"}]}"#,
    )
    .await;
    let dir = tempfile::tempdir().unwrap();
    let core = start(dir.path());
    core.send(Command::InstallAddon { url });

    let loading = wait_for(&core, "two loading rows", |s| s.home.len() == 2).await;
    assert!(loading.home.iter().any(|row| row.items == Load::Loading));
    let state = wait_for(&core, "rows ready", |s| rows_settled(s, 2)).await;

    assert_eq!(state.home[0].addon_name, "one");
    assert_eq!(state.home[0].catalog_name, "Popular");
    assert_eq!(state.home[0].meta_type, "movie");
    let Load::Ready(items) = &state.home[0].items else {
        panic!("not ready: {:?}", state.home[0].items)
    };
    assert_eq!(items[0].id, "tt1");
    assert_eq!(items[0].name, "A Film");
    assert_eq!(items[0].poster.as_deref(), Some("http://x/p.jpg"));
    let Load::Ready(items) = &state.home[1].items else {
        panic!("not ready: {:?}", state.home[1].items)
    };
    assert_eq!(items[0].poster, None);
}

#[tokio::test]
async fn one_failing_addon_does_not_affect_the_other_rows() {
    let server = MockServer::start().await;
    let good = serve_addon(
        &server,
        "good",
        r#"[{"type": "movie", "id": "top", "name": "Good"}]"#,
    )
    .await;
    let bad = serve_addon(
        &server,
        "bad",
        r#"[{"type": "movie", "id": "top", "name": "Bad"}]"#,
    )
    .await;
    serve_catalog(
        &server,
        "/good/catalog/movie/top.json",
        200,
        r#"{"metas": []}"#,
    )
    .await;
    serve_catalog(&server, "/bad/catalog/movie/top.json", 500, "oops").await;
    let dir = tempfile::tempdir().unwrap();
    let core = start(dir.path());
    core.send(Command::InstallAddon { url: good });
    wait_for(&core, "good installed", |s| s.addons.len() == 1).await;
    core.send(Command::InstallAddon { url: bad });

    let state = wait_for(&core, "both rows settled", |s| rows_settled(s, 2)).await;

    let good_row = state
        .home
        .iter()
        .find(|row| row.addon_name == "good")
        .unwrap();
    let bad_row = state
        .home
        .iter()
        .find(|row| row.addon_name == "bad")
        .unwrap();
    assert!(
        matches!(good_row.items, Load::Ready(_)),
        "{:?}",
        good_row.items
    );
    let Load::Failed(message) = &bad_row.items else {
        panic!("not failed: {:?}", bad_row.items)
    };
    assert!(message.contains("500"), "{message}");
}

#[tokio::test]
async fn a_catalog_with_a_required_extra_makes_no_row() {
    let server = MockServer::start().await;
    let url = serve_addon(
        &server,
        "one",
        r#"[{"type": "movie", "id": "top", "name": "Popular", "extra": [{"name": "skip"}]},
            {"type": "movie", "id": "search", "name": "Search",
             "extra": [{"name": "search", "isRequired": true}]}]"#,
    )
    .await;
    serve_catalog(
        &server,
        "/one/catalog/movie/top.json",
        200,
        r#"{"metas": []}"#,
    )
    .await;
    let dir = tempfile::tempdir().unwrap();
    let core = start(dir.path());
    core.send(Command::InstallAddon { url });

    let state = wait_for(&core, "one settled row", |s| rows_settled(s, 1)).await;
    tokio::time::sleep(Duration::from_millis(200)).await;

    assert_eq!(state.home[0].catalog_name, "Popular");
    assert_eq!(core.state().home.len(), 1);
}

// ---- Posters ------------------------------------------------------------------------------

fn png_bytes(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    image::RgbaImage::from_pixel(width, height, image::Rgba([10, 120, 200, 255]))
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .unwrap();
    bytes
}

async fn serve_poster(server: &MockServer, request_path: &str) {
    Mock::given(method("GET"))
        .and(path(request_path))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(png_bytes(600, 900)))
        .mount(server)
        .await;
}

async fn poster_requests(server: &MockServer) -> usize {
    server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|request| request.url.path() == "/poster.png")
        .count()
}

async fn wait_for_poster(core: &Core, url: &str) -> std::sync::Arc<streem_core::Poster> {
    for _ in 0..250 {
        if let Some(poster) = core.poster(url) {
            return poster;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("timed out waiting for the poster {url}");
}

#[tokio::test]
async fn the_same_poster_twice_makes_one_request_and_is_scaled_down() {
    let server = MockServer::start().await;
    serve_poster(&server, "/poster.png").await;
    let url = format!("{}/poster.png", server.uri());
    let dir = tempfile::tempdir().unwrap();
    let core = start(dir.path());

    // The first ask answers nothing and starts the download; asking again while it runs must
    // not start another.
    assert!(core.poster(&url).is_none());
    let _ = core.poster(&url);
    let poster = wait_for_poster(&core, &url).await;
    let again = core.poster(&url).unwrap();

    assert_eq!((poster.width, poster.height), (300, 450));
    assert_eq!(poster.rgba.len(), 300 * 450 * 4);
    assert!(std::sync::Arc::ptr_eq(&poster, &again));
    assert_eq!(poster_requests(&server).await, 1);
}

#[tokio::test]
async fn a_new_core_on_the_same_data_directory_makes_no_request_for_a_saved_poster() {
    let server = MockServer::start().await;
    serve_poster(&server, "/poster.png").await;
    let url = format!("{}/poster.png", server.uri());
    let dir = tempfile::tempdir().unwrap();
    {
        let core = start(dir.path());
        wait_for_poster(&core, &url).await;
    }
    assert_eq!(poster_requests(&server).await, 1);

    let core = start(dir.path());
    let poster = wait_for_poster(&core, &url).await;

    assert_eq!((poster.width, poster.height), (300, 450));
    assert_eq!(poster_requests(&server).await, 1);
}
