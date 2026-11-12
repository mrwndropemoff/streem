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

async fn serve_json(server: &MockServer, request_path: &str, status: u16, body: &str) {
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
    serve_json(
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
    serve_json(
        &server,
        "/good/catalog/movie/top.json",
        200,
        r#"{"metas": []}"#,
    )
    .await;
    serve_json(&server, "/bad/catalog/movie/top.json", 500, "oops").await;
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
    serve_json(
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

// ---- Title details ------------------------------------------------------------------------

/// An **addon** that offers the `meta` resource. `meta_resource` is the JSON of that entry.
async fn serve_meta_addon(server: &MockServer, name: &str, meta_resource: &str) -> String {
    let manifest = format!(
        r#"{{"id": "org.example.{name}", "version": "1.0.0", "name": "{name}",
            "types": ["movie", "series"], "resources": ["catalog", {meta_resource}]}}"#
    );
    Mock::given(method("GET"))
        .and(path(format!("/{name}/manifest.json")))
        .respond_with(ResponseTemplate::new(200).set_body_string(manifest))
        .mount(server)
        .await;
    format!("{}/{name}/manifest.json", server.uri())
}

async fn install(core: &Core, url: String, count: usize) {
    core.send(Command::InstallAddon { url });
    wait_for(core, "addon installed", |s| s.addons.len() == count).await;
}

fn details_settled(state: &State) -> bool {
    state
        .details
        .as_ref()
        .is_some_and(|details| !matches!(details.meta, Load::Loading))
}

async fn open_title(core: &Core, meta_type: &str, meta_id: &str) -> streem_core::Meta {
    core.send(Command::OpenTitle {
        meta_type: meta_type.into(),
        meta_id: meta_id.into(),
    });
    let state = wait_for(core, "details settled", details_settled).await;
    match &state.details.as_ref().unwrap().meta {
        Load::Ready(meta) => meta.clone(),
        other => panic!("not ready: {other:?}"),
    }
}

#[tokio::test]
async fn opening_a_movie_goes_from_loading_to_ready_with_the_fields_filled() {
    let server = MockServer::start().await;
    let url = serve_meta_addon(&server, "one", r#""meta""#).await;
    Mock::given(method("GET"))
        .and(path("/one/meta/movie/tt1.json"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(
                    r#"{"meta": {"id": "tt1", "type": "movie", "name": "A Film",
                        "poster": "http://x/p.jpg", "background": "http://x/b.jpg",
                        "description": "It happens.", "releaseInfo": "2010",
                        "imdbRating": "8.8", "genres": ["Action", "Drama"]}}"#,
                )
                .set_delay(Duration::from_millis(300)),
        )
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let core = start(dir.path());
    install(&core, url, 1).await;

    core.send(Command::OpenTitle {
        meta_type: "movie".into(),
        meta_id: "tt1".into(),
    });
    let loading = wait_for(&core, "details open", |s| s.details.is_some()).await;
    let details = loading.details.as_ref().unwrap();
    assert_eq!(details.meta_type, "movie");
    assert_eq!(details.meta_id, "tt1");
    assert_eq!(details.meta, Load::Loading);

    let state = wait_for(&core, "details settled", details_settled).await;
    let Load::Ready(meta) = &state.details.as_ref().unwrap().meta else {
        panic!("not ready: {:?}", state.details)
    };
    assert_eq!(meta.id, "tt1");
    assert_eq!(meta.meta_type, "movie");
    assert_eq!(meta.name, "A Film");
    assert_eq!(meta.poster.as_deref(), Some("http://x/p.jpg"));
    assert_eq!(meta.background.as_deref(), Some("http://x/b.jpg"));
    assert_eq!(meta.description.as_deref(), Some("It happens."));
    assert_eq!(meta.release_info.as_deref(), Some("2010"));
    assert_eq!(meta.rating.as_deref(), Some("8.8"));
    assert_eq!(meta.genres, ["Action", "Drama"]);
    // A movie has no videos, so it gets one whose id is the meta id.
    assert_eq!(meta.videos.len(), 1);
    assert_eq!(meta.videos[0].id, "tt1");
    assert_eq!(
        (meta.videos[0].season, meta.videos[0].episode),
        (None, None)
    );
}

#[tokio::test]
async fn a_series_returns_its_videos_with_seasons_and_episodes() {
    let server = MockServer::start().await;
    let url = serve_meta_addon(&server, "one", r#""meta""#).await;
    serve_json(
        &server,
        "/one/meta/series/tt2.json",
        200,
        r#"{"meta": {"id": "tt2", "type": "series", "name": "A Show",
            "videos": [
                {"id": "tt2:1:1", "title": "Pilot", "season": 1, "episode": 1,
                 "released": "2011-04-17T00:00:00.000Z"},
                {"id": "tt2:0:1", "name": "Behind the scenes", "season": 0, "episode": 1},
                {"id": "tt2:2:3", "title": "Third", "season": 2, "episode": 3}]}}"#,
    )
    .await;
    let dir = tempfile::tempdir().unwrap();
    let core = start(dir.path());
    install(&core, url, 1).await;

    let meta = open_title(&core, "series", "tt2").await;

    assert_eq!(meta.videos.len(), 3);
    let pilot = &meta.videos[0];
    assert_eq!(pilot.id, "tt2:1:1");
    assert_eq!(pilot.title, "Pilot");
    assert_eq!((pilot.season, pilot.episode), (Some(1), Some(1)));
    assert_eq!(pilot.released.as_deref(), Some("2011-04-17T00:00:00.000Z"));
    // Older **addons** call the title `name`.
    assert_eq!(meta.videos[1].title, "Behind the scenes");
    assert_eq!(meta.videos[1].season, Some(0));
    assert_eq!(meta.videos[2].episode, Some(3));
}

