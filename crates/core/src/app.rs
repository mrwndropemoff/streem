//! The single task that owns the mutable [`State`].
//!
//! It never waits on the network or the disk. It spawns that work and gets the result back
//! as a [`Message`]. All disk work goes through one worker, so results arrive in the order the
//! writes were made.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use tokio::sync::{mpsc, oneshot};

use crate::addon::client::{
    ResolvedAddon, fetch_catalog, fetch_manifest, fetch_meta, fetch_streams, fetch_subtitles,
    http_client, normalize_url, parse_manifest,
};
use crate::addon::types::{
    Manifest, ManifestCatalog, Meta, MetaPreview, Stream, StreamSource, is_http,
};
use crate::addon::url::{has_placeholder, templatize};
use crate::playback::{self, Links, Reported};
use crate::posters::{self, Poster, PosterCache};
use crate::settings::{self, Settings};
use crate::store::{Store, StoreError, StoredAddon};
use crate::{
    CatalogRow, Command, Details, InstalledAddon, Load, Notice, Playback, Search, State,
    StreamGroup, StreamItem, StreamList, SubtitleTrack,
};

/// Position-only reports from the player are published at most this often.
const POSITION_PUBLISH_INTERVAL: Duration = Duration::from_secs(1);

/// What the task receives: the UI's commands, and results of work it started.
pub(crate) enum Message {
    Command(Command),
    /// The install started with this id finished downloading: the valid manifest text, or text
    /// for a [`Notice`].
    Fetched(u64, Result<String, String>),
    /// A disk job finished. The error is text for a [`Notice`].
    Stored(Result<StoreOutput, String>),
    /// A position held back by `POSITION_PUBLISH_INTERVAL` may now be published. Sent by the
    /// task's own timer, not by the UI.
    PublishHeldBack,
    /// The UI asked for a poster it has not asked for before.
    LoadPoster(String),
    /// A poster finished loading, or could not be.
    PosterLoaded(String, Result<Poster, String>),
    /// A home or search row finished loading. Ignored when `generation` is not the latest one
    /// for that list, which means its rows were rebuilt (or the search was cleared) since the
    /// request was made.
    Catalog {
        list: RowList,
        generation: u64,
        row: usize,
        result: Result<Vec<MetaPreview>, String>,
    },
    /// The details of a title finished loading. Ignored when `generation` is not the latest one,
    /// which means the title was closed or another one was opened since the request was made.
    Meta {
        generation: u64,
        result: Result<Meta, String>,
    },
    /// One **addon** answered a stream request. Ignored when `generation` is not the latest
    /// one, which means other streams were asked for, or the title was closed, since then.
    Streams {
        generation: u64,
        addon_url: String,
        result: Result<Vec<Stream>, String>,
    },
    /// One **addon** answered a subtitles request with these tracks. A failing **addon** sends
    /// nothing. Ignored when `generation` is not the latest one, which means another stream
    /// started since the request was made, and also when nothing is playing any more.
    Subtitles {
        generation: u64,
        tracks: Vec<SubtitleTrack>,
    },
}

/// Which rows a [`Message::Catalog`] belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RowList {
    Home,
    Search,
}

/// The **addon** list as saved, after a disk job.
pub(crate) struct StoreOutput {
    /// `skipped_reason` is not set yet; the task sets it from its own settings.
    addons: Vec<InstalledAddon>,
    /// The saved settings, read by the startup load only.
    settings: Option<Settings>,
    /// The job was a startup load, an install or a removal.
    addons_changed: bool,
    /// The catalogs the home screen can show, from the saved manifests.
    catalogs: Vec<CatalogSource>,
    /// The catalogs a search asks, from the saved manifests.
    search_catalogs: Vec<CatalogSource>,
    /// The saved manifests, by **addon** URL, in **addon** order. Used to pick who answers a
    /// meta request. An **addon** whose manifest was never fetched is not here.
    manifests: Vec<(String, Manifest)>,
    /// Set by the first load only: the data directory had no **addon** rows at all.
    first_launch: bool,
}

/// A catalog that gets a row on the home or search screen, with the **addon** that offers it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct CatalogSource {
    addon_url: String,
    addon_name: String,
    catalog: ManifestCatalog,
}

enum StoreJob {
    Load,
    /// An empty manifest saves an **addon** whose manifest cannot be fetched until a
    /// Real-Debrid key is saved.
    Upsert {
        url: String,
        manifest_json: String,
    },
    Remove {
        url: String,
    },
    SaveRealDebridKey(Option<String>),
    SaveLanguages {
        audio: Option<String>,
        subtitle: Option<String>,
    },
}

pub(crate) struct App {
    pub(crate) data_dir: PathBuf,
    pub(crate) on_change: Arc<dyn Fn() + Send + Sync>,
    pub(crate) default_addon_urls: Vec<String>,
    pub(crate) state: Arc<RwLock<Arc<State>>>,
    pub(crate) posters: Arc<PosterCache>,
    pub(crate) sender: mpsc::UnboundedSender<Message>,
    pub(crate) inbox: mpsc::UnboundedReceiver<Message>,
    pub(crate) shutdown: oneshot::Receiver<()>,
}

impl App {
    pub(crate) async fn run(mut self) {
        let (jobs, job_inbox) = mpsc::unbounded_channel();
        let worker = tokio::spawn(store_worker(
            self.data_dir.clone(),
            job_inbox,
            self.sender.clone(),
        ));
        let _ = jobs.send(StoreJob::Load);

        let client = http_client();
        let mut state = State::default();
        let mut published = Arc::new(state.clone());
        let mut next_notice_id = 1;
        // Installs still downloading, by id, with the **addon** URL. Removing it cancels them.
        let mut downloading: HashMap<u64, String> = HashMap::new();
        let mut next_download_id = 1;
        // What the home rows were built from, and which build the running fetches belong to.
        let mut catalogs: Vec<CatalogSource> = Vec::new();
        let mut home_generation = 0;
        // The same for the search rows.
        let mut search_catalogs: Vec<CatalogSource> = Vec::new();
        let mut search_generation = 0;
        // The manifests of the installed **addons**, and which open title the running fetch
        // belongs to.
        let mut manifests: Vec<(String, Manifest)> = Vec::new();
        let mut details_generation = 0;
        // The **addon** URL of each stream group, in the same order as the groups, and which
        // stream request the running fetches belong to.
        let mut stream_addon_urls: Vec<String> = Vec::new();
        let mut streams_generation = 0;
        // The links of the streams in those groups. The UI never sees them.
        let mut stream_links: Links = Vec::new();
        // Which playback the running subtitles fetches belong to. Grows with every stream
        // that starts playing.
        let mut playback_generation = 0;
        // When a report from the player was last published, for `POSITION_PUBLISH_INTERVAL`.
        let mut last_report_published: Option<Instant> = None;
        // When a held-back position must be published, if the player sends nothing before then.
        let mut held_back_until: Option<Instant> = None;
        // The person changed the key, or the languages, before the startup load finished;
        // the saved value it brings is then older and is not used.
        let mut key_set = false;
        let mut languages_set = false;

        loop {
            // The future is built even when the branch is off, so it needs some instant.
            let deadline = held_back_until.unwrap_or_else(Instant::now);
            let message = tokio::select! {
                message = self.inbox.recv() => message,
                () = tokio::time::sleep_until(deadline.into()), if held_back_until.is_some() => {
                    Some(Message::PublishHeldBack)
                }
                _ = &mut self.shutdown => None,
            };
            let Some(message) = message else { break };

            let mut to_install = Vec::new();
            // Installed **addons** whose manifest is fetched again, by stored URL.
            let mut to_refresh = Vec::new();
            // A Real-Debrid key was saved: whether it differs from the one before.
            let mut key_saved = None;
            // The startup load finished.
            let mut first_load = false;
            let mut reload_home = false;
            // Set when the state changed only by a position nobody needs to hear about yet.
            let mut hold_back = false;
            match message {
                Message::PublishHeldBack => last_report_published = Some(Instant::now()),
                Message::Command(Command::InstallAddon { url }) => to_install.push(url),
                Message::Command(Command::LoadHome) => reload_home = true,
                Message::LoadPoster(url) => {
                    spawn_poster(&client, &self.data_dir, self.sender.clone(), url);
                }
                Message::PosterLoaded(url, result) => {
                    self.posters.finish(url, result);
                    // Posters are not part of `State`, so the UI is told directly.
                    (self.on_change)();
                }
                Message::Catalog {
                    list,
                    generation,
                    row,
                    result,
                } => {
                    let rows = match list {
                        RowList::Home if generation == home_generation => Some(&mut state.home),
                        RowList::Search if generation == search_generation => {
                            state.search.as_mut().map(|search| &mut search.rows)
                        }
                        _ => None,
                    };
                    if let Some(row) = rows.and_then(|rows| rows.get_mut(row)) {
                        row.items = match result {
                            Ok(items) => Load::Ready(items),
                            Err(text) => Load::Failed(text),
                        };
                    }
                }
                Message::Command(Command::Search { query }) => {
                    search_generation += 1;
                    let query = query.trim();
                    state.search = (!query.is_empty()).then(|| Search {
                        query: query.to_string(),
                        rows: start_rows(
                            &client,
                            &self.sender,
                            &search_catalogs,
                            &state.settings,
                            Some(query),
                            RowList::Search,
                            search_generation,
                        ),
                    });
                }
                Message::Command(Command::ClearSearch) => {
                    search_generation += 1;
                    state.search = None;
                }
                Message::Command(Command::OpenTitle { meta_type, meta_id }) => {
                    details_generation += 1;
                    streams_generation += 1;
                    stream_addon_urls.clear();
                    stream_links.clear();
                    let meta = start_meta(
                        &client,
                        &self.sender,
                        &manifests,
                        &state.settings,
                        &meta_type,
                        &meta_id,
                        details_generation,
                    );
                    state.details = Some(Details {
                        meta_type,
                        meta_id,
                        meta,
                        streams: None,
                    });
                }
                Message::Command(Command::CloseTitle) => {
                    details_generation += 1;
                    streams_generation += 1;
                    stream_addon_urls.clear();
                    stream_links.clear();
                    state.details = None;
                    playback::stop(&mut state);
                }
                Message::Command(Command::LoadStreams { video_id }) => {
                    if let Some(details) = state.details.as_mut() {
                        streams_generation += 1;
                        let (sources, list) = start_streams(
                            &client,
                            &self.sender,
                            &manifests,
                            &state.settings,
                            &details.meta_type,
                            video_id,
                            streams_generation,
                        );
                        stream_addon_urls = sources;
                        stream_links = list.groups.iter().map(|_| Vec::new()).collect();
                        details.streams = Some(list);
                    }
                }
                Message::Command(Command::Play {
                    addon_id,
                    stream_index,
                }) => {
                    if playback::play(&mut state, &stream_links, &addon_id, stream_index)
                        && let Some(playback) = &state.playback
                    {
                        playback_generation += 1;
                        start_subtitles(
                            &client,
                            &self.sender,
                            &manifests,
                            &state.settings,
                            playback,
                            playback_generation,
                        );
                    }
                }
                Message::Subtitles { generation, tracks } => {
                    if generation == playback_generation {
                        playback::add_subtitles(&mut state, tracks);
                    }
                }
                Message::Command(Command::SetPaused(paused)) => {
                    playback::set_paused(&mut state, paused);
                }
                Message::Command(Command::Seek { position_secs }) => {
                    playback::seek(&mut state, position_secs);
                }
                Message::Command(Command::StopPlayback) => playback::stop(&mut state),
                Message::Command(Command::ReportPlayback(report)) => {
                    match playback::report(&mut state, report) {
                        Reported::Ignored => {}
                        Reported::Failed => {
                            let text = playback::FAILED_NOTICE.to_string();
                            state.notice = Some(notice(&mut next_notice_id, text));
                            last_report_published = Some(Instant::now());
                        }
                        Reported::Changed => last_report_published = Some(Instant::now()),
                        Reported::PositionOnly => {
                            let recent = last_report_published
                                .filter(|at| at.elapsed() < POSITION_PUBLISH_INTERVAL);
                            if let Some(at) = recent {
                                hold_back = true;
                                held_back_until = Some(at + POSITION_PUBLISH_INTERVAL);
                            } else {
                                last_report_published = Some(Instant::now());
                            }
                        }
                    }
                }
                Message::Streams {
                    generation,
                    addon_url,
                    result,
                } => {
                    if generation == streams_generation
                        && let Some(list) = state
                            .details
                            .as_mut()
                            .and_then(|details| details.streams.as_mut())
                        && let Some(index) =
                            stream_addon_urls.iter().position(|url| *url == addon_url)
                    {
                        match result {
                            Ok(streams) if streams.is_empty() => {
                                list.groups.remove(index);
                                stream_addon_urls.remove(index);
                                stream_links.remove(index);
                            }
                            Ok(streams) => {
                                stream_links[index] = streams.iter().map(stream_link).collect();
                                list.groups[index].streams =
                                    Load::Ready(streams.into_iter().map(stream_item).collect());
                            }
                            Err(text) => list.groups[index].streams = Load::Failed(text),
                        }
                    }
                }
                Message::Meta { generation, result } => {
                    if generation == details_generation
                        && let Some(details) = state.details.as_mut()
                    {
                        details.meta = match result {
                            Ok(meta) => Load::Ready(meta),
                            Err(text) => Load::Failed(text),
                        };
                    }
                }
                Message::Command(Command::RemoveAddon { url }) => {
                    downloading.retain(|_, downloading_url| *downloading_url != url);
                    let _ = jobs.send(StoreJob::Remove { url });
                }
                Message::Command(Command::DismissNotice) => state.notice = None,
                // A setting changes in `State` at once, so a command right after it, such as
                // an install, already uses it; the save follows on the disk worker.
                Message::Command(
                    command @ (Command::SetRealDebridKey(_) | Command::SetPlaybackLanguages { .. }),
                ) => {
                    let Some(job) = save_settings_job(command) else {
                        continue;
                    };
                    match &job {
                        StoreJob::SaveRealDebridKey(key) => {
                            key_set = true;
                            let changed = state.settings.key() != key.as_deref();
                            state.settings.real_debrid_key = key.clone();
                            key_saved = Some(changed);
                        }
                        StoreJob::SaveLanguages { audio, subtitle } => {
                            languages_set = true;
                            state.settings.audio_lang = audio.clone();
                            state.settings.subtitle_lang = subtitle.clone();
                        }
                        _ => {}
                    }
                    let _ = jobs.send(job);
                }
                Message::Fetched(id, result) => {
                    // Not in the map: the **addon** was removed while it was downloading.
                    if let Some(url) = downloading.remove(&id) {
                        match result {
                            Ok(manifest_json) => {
                                let _ = jobs.send(StoreJob::Upsert { url, manifest_json });
                            }
                            Err(text) => {
                                state.notice = Some(notice(&mut next_notice_id, text));
                            }
                        }
                    }
                }
                Message::Stored(Ok(output)) => {
                    if let Some(saved) = output.settings {
                        first_load = true;
                        // A setting changed before the startup load finished is newer.
                        if !key_set {
                            state.settings.real_debrid_key = saved.real_debrid_key;
                        }
                        if !languages_set {
                            state.settings.audio_lang = saved.audio_lang;
                            state.settings.subtitle_lang = saved.subtitle_lang;
                        }
                    }
                    state.addons = output.addons;
                    mark_skipped(&mut state.addons, &state.settings);
                    catalogs = output.catalogs;
                    search_catalogs = output.search_catalogs;
                    manifests = output.manifests;
                    reload_home = output.addons_changed;
                    if output.first_launch {
                        to_install.extend(self.default_addon_urls.iter().cloned());
                    }
                }
                Message::Stored(Err(text)) => {
                    state.notice = Some(notice(&mut next_notice_id, text));
                }
            }

            if let Some(changed) = key_saved {
                mark_skipped(&mut state.addons, &state.settings);
                if changed {
                    // With a key, the **addons** that need one can answer now; without one
                    // they are skipped, and there is nothing to fetch.
                    reload_home = true;
                    to_refresh.extend(
                        state
                            .addons
                            .iter()
                            .filter(|addon| has_placeholder(&addon.url))
                            .map(|addon| addon.url.clone()),
                    );
                }
            }
            if key_saved.is_some() || first_load {
                // Also a retry for an **addon** whose manifest could not be fetched before.
                to_refresh.extend(
                    state
                        .addons
                        .iter()
                        .filter(|addon| !manifests.iter().any(|(url, _)| *url == addon.url))
                        .map(|addon| addon.url.clone()),
                );
            }

            if reload_home {
                home_generation += 1;
                state.home = start_rows(
                    &client,
                    &self.sender,
                    &catalogs,
                    &state.settings,
                    None,
                    RowList::Home,
                    home_generation,
                );
            }

            for input in to_install {
                // First, so that no notice below can show the key.
                let input = templatize(&input, &state.settings);
                let url = match normalize_url(&input) {
                    Ok(url) => url,
                    Err(error) => {
                        let text = format!("Could not install {}: {error}", input.trim());
                        state.notice = Some(notice(&mut next_notice_id, text));
                        continue;
                    }
                };
                match ResolvedAddon::resolve(&url, &state.settings) {
                    Ok(addon) => {
                        let id = next_download_id;
                        next_download_id += 1;
                        downloading.insert(id, url.clone());
                        let sender = self.sender.clone();
                        spawn_download(&client, sender, id, addon, Fetch::Install, url);
                    }
                    // Listed and skipped until a key is saved, which fetches its manifest.
                    Err(_) if state.addons.iter().any(|addon| addon.url == url) => {}
                    Err(_) => {
                        let manifest_json = String::new();
                        let _ = jobs.send(StoreJob::Upsert { url, manifest_json });
                    }
                }
            }
            for url in to_refresh {
                if downloading
                    .values()
                    .any(|downloading_url| *downloading_url == url)
                {
                    continue;
                }
                if let Ok(addon) = ResolvedAddon::resolve(&url, &state.settings) {
                    let id = next_download_id;
                    next_download_id += 1;
                    downloading.insert(id, url.clone());
                    let sender = self.sender.clone();
                    spawn_download(&client, sender, id, addon, Fetch::Refresh, url);
                }
            }
            if !hold_back {
                // Whatever was held back goes out now with the rest of the state.
                held_back_until = None;
            }
            if !hold_back && *published != state {
                published = Arc::new(state.clone());
                *self
                    .state
                    .write()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()) = published.clone();
                (self.on_change)();
            }
        }