#[tokio::test]
async fn the_addon_whose_id_prefix_matches_is_the_one_asked() {
    let server = MockServer::start().await;
    let first = serve_meta_addon(
        &server,
        "one",
        r#"{"name": "meta", "types": ["movie"], "idPrefixes": ["tt"]}"#,
    )
    .await;
    let second = serve_meta_addon(
        &server,
        "two",
        r#"{"name": "meta", "types": ["movie"], "idPrefixes": ["kitsu"]}"#,
    )
    .await;
    for name in ["one", "two"] {
        serve_json(
            &server,
            &format!("/{name}/meta/movie/kitsu:7.json"),
            200,
            &format!(r#"{{"meta": {{"id": "kitsu:7", "type": "movie", "name": "From {name}"}}}}"#),
        )
        .await;
    }
    let dir = tempfile::tempdir().unwrap();
    let core = start(dir.path());
    install(&core, first, 1).await;
    install(&core, second, 2).await;

    let meta = open_title(&core, "movie", "kitsu:7").await;

    assert_eq!(meta.name, "From two");
    let asked_first = server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .any(|request| request.url.path().starts_with("/one/meta/"));
    assert!(!asked_first);
}

#[tokio::test]
async fn no_matching_addon_gives_failed() {
    let server = MockServer::start().await;
    // One **addon** offers `meta` for series only; the other offers no `meta` at all.
    let series_only =
        serve_meta_addon(&server, "one", r#"{"name": "meta", "types": ["series"]}"#).await;
    let catalog_only = serve_addon(&server, "two", "[]").await;
    let dir = tempfile::tempdir().unwrap();
    let core = start(dir.path());
    install(&core, series_only, 1).await;
    install(&core, catalog_only, 2).await;

    core.send(Command::OpenTitle {
        meta_type: "movie".into(),
        meta_id: "tt1".into(),
    });
    let state = wait_for(&core, "details settled", details_settled).await;

    let Load::Failed(message) = &state.details.as_ref().unwrap().meta else {
        panic!("not failed: {:?}", state.details)
    };
    assert!(message.contains("addon"), "{message}");
}

#[tokio::test]
async fn a_failing_request_gives_failed() {
    let server = MockServer::start().await;
    let url = serve_meta_addon(&server, "one", r#""meta""#).await;
    serve_json(&server, "/one/meta/movie/tt1.json", 500, "oops").await;
    let dir = tempfile::tempdir().unwrap();
    let core = start(dir.path());
    install(&core, url, 1).await;

    core.send(Command::OpenTitle {
        meta_type: "movie".into(),
        meta_id: "tt1".into(),
    });
    let state = wait_for(&core, "details settled", details_settled).await;

    let Load::Failed(message) = &state.details.as_ref().unwrap().meta else {
        panic!("not failed: {:?}", state.details)
    };
    assert!(message.contains("500"), "{message}");
}

#[tokio::test]
async fn closing_the_title_clears_the_details() {
    let server = MockServer::start().await;
    let url = serve_meta_addon(&server, "one", r#""meta""#).await;
    serve_json(
        &server,
        "/one/meta/movie/tt1.json",
        200,
        r#"{"meta": {"id": "tt1", "type": "movie", "name": "A Film"}}"#,
    )
    .await;
    let dir = tempfile::tempdir().unwrap();
    let core = start(dir.path());
    install(&core, url, 1).await;
    open_title(&core, "movie", "tt1").await;

    core.send(Command::CloseTitle);

    wait_for(&core, "details gone", |s| s.details.is_none()).await;
}

// ---- Streams --------------------------------------------------------------------------------

use streem_core::{StreamGroup, StreamList};

/// An **addon** that offers the `stream` resource. `stream_resource` is the JSON of that entry.
async fn serve_stream_addon(server: &MockServer, name: &str, stream_resource: &str) -> String {
    let manifest = format!(
        r#"{{"id": "org.example.{name}", "version": "1.0.0", "name": "Streams {name}",
            "types": ["movie", "series"], "resources": [{stream_resource}]}}"#
    );
    Mock::given(method("GET"))
        .and(path(format!("/{name}/manifest.json")))
        .respond_with(ResponseTemplate::new(200).set_body_string(manifest))
        .mount(server)
        .await;
    format!("{}/{name}/manifest.json", server.uri())
}

/// Installs a meta **addon** named "meta", then the given **addons**, and opens the title.
async fn core_with_title(
    server: &MockServer,
    dir: &Path,
    stream_addons: Vec<String>,
    meta_type: &str,
    meta_id: &str,
) -> Core {
    let meta = serve_meta_addon(server, "meta", r#""meta""#).await;
    serve_json(
        server,
        &format!("/meta/meta/{meta_type}/{meta_id}.json"),
        200,
        &format!(r#"{{"meta": {{"id": "{meta_id}", "type": "{meta_type}", "name": "A Title"}}}}"#),
    )
    .await;
    let core = start(dir);
    install(&core, meta, 1).await;
    for (count, url) in stream_addons.into_iter().enumerate() {
        install(&core, url, count + 2).await;
    }
    open_title(&core, meta_type, meta_id).await;
    core
}

fn stream_list(state: &State) -> Option<&StreamList> {
    state.details.as_ref()?.streams.as_ref()
}

fn streams_settled(state: &State) -> bool {
    stream_list(state).is_some_and(|list| {
        list.groups
            .iter()
            .all(|group| !matches!(group.streams, Load::Loading))
    })
}

fn group<'a>(list: &'a StreamList, addon_id: &str) -> &'a StreamGroup {
    list.groups
        .iter()
        .find(|group| group.addon_id == addon_id)
        .unwrap_or_else(|| panic!("no group for {addon_id}: {list:?}"))
}

async fn load_streams(core: &Core, video_id: &str) -> StreamList {
    core.send(Command::LoadStreams {
        video_id: video_id.into(),
    });
    let state = wait_for(core, "streams settled", |s| {
        streams_settled(s) && stream_list(s).unwrap().video_id == video_id
    })
    .await;
    stream_list(&state).unwrap().clone()
}

#[tokio::test]
async fn two_addons_give_two_groups_that_load_independently() {
    let server = MockServer::start().await;
    let fast = serve_stream_addon(&server, "fast", r#""stream""#).await;
    let slow = serve_stream_addon(&server, "slow", r#""stream""#).await;
    let empty = serve_stream_addon(&server, "empty", r#""stream""#).await;
    serve_json(
        &server,
        "/fast/stream/movie/tt1.json",
        200,
        r#"{"streams": [{"name": "Fast", "description": "1080p", "url": "https://cdn/a.mkv"}]}"#,
    )
    .await;
    Mock::given(method("GET"))
        .and(path("/slow/stream/movie/tt1.json"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(r#"{"streams": [{"name": "Slow", "url": "https://cdn/b.mkv"}]}"#)
                .set_delay(Duration::from_millis(600)),
        )
        .mount(&server)
        .await;
    serve_json(
        &server,
        "/empty/stream/movie/tt1.json",
        200,
        r#"{"streams": []}"#,
    )
    .await;
    let dir = tempfile::tempdir().unwrap();
    let core = core_with_title(&server, dir.path(), vec![fast, slow, empty], "movie", "tt1").await;

    core.send(Command::LoadStreams {
        video_id: "tt1".into(),
    });
    let state = wait_for(&core, "fast group ready while slow loads", |s| {
        stream_list(s).is_some_and(|list| {
            list.groups
                .iter()
                .any(|g| g.addon_id == "org.example.fast" && matches!(g.streams, Load::Ready(_)))
                && list
                    .groups
                    .iter()
                    .any(|g| g.addon_id == "org.example.slow" && matches!(g.streams, Load::Loading))
        })
    })
    .await;
    let list = stream_list(&state).unwrap();
    assert_eq!(list.video_id, "tt1");
    let fast_group = group(list, "org.example.fast");
    assert_eq!(fast_group.addon_name, "Streams fast");
    let Load::Ready(streams) = &fast_group.streams else {
        unreachable!()
    };
    assert_eq!(streams[0].name, "Fast");
    assert_eq!(streams[0].description, "1080p");

    let state = wait_for(&core, "streams settled", streams_settled).await;
    let list = stream_list(&state).unwrap();
    // The **addon** with no streams is dropped once it has loaded.
    let ids: Vec<&str> = list.groups.iter().map(|g| g.addon_id.as_str()).collect();
    assert_eq!(ids, ["org.example.fast", "org.example.slow"]);
    let Load::Ready(streams) = &group(list, "org.example.slow").streams else {
        panic!("slow not ready: {list:?}")
    };
    assert_eq!(streams[0].name, "Slow");
}

#[tokio::test]
async fn each_kind_of_stream_gets_its_playability_and_filename() {
    let server = MockServer::start().await;
    let one = serve_stream_addon(&server, "one", r#""stream""#).await;
    serve_json(
        &server,
        "/one/stream/movie/tt1.json",
        200,
        r#"{"streams": [
            {"name": "Direct", "description": "1080p",
             "url": "https://cdn.example/files/My%20Film.mkv?token=abc"},
            {"name": "Hinted", "title": "Older addons say title",
             "url": "http://cdn.example/play/1", "behaviorHints": {"filename": "Hinted.File.mp4"}},
            {"name": "No extension", "url": "https://cdn.example/play/12345"},
            {"name": "Torrent", "infoHash": "0123456789abcdef0123456789abcdef01234567", "fileIdx": 1},
            {"name": "Video site", "ytId": "abc"}
        ]}"#,
    )
    .await;
    let dir = tempfile::tempdir().unwrap();
    let core = core_with_title(&server, dir.path(), vec![one], "movie", "tt1").await;

    let list = load_streams(&core, "tt1").await;

    let Load::Ready(streams) = &group(&list, "org.example.one").streams else {
        panic!("not ready: {list:?}")
    };
    assert_eq!(streams.len(), 5);

    let direct = &streams[0];
    assert!(direct.playable);
    assert_eq!(direct.unplayable_reason, None);
    assert_eq!(direct.filename.as_deref(), Some("My Film.mkv"));

    let hinted = &streams[1];
    assert!(hinted.playable);
    assert_eq!(hinted.description, "Older addons say title");
    assert_eq!(hinted.filename.as_deref(), Some("Hinted.File.mp4"));

    assert_eq!(streams[2].filename, None);

    let torrent = &streams[3];
    assert!(!torrent.playable);
    assert_eq!(
        torrent.unplayable_reason.as_deref(),
        Some("Torrent streams are not supported")
    );

    let video_site = &streams[4];
    assert!(!video_site.playable);
    assert_eq!(
        video_site.unplayable_reason.as_deref(),
        Some("This kind of stream is not supported")
    );
}

#[tokio::test]
async fn an_addon_without_the_stream_resource_is_not_asked() {
    let server = MockServer::start().await;
    let streams = serve_stream_addon(
        &server,
        "one",
        r#"{"name": "stream", "types": ["series"], "idPrefixes": ["tt"]}"#,
    )
    .await;
    serve_json(
        &server,
        "/one/stream/series/tt2:1:2.json",
        200,
        r#"{"streams": [{"name": "Episode", "url": "https://cdn/e.mkv"}]}"#,
    )
    .await;
    let dir = tempfile::tempdir().unwrap();
    let core = core_with_title(&server, dir.path(), vec![streams], "series", "tt2").await;

    let list = load_streams(&core, "tt2:1:2").await;

    assert_eq!(list.groups.len(), 1);
    assert!(matches!(
        group(&list, "org.example.one").streams,
        Load::Ready(_)
    ));
    // The meta **addon** offers no `stream` resource.
    let asked_meta_addon = server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .any(|request| request.url.path().starts_with("/meta/stream/"));
    assert!(!asked_meta_addon);
}

#[tokio::test]
async fn a_failing_addon_gives_a_failed_group_and_leaves_the_others_intact() {
    let server = MockServer::start().await;
    let good = serve_stream_addon(&server, "good", r#""stream""#).await;
    let bad = serve_stream_addon(&server, "bad", r#""stream""#).await;
    serve_json(
        &server,
        "/good/stream/movie/tt1.json",
        200,
        r#"{"streams": [{"name": "Good", "url": "https://cdn/a.mkv"}]}"#,
    )
    .await;
    serve_json(&server, "/bad/stream/movie/tt1.json", 500, "oops").await;
    let dir = tempfile::tempdir().unwrap();
    let core = core_with_title(&server, dir.path(), vec![good, bad], "movie", "tt1").await;

    let list = load_streams(&core, "tt1").await;

    assert_eq!(list.groups.len(), 2);
    let Load::Failed(message) = &group(&list, "org.example.bad").streams else {
        panic!("not failed: {list:?}")
    };
    assert!(message.contains("500"), "{message}");
    let Load::Ready(streams) = &group(&list, "org.example.good").streams else {
        panic!("not ready: {list:?}")
    };
    assert_eq!(streams[0].name, "Good");
}

#[tokio::test]
async fn an_info_hash_gives_the_torrent_reason_unless_the_url_is_http() {
    let server = MockServer::start().await;
    let one = serve_stream_addon(&server, "one", r#""stream""#).await;
    serve_json(
        &server,
        "/one/stream/movie/tt1.json",
        200,
        r#"{"streams": [
            {"name": "Magnet", "url": "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567",
             "infoHash": "0123456789abcdef0123456789abcdef01234567"},
            {"name": "Link and hash", "url": "https://cdn.example/a.mkv",
             "infoHash": "0123456789abcdef0123456789abcdef01234567"},
            {"name": "Other scheme", "url": "rtmp://live.example/a"}
        ]}"#,
    )
    .await;
    let dir = tempfile::tempdir().unwrap();
    let core = core_with_title(&server, dir.path(), vec![one], "movie", "tt1").await;

    let list = load_streams(&core, "tt1").await;

    let Load::Ready(streams) = &group(&list, "org.example.one").streams else {
        panic!("not ready: {list:?}")
    };
    let reasons: Vec<Option<&str>> = streams
        .iter()
        .map(|stream| stream.unplayable_reason.as_deref())
        .collect();
    assert_eq!(
        reasons,
        [
            Some("Torrent streams are not supported"),
            None,
            Some("This kind of stream is not supported"),
        ]
    );
}

#[tokio::test]
async fn a_url_gives_a_filename_only_when_its_last_segment_has_a_real_extension() {
    let server = MockServer::start().await;
    let one = serve_stream_addon(&server, "one", r#""stream""#).await;
    serve_json(
        &server,
        "/one/stream/movie/tt1.json",
        200,
        r#"{"streams": [
            {"name": "Encoded question mark", "url": "https://cdn.example/a.mkv%3Fx"},
            {"name": "Version number", "url": "https://cdn.example/api/v1.2"},
            {"name": "Fragment", "url": "https://cdn.example/Film.mp4#t=10"}
        ]}"#,
    )
    .await;
    let dir = tempfile::tempdir().unwrap();
    let core = core_with_title(&server, dir.path(), vec![one], "movie", "tt1").await;

    let list = load_streams(&core, "tt1").await;

    let Load::Ready(streams) = &group(&list, "org.example.one").streams else {
        panic!("not ready: {list:?}")
    };
    let filenames: Vec<Option<&str>> = streams
        .iter()
        .map(|stream| stream.filename.as_deref())
        .collect();
    assert_eq!(filenames, [None, None, Some("Film.mp4")]);
}

#[tokio::test]
async fn an_addon_whose_types_or_id_prefixes_do_not_match_is_not_asked() {
    let server = MockServer::start().await;
    let series_only = serve_stream_addon(
        &server,
        "series-only",
        r#"{"name": "stream", "types": ["series"]}"#,
    )
    .await;
    let other_ids = serve_stream_addon(
        &server,
        "other-ids",
        r#"{"name": "stream", "idPrefixes": ["kitsu:"]}"#,
    )
    .await;
    let good = serve_stream_addon(&server, "good", r#""stream""#).await;
    serve_json(
        &server,
        "/good/stream/movie/tt1.json",
        200,
        r#"{"streams": [{"name": "Good", "url": "https://cdn/a.mkv"}]}"#,
    )
    .await;
    let dir = tempfile::tempdir().unwrap();
    let core = core_with_title(
        &server,
        dir.path(),
        vec![series_only, other_ids, good],
        "movie",
        "tt1",
    )
    .await;

    let list = load_streams(&core, "tt1").await;

    let ids: Vec<&str> = list.groups.iter().map(|g| g.addon_id.as_str()).collect();
    assert_eq!(ids, ["org.example.good"]);
    let asked_others = server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .any(|request| {
            let path = request.url.path();
            path.starts_with("/series-only/stream/") || path.starts_with("/other-ids/stream/")
        });
    assert!(!asked_others);
}

#[tokio::test]
async fn streams_for_an_earlier_episode_that_arrive_late_are_ignored() {
    let server = MockServer::start().await;
    let one = serve_stream_addon(&server, "one", r#""stream""#).await;
    Mock::given(method("GET"))
        .and(path("/one/stream/series/tt2:1:1.json"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(
                    r#"{"streams": [{"name": "Episode 1", "url": "https://cdn/1.mkv"}]}"#,
                )
                .set_delay(Duration::from_millis(400)),
        )
        .mount(&server)
        .await;
    serve_json(
        &server,
        "/one/stream/series/tt2:1:2.json",
        200,
        r#"{"streams": [{"name": "Episode 2", "url": "https://cdn/2.mkv"}]}"#,
    )
    .await;
    let dir = tempfile::tempdir().unwrap();
    let core = core_with_title(&server, dir.path(), vec![one], "series", "tt2").await;

    core.send(Command::LoadStreams {
        video_id: "tt2:1:1".into(),
    });
    load_streams(&core, "tt2:1:2").await;
    tokio::time::sleep(Duration::from_millis(900)).await;

    let state = core.state();
    let list = stream_list(&state).unwrap();
    assert_eq!(list.video_id, "tt2:1:2");
    let Load::Ready(streams) = &group(list, "org.example.one").streams else {
        panic!("not ready: {list:?}")
    };
    let names: Vec<&str> = streams.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["Episode 2"]);
}

// ---- Playback -------------------------------------------------------------------------------

use streem_core::{Playback, PlaybackReport};

const FAILED_NOTICE: &str = "This stream could not be played. Pick another one.";

/// A movie (`tt1`) and a series episode (`tt2:1:2`), each with one **addon** offering an
/// unplayable torrent (index 0) and a playable direct link (index 1). Opens the title of
/// `video_id` and loads its streams.
async fn core_with_streams(
    server: &MockServer,
    dir: &Path,
    video_id: &str,
    on_change: impl Fn() + Send + Sync + 'static,
) -> Core {
    let meta = serve_meta_addon(server, "meta", r#""meta""#).await;
    serve_json(
        server,
        "/meta/meta/series/tt2.json",
        200,
        r#"{"meta": {"id": "tt2", "type": "series", "name": "A Show", "poster": "http://x/p.jpg",
            "videos": [{"id": "tt2:1:2", "title": "Second", "season": 1, "episode": 2}]}}"#,
    )
    .await;
    serve_json(
        server,
        "/meta/meta/movie/tt1.json",
        200,
        r#"{"meta": {"id": "tt1", "type": "movie", "name": "A Film"}}"#,
    )
    .await;
    let streams = serve_stream_addon(server, "one", r#""stream""#).await;
    let body = r#"{"streams": [
        {"name": "Torrent", "infoHash": "0123456789abcdef0123456789abcdef01234567"},
        {"name": "Direct", "url": "https://cdn.example/files/Film.mkv"}]}"#;
    serve_json(server, "/one/stream/movie/tt1.json", 200, body).await;
    serve_json(server, "/one/stream/series/tt2:1:2.json", 200, body).await;

    let core = Core::start_with_defaults(dir.to_path_buf(), on_change, Vec::new());
    install(&core, meta, 1).await;
    install(&core, streams, 2).await;
    let (meta_type, meta_id) = match video_id {
        "tt1" => ("movie", "tt1"),
        _ => ("series", "tt2"),
    };
    open_title(&core, meta_type, meta_id).await;
    load_streams(&core, video_id).await;
    core
}

async fn core_ready_to_play(server: &MockServer, dir: &Path) -> Core {
    core_with_streams(server, dir, "tt1", || {}).await
}

fn playback(state: &State) -> &Playback {
    state
        .playback
        .as_ref()
        .unwrap_or_else(|| panic!("nothing is playing: {state:?}"))
}

async fn play_direct(core: &Core) -> std::sync::Arc<State> {
    core.send(Command::Play {
        addon_id: "org.example.one".into(),
        stream_index: 1,
    });
    wait_for(core, "playback started", |s| s.playback.is_some()).await
}

fn report(position_secs: f64, duration_secs: f64) -> PlaybackReport {
    PlaybackReport {
        position_secs,
        duration_secs,
        buffering: false,
        ended: false,
        failed: None,
    }
}

/// Reports a duration, so that a later seek has a range to be clamped to.
async fn report_duration(core: &Core, duration_secs: f64) {
    core.send(Command::ReportPlayback(report(5.0, duration_secs)));
    wait_for(core, "duration known", |s| {
        s.playback
            .as_ref()
            .is_some_and(|p| p.duration_secs == duration_secs)
    })
    .await;
}

async fn wait_for_seek(core: &Core, position_secs: f64) -> std::sync::Arc<State> {
    wait_for(core, "seek", |s| {
        s.playback
            .as_ref()
            .and_then(|p| p.seek.as_ref())
            .is_some_and(|seek| seek.position_secs == position_secs)
    })
    .await
}

#[tokio::test]
async fn playing_a_playable_stream_starts_playback_with_its_link_and_what_is_playing() {
    let server = MockServer::start().await;
    let dir = tempfile::tempdir().unwrap();
    let core = core_ready_to_play(&server, dir.path()).await;
    assert!(core.state().playback.is_none());

    let state = play_direct(&core).await;

    let playback = playback(&state);
    assert_eq!(playback.url, "https://cdn.example/files/Film.mkv");
    assert!(!playback.paused);
    assert_eq!(playback.seek, None);
    assert_eq!(playback.meta_type, "movie");
    assert_eq!(playback.meta_id, "tt1");
    assert_eq!(playback.video_id, "tt1");
    assert_eq!(playback.title, "A Film");
    assert_eq!(playback.addon_id, "org.example.one");
    assert_eq!(playback.stream_name, "Direct");
    assert_eq!(playback.filename.as_deref(), Some("Film.mkv"));
}

#[tokio::test]
async fn an_episode_is_titled_with_its_season_episode_and_title() {
    let server = MockServer::start().await;
    let dir = tempfile::tempdir().unwrap();
    let core = core_with_streams(&server, dir.path(), "tt2:1:2", || {}).await;

    let state = play_direct(&core).await;

    let playback = playback(&state);
    assert_eq!(playback.title, "A Show · S1E2 · Second");
    assert_eq!(playback.video_id, "tt2:1:2");
    assert_eq!(playback.poster.as_deref(), Some("http://x/p.jpg"));
}

#[tokio::test]
async fn playing_an_unplayable_or_missing_stream_changes_nothing() {
    let server = MockServer::start().await;
    let dir = tempfile::tempdir().unwrap();
    let core = core_ready_to_play(&server, dir.path()).await;
    let before = core.state();

    for (addon_id, stream_index) in [("org.example.one", 0), ("org.example.one", 9), ("nope", 1)] {
        core.send(Command::Play {
            addon_id: addon_id.into(),
            stream_index,
        });
    }
    // Commands are handled in order, so a start would show by the time this is handled.
    core.send(Command::DismissNotice);
    tokio::time::sleep(Duration::from_millis(300)).await;

    assert_eq!(*core.state(), *before);
}

#[tokio::test]
async fn pausing_and_resuming_set_paused() {
    let server = MockServer::start().await;
    let dir = tempfile::tempdir().unwrap();
    let core = core_ready_to_play(&server, dir.path()).await;
    play_direct(&core).await;

    core.send(Command::SetPaused(true));
    wait_for(&core, "paused", |s| {
        s.playback.as_ref().is_some_and(|p| p.paused)
    })
    .await;
    core.send(Command::SetPaused(false));
    wait_for(&core, "resumed", |s| {
        s.playback.as_ref().is_some_and(|p| !p.paused)
    })
    .await;
}

#[tokio::test]
async fn each_seek_gets_a_higher_id_and_shows_its_position_at_once() {
    let server = MockServer::start().await;
    let dir = tempfile::tempdir().unwrap();
    let core = core_ready_to_play(&server, dir.path()).await;
    play_direct(&core).await;
    report_duration(&core, 100.0).await;

    core.send(Command::Seek {
        position_secs: 30.0,
    });
    let first = wait_for_seek(&core, 30.0).await;
    core.send(Command::Seek {
        position_secs: 40.0,
    });
    let second = wait_for_seek(&core, 40.0).await;

    let first_id = playback(&first).seek.as_ref().unwrap().id;
    let second_id = playback(&second).seek.as_ref().unwrap().id;
    assert!(second_id > first_id, "{first_id} then {second_id}");
    // The slider moves now, not when the player next reports.
    assert_eq!(playback(&second).position_secs, 40.0);
}

#[tokio::test]
async fn a_seek_beyond_the_duration_or_before_the_start_is_clamped() {
    let server = MockServer::start().await;
    let dir = tempfile::tempdir().unwrap();
    let core = core_ready_to_play(&server, dir.path()).await;
    play_direct(&core).await;
    report_duration(&core, 100.0).await;

    core.send(Command::Seek {
        position_secs: 500.0,
    });
    wait_for_seek(&core, 100.0).await;

    core.send(Command::Seek {
        position_secs: -20.0,
    });
    wait_for_seek(&core, 0.0).await;
}

#[tokio::test]
async fn a_report_fills_in_position_duration_and_buffering() {
    let server = MockServer::start().await;
    let dir = tempfile::tempdir().unwrap();
    let core = core_ready_to_play(&server, dir.path()).await;
    play_direct(&core).await;

    core.send(Command::ReportPlayback(PlaybackReport {
        buffering: true,
        ..report(12.5, 90.0)
    }));
    let state = wait_for(&core, "report applied", |s| {
        s.playback.as_ref().is_some_and(|p| p.buffering)
    })
    .await;

    let playback = playback(&state);
    assert_eq!(playback.position_secs, 12.5);
    assert_eq!(playback.duration_secs, 90.0);
}

#[tokio::test]
async fn an_ended_report_or_stopping_clears_playback() {
    let server = MockServer::start().await;
    let dir = tempfile::tempdir().unwrap();
    let core = core_ready_to_play(&server, dir.path()).await;

    play_direct(&core).await;
    core.send(Command::ReportPlayback(PlaybackReport {
        ended: true,
        ..report(100.0, 100.0)
    }));
    let state = wait_for(&core, "playback cleared", |s| s.playback.is_none()).await;
    assert!(state.notice.is_none());

    play_direct(&core).await;
    core.send(Command::StopPlayback);
    let state = wait_for(&core, "playback cleared", |s| s.playback.is_none()).await;
    assert!(state.notice.is_none());
}

#[tokio::test]
async fn a_failed_report_clears_playback_and_raises_the_notice() {
    let server = MockServer::start().await;
    let dir = tempfile::tempdir().unwrap();
    let core = core_ready_to_play(&server, dir.path()).await;
    play_direct(&core).await;

    core.send(Command::ReportPlayback(PlaybackReport {
        failed: Some("loading failed".into()),
        ..report(0.0, 0.0)
    }));
    let state = wait_for(&core, "playback cleared", |s| s.playback.is_none()).await;

    assert_eq!(state.notice.as_ref().unwrap().text, FAILED_NOTICE);
    // The title stays open, so the person is back at its stream list.
    assert!(stream_list(&state).is_some());
}

#[tokio::test]
async fn commands_and_reports_after_playback_ended_are_ignored() {
    let server = MockServer::start().await;
    let dir = tempfile::tempdir().unwrap();
    let core = core_ready_to_play(&server, dir.path()).await;
    play_direct(&core).await;
    core.send(Command::StopPlayback);
    wait_for(&core, "playback cleared", |s| s.playback.is_none()).await;

    core.send(Command::ReportPlayback(PlaybackReport {
        failed: Some("late".into()),
        ..report(0.0, 0.0)
    }));
    core.send(Command::SetPaused(true));
    core.send(Command::Seek { position_secs: 5.0 });
    core.send(Command::StopPlayback);
    tokio::time::sleep(Duration::from_millis(300)).await;

    let state = core.state();
    assert!(state.playback.is_none());
    assert!(state.notice.is_none());
}

#[tokio::test]
async fn position_reports_publish_at_most_once_a_second() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    let server = MockServer::start().await;
    let dir = tempfile::tempdir().unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let core = core_with_streams(&server, dir.path(), "tt1", move || {
        counter.fetch_add(1, Ordering::SeqCst);
    })
    .await;
    play_direct(&core).await;
    report_duration(&core, 100.0).await;

    let before = calls.load(Ordering::SeqCst);
    for step in 0..50 {
        core.send(Command::ReportPlayback(report(
            6.0 + f64::from(step) * 0.01,
            100.0,
        )));
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let during = calls.load(Ordering::SeqCst) - before;

    assert!(
        during <= 1,
        "on_change was called {during} times in about half a second"
    );

    // Held back is not lost: a position report a second later is published.
    tokio::time::sleep(Duration::from_millis(1100)).await;
    core.send(Command::ReportPlayback(report(50.0, 100.0)));
    wait_for(&core, "position published", |s| {
        s.playback.as_ref().is_some_and(|p| p.position_secs == 50.0)
    })
    .await;
}

#[tokio::test]
async fn the_link_never_shows_up_in_debug_output() {
    let server = MockServer::start().await;
    let dir = tempfile::tempdir().unwrap();
    let core = core_ready_to_play(&server, dir.path()).await;

    let state = play_direct(&core).await;

    assert!(!format!("{state:?}").contains("cdn.example"));
    assert!(!format!("{:?}", playback(&state)).contains("cdn.example"));
}