        // Shutting down. Keep what the person already asked for: removals and downloaded
        // manifests still waiting in the inbox become disk jobs, then wait for the disk
        // worker to finish every queued job. Downloads still in progress are given up.
        while let Ok(message) = self.inbox.try_recv() {
            match message {
                Message::Command(Command::RemoveAddon { url }) => {
                    downloading.retain(|_, downloading_url| *downloading_url != url);
                    let _ = jobs.send(StoreJob::Remove { url });
                }
                Message::Fetched(id, Ok(manifest_json)) => {
                    if let Some(url) = downloading.remove(&id) {
                        let _ = jobs.send(StoreJob::Upsert { url, manifest_json });
                    }
                }
                Message::Command(command) => {
                    if let Some(job) = save_settings_job(command) {
                        let _ = jobs.send(job);
                    }
                }
                _ => {}
            }
        }
        drop(jobs);
        let _ = worker.await;
    }
}

/// The disk job that saves a settings command, with blank values made `None`. `None` for any
/// other command.
fn save_settings_job(command: Command) -> Option<StoreJob> {
    match command {
        Command::SetRealDebridKey(key) => {
            Some(StoreJob::SaveRealDebridKey(settings::non_blank(key)))
        }
        Command::SetPlaybackLanguages { audio, subtitle } => Some(StoreJob::SaveLanguages {
            audio: settings::non_blank(audio),
            subtitle: settings::non_blank(subtitle),
        }),
        _ => None,
    }
}

/// Sets each **addon**'s `skipped_reason`: whether its URL needs a key that is not saved.
fn mark_skipped(addons: &mut [InstalledAddon], settings: &Settings) {
    for addon in addons {
        addon.skipped_reason = ResolvedAddon::resolve(&addon.url, settings)
            .err()
            .map(|missing| missing.to_string());
    }
}

/// Why a manifest is downloaded, for the notice when it fails.
#[derive(Clone, Copy)]
enum Fetch {
    Install,
    /// An installed **addon**, after the Real-Debrid key changed or when its manifest was
    /// never fetched.
    Refresh,
}

impl Fetch {
    fn verb(self) -> &'static str {
        match self {
            Fetch::Install => "install",
            Fetch::Refresh => "refresh",
        }
    }
}

fn notice(next_id: &mut u64, text: String) -> Notice {
    let id = *next_id;
    *next_id += 1;
    Notice { id, text }
}

/// Downloads and checks the manifest in the background, then reports back. A failure says
/// "Could not {verb} {url}", where `url` is the stored one, never the resolved one.
fn spawn_download(
    client: &reqwest::Client,
    sender: mpsc::UnboundedSender<Message>,
    id: u64,
    addon: ResolvedAddon,
    fetch: Fetch,
    url: String,
) {
    let client = client.clone();
    tokio::spawn(async move {
        let result = fetch_manifest(&client, &addon)
            .await
            .map(|(_manifest, text)| text)
            .map_err(|error| format!("Could not {} {url}: {error}", fetch.verb()));
        let _ = sender.send(Message::Fetched(id, result));
    });
}

/// Builds a `Loading` row for each catalog and fetches them all at once: the catalog itself,
/// or with a `query` a search in it. Each row is answered on its own, so one failing **addon**
/// never touches the others. An **addon** that is skipped for want of a key gets no row.
fn start_rows(
    client: &reqwest::Client,
    sender: &mpsc::UnboundedSender<Message>,
    catalogs: &[CatalogSource],
    settings: &Settings,
    query: Option<&str>,
    list: RowList,
    generation: u64,
) -> Vec<CatalogRow> {
    let mut rows = Vec::with_capacity(catalogs.len());
    for source in catalogs {
        let Ok(addon) = ResolvedAddon::resolve(&source.addon_url, settings) else {
            continue;
        };
        let row = rows.len();
        rows.push(CatalogRow {
            addon_name: source.addon_name.clone(),
            catalog_name: source.catalog.display_name().to_string(),
            meta_type: source.catalog.kind.clone(),
            items: Load::Loading,
        });

        let client = client.clone();
        let sender = sender.clone();
        let catalog = source.catalog.clone();
        let query = query.map(str::to_string);
        tokio::spawn(async move {
            let result = fetch_catalog(
                &client,
                &addon,
                &catalog.kind,
                &catalog.id,
                query.as_deref(),
            )
            .await;
            let result = result.map_err(|error| error.to_string());
            let _ = sender.send(Message::Catalog {
                list,
                generation,
                row,
                result,
            });
        });
    }
    rows
}

/// Picks the **addon** to ask about a title: the first installed one that offers `meta` for
/// the type and id and is not skipped for want of a key. Fetches in the background and
/// returns `Loading`, or `Failed` when no **addon** matches.
fn start_meta(
    client: &reqwest::Client,
    sender: &mpsc::UnboundedSender<Message>,
    manifests: &[(String, Manifest)],
    settings: &Settings,
    meta_type: &str,
    meta_id: &str,
    generation: u64,
) -> Load<Meta> {
    let Some(addon) = manifests
        .iter()
        .filter(|(_, manifest)| manifest.offers("meta", meta_type, meta_id))
        .find_map(|(addon_url, _)| ResolvedAddon::resolve(addon_url, settings).ok())
    else {
        return Load::Failed(format!(
            "No installed addon can describe {meta_type} {meta_id}."
        ));
    };

    let client = client.clone();
    let sender = sender.clone();
    let meta_type = meta_type.to_string();
    let meta_id = meta_id.to_string();
    tokio::spawn(async move {
        let result = fetch_meta(&client, &addon, &meta_type, &meta_id).await;
        let result = result.map_err(|error| error.to_string());
        let _ = sender.send(Message::Meta { generation, result });
    });
    Load::Loading
}

/// Builds a `Loading` group for every installed **addon** that offers `stream` for the type and
/// video id, and fetches them all at once. Each group is answered on its own, so one failing
/// **addon** never touches the others. An **addon** that is skipped for want of a key gets no
/// group. Returns the stored **addon** URL of each group with the list.
fn start_streams(
    client: &reqwest::Client,
    sender: &mpsc::UnboundedSender<Message>,
    manifests: &[(String, Manifest)],
    settings: &Settings,
    meta_type: &str,
    video_id: String,
    generation: u64,
) -> (Vec<String>, StreamList) {
    let mut sources = Vec::new();
    let mut groups = Vec::new();
    for (addon_url, manifest) in manifests {
        if !manifest.offers("stream", meta_type, &video_id) {
            continue;
        }
        let Ok(addon) = ResolvedAddon::resolve(addon_url, settings) else {
            continue;
        };
        sources.push(addon_url.clone());
        groups.push(StreamGroup {
            addon_id: manifest.id.clone(),
            addon_name: manifest.name.clone(),
            streams: Load::Loading,
        });

        let client = client.clone();
        let sender = sender.clone();
        let addon_url = addon_url.clone();
        let meta_type = meta_type.to_string();
        let video_id = video_id.clone();
        tokio::spawn(async move {
            let result = fetch_streams(&client, &addon, &meta_type, &video_id).await;
            let result = result.map_err(|error| error.to_string());
            let _ = sender.send(Message::Streams {
                generation,
                addon_url,
                result,
            });
        });
    }
    (sources, StreamList { video_id, groups })
}

/// Asks every installed **addon** that offers `subtitles` for the playing video, all at once.
/// Each answer comes back on its own; a failing **addon** is ignored silently, because missing
/// subtitles must never stop playback or raise a notice. Only `http://` and `https://` links
/// are kept, since the player opens whatever it is given. An **addon** that is skipped for
/// want of a key is not asked.
fn start_subtitles(
    client: &reqwest::Client,
    sender: &mpsc::UnboundedSender<Message>,
    manifests: &[(String, Manifest)],
    settings: &Settings,
    playback: &Playback,
    generation: u64,
) {
    for (addon_url, manifest) in manifests {
        if !manifest.offers("subtitles", &playback.meta_type, &playback.video_id) {
            continue;
        }
        let Ok(addon) = ResolvedAddon::resolve(addon_url, settings) else {
            continue;
        };
        let client = client.clone();
        let sender = sender.clone();
        let meta_type = playback.meta_type.clone();
        let video_id = playback.video_id.clone();
        let addon_name = manifest.name.clone();
        tokio::spawn(async move {
            let Ok(subtitles) = fetch_subtitles(&client, &addon, &meta_type, &video_id).await
            else {
                return;
            };
            let tracks = subtitles
                .into_iter()
                .filter(|subtitle| is_http(&subtitle.url))
                .map(|subtitle| SubtitleTrack {
                    lang: subtitle.lang.unwrap_or_default(),
                    url: subtitle.url,
                    addon_name: addon_name.clone(),
                })
                .collect();
            let _ = sender.send(Message::Subtitles { generation, tracks });
        });
    }
}

/// The link to play a stream from, kept inside `Core`. Only `http://` and `https://` links can
/// be played, which is also what [`stream_item`] reports as `playable`.
fn stream_link(stream: &Stream) -> Option<String> {
    match &stream.source {
        StreamSource::Url(url) if is_http(url) => Some(url.clone()),
        _ => None,
    }
}

/// What the UI is told about a stream. Only `http://` and `https://` links can be played.
fn stream_item(stream: Stream) -> StreamItem {
    let unplayable_reason = match &stream.source {
        StreamSource::Url(url) if is_http(url) => None,
        StreamSource::Torrent => Some("Torrent streams are not supported"),
        StreamSource::Url(_) | StreamSource::Other => Some("This kind of stream is not supported"),
    };
    StreamItem {
        name: stream.name,
        description: stream.description,
        filename: stream.filename,
        playable: unplayable_reason.is_none(),
        unplayable_reason: unplayable_reason.map(str::to_string),
    }
}

/// Loads a poster in the background, then reports back.
fn spawn_poster(
    client: &reqwest::Client,
    data_dir: &std::path::Path,
    sender: mpsc::UnboundedSender<Message>,
    url: String,
) {
    let client = client.clone();
    let data_dir = data_dir.to_path_buf();
    tokio::spawn(async move {
        let result = posters::load(&client, &data_dir, &url).await;
        let _ = sender.send(Message::PosterLoaded(url, result));
    });
}

/// Runs disk jobs one at a time, each on a blocking thread.
async fn store_worker(
    data_dir: PathBuf,
    mut jobs: mpsc::UnboundedReceiver<StoreJob>,
    results: mpsc::UnboundedSender<Message>,
) {
    let opened = tokio::task::spawn_blocking(move || Store::open(&data_dir)).await;
    let store: Result<Store, String> = match opened {
        Ok(Ok(store)) => Ok(store),
        Ok(Err(error)) => Err(error.to_string()),
        Err(error) => Err(error.to_string()),
    };

    while let Some(job) = jobs.recv().await {
        let result = match &store {
            Ok(store) => {
                let store = store.clone();
                match tokio::task::spawn_blocking(move || run_job(&store, job)).await {
                    Ok(done) => done.map_err(|error| error.to_string()),
                    Err(error) => Err(error.to_string()),
                }
            }
            Err(error) => Err(error.clone()),
        };
        if results.send(Message::Stored(result)).is_err() {
            break;
        }
    }
}

fn run_job(store: &Store, job: StoreJob) -> Result<StoreOutput, StoreError> {
    let mut first_launch = false;
    let mut addons_changed = true;
    let mut settings = None;
    match job {
        StoreJob::Load => {
            first_launch = store.has_no_addon_rows()?;
            settings = Some(store.load_settings()?);
        }
        StoreJob::Upsert { url, manifest_json } => store.upsert_addon(&url, &manifest_json)?,
        StoreJob::Remove { url } => store.remove_addon(&url)?,
        StoreJob::SaveRealDebridKey(key) => {
            addons_changed = false;
            store.save_real_debrid_key(key.as_deref())?;
        }
        StoreJob::SaveLanguages { audio, subtitle } => {
            addons_changed = false;
            store.save_languages(audio.as_deref(), subtitle.as_deref())?;
        }
    }
    let mut addons = Vec::new();
    let mut catalogs = Vec::new();
    let mut search_catalogs = Vec::new();
    let mut manifests = Vec::new();
    for stored in store.list_addons()? {
        let Some((addon, manifest)) = installed_addon(stored) else {
            continue;
        };
        let Some(manifest) = manifest else {
            addons.push(addon);
            continue;
        };
        for catalog in &manifest.catalogs {
            let source = || CatalogSource {
                addon_url: addon.url.clone(),
                addon_name: addon.name.clone(),
                catalog: catalog.clone(),
            };
            if !catalog.has_required_extra() {
                catalogs.push(source());
            }
            if catalog.supports_search() {
                search_catalogs.push(source());
            }
        }
        manifests.push((addon.url.clone(), manifest));
        addons.push(addon);
    }
    Ok(StoreOutput {
        addons,
        settings,
        addons_changed,
        catalogs,
        search_catalogs,
        manifests,
        first_launch,
    })
}

/// Returns the **addon** with its manifest, or with none when the manifest was never fetched.
/// A saved row whose manifest can no longer be read is left out of the list.
fn installed_addon(stored: StoredAddon) -> Option<(InstalledAddon, Option<Manifest>)> {
    if stored.manifest_json.is_empty() {
        let name = reqwest::Url::parse(&stored.url)
            .ok()
            .and_then(|url| url.host_str().map(str::to_string))
            .unwrap_or_else(|| stored.url.clone());
        let addon = InstalledAddon {
            url: stored.url,
            id: String::new(),
            name,
            version: String::new(),
            description: String::new(),
            skipped_reason: None,
        };
        return Some((addon, None));
    }
    let manifest = parse_manifest(&stored.manifest_json).ok()?;
    let addon = InstalledAddon {
        url: stored.url,
        id: manifest.id.clone(),
        name: manifest.name.clone(),
        version: manifest.version.clone(),
        description: manifest.description.clone().unwrap_or_default(),
        skipped_reason: None,
    };
    Some((addon, Some(manifest)))
}
