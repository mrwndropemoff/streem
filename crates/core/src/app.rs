//! The single task that owns the mutable [`State`].
//!
//! It never waits on the network or the disk. It spawns that work and gets the result back
//! as a [`Message`]. All disk work goes through one worker, so results arrive in the order the
//! writes were made.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use tokio::sync::{mpsc, oneshot};

use crate::addon::client::{
    catalog_url, fetch_catalog, fetch_manifest, fetch_meta, fetch_streams, http_client, meta_url,
    normalize_url, parse_manifest, stream_url,
};
use crate::addon::types::{Manifest, ManifestCatalog, Meta, MetaPreview, Stream, StreamSource};
use crate::posters::{self, Poster, PosterCache};
use crate::store::{Store, StoreError, StoredAddon};
use crate::{
    CatalogRow, Command, Details, InstalledAddon, Load, Notice, State, StreamGroup, StreamItem,
    StreamList,
};

/// What the task receives: the UI's commands, and results of work it started.
pub(crate) enum Message {
    Command(Command),
    /// The install started with this id finished downloading: the valid manifest text, or text
    /// for a [`Notice`].
    Fetched(u64, Result<String, String>),
    /// A disk job finished. The error is text for a [`Notice`].
    Stored(Result<StoreOutput, String>),
    /// The UI asked for a poster it has not asked for before.
    LoadPoster(String),
    /// A poster finished loading, or could not be.
    PosterLoaded(String, Result<Poster, String>),
    /// A home row finished loading. Ignored when `generation` is not the latest one, which means
    /// the rows were rebuilt since the request was made.
    Catalog {
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
}

/// The **addon** list as saved, after a disk job.
pub(crate) struct StoreOutput {
    addons: Vec<InstalledAddon>,
    /// The catalogs the home screen can show, from the saved manifests.
    catalogs: Vec<CatalogSource>,
    /// The saved manifests, by **addon** URL, in **addon** order. Used to pick who answers a
    /// meta request.
    manifests: Vec<(String, Manifest)>,
    /// Set by the first load only: the data directory had no **addon** rows at all.
    first_launch: bool,
}

/// A catalog that gets a row on the home screen, with the **addon** that offers it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct CatalogSource {
    addon_url: String,
    addon_name: String,
    catalog: ManifestCatalog,
}

enum StoreJob {
    Load,
    Upsert { url: String, manifest_json: String },
    Remove { url: String },
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
        // The manifests of the installed **addons**, and which open title the running fetch
        // belongs to.
        let mut manifests: Vec<(String, Manifest)> = Vec::new();
        let mut details_generation = 0;
        // The **addon** URL of each stream group, in the same order as the groups, and which
        // stream request the running fetches belong to.
        let mut stream_sources: Vec<String> = Vec::new();
        let mut streams_generation = 0;

        loop {
            let message = tokio::select! {
                message = self.inbox.recv() => message,
                _ = &mut self.shutdown => None,
            };
            let Some(message) = message else { break };

            let mut to_install = Vec::new();
            let mut reload_home = false;
            match message {
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
                    generation,
                    row,
                    result,
                } => {
                    if generation == home_generation
                        && let Some(row) = state.home.get_mut(row)
                    {
                        row.items = match result {
                            Ok(items) => Load::Ready(items),
                            Err(text) => Load::Failed(text),
                        };
                    }
                }
                Message::Command(Command::OpenTitle { meta_type, meta_id }) => {
                    details_generation += 1;
                    streams_generation += 1;
                    stream_sources.clear();
                    let meta = start_meta(
                        &client,
                        &self.sender,
                        &manifests,
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
                    stream_sources.clear();
                    state.details = None;
                }
                Message::Command(Command::LoadStreams { video_id }) => {
                    if let Some(details) = state.details.as_mut() {
                        streams_generation += 1;
                        let (sources, list) = start_streams(
                            &client,
                            &self.sender,
                            &manifests,
                            &details.meta_type,
                            video_id,
                            streams_generation,
                        );
                        stream_sources = sources;
                        details.streams = Some(list);
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
                        && let Some(index) = stream_sources.iter().position(|url| *url == addon_url)
                    {
                        match result {
                            Ok(streams) if streams.is_empty() => {
                                list.groups.remove(index);
                                stream_sources.remove(index);
                            }
                            Ok(streams) => {
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
                    state.addons = output.addons;
                    // Every finished disk job is a startup load, an install or a removal.
                    catalogs = output.catalogs;
                    manifests = output.manifests;
                    reload_home = true;
                    if output.first_launch {
                        to_install.extend(self.default_addon_urls.iter().cloned());
                    }
                }
                Message::Stored(Err(text)) => {
                    state.notice = Some(notice(&mut next_notice_id, text));
                }
            }

            if reload_home {
                home_generation += 1;
                state.home = start_home(&client, &self.sender, &catalogs, home_generation);
            }

            for input in to_install {
                match normalize_url(&input) {
                    Ok(url) => {
                        let id = next_download_id;
                        next_download_id += 1;
                        downloading.insert(id, url.clone());
                        spawn_download(&client, self.sender.clone(), id, url, input);
                    }
                    Err(error) => {
                        let text = format!("Could not install {}: {error}", input.trim());
                        state.notice = Some(notice(&mut next_notice_id, text));
                    }
                }
            }
            if *published != state {
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
                _ => {}
            }
        }
        drop(jobs);
        let _ = worker.await;
    }
}

fn notice(next_id: &mut u64, text: String) -> Notice {
    let id = *next_id;
    *next_id += 1;
    Notice { id, text }
}

/// Downloads and checks the manifest in the background, then reports back.
fn spawn_download(
    client: &reqwest::Client,
    sender: mpsc::UnboundedSender<Message>,
    id: u64,
    url: String,
    input: String,
) {
    let client = client.clone();
    tokio::spawn(async move {
        let result = fetch_manifest(&client, &url)
            .await
            .map(|(_manifest, text)| text)
            .map_err(|error| format!("Could not install {}: {error}", input.trim()));
        let _ = sender.send(Message::Fetched(id, result));
    });
}

/// Builds a `Loading` row for each catalog and fetches them all at once. Each row is answered
/// on its own, so one failing **addon** never touches the others.
fn start_home(
    client: &reqwest::Client,
    sender: &mpsc::UnboundedSender<Message>,
    catalogs: &[CatalogSource],
    generation: u64,
) -> Vec<CatalogRow> {
    let mut rows = Vec::with_capacity(catalogs.len());
    for (row, source) in catalogs.iter().enumerate() {
        rows.push(CatalogRow {
            addon_name: source.addon_name.clone(),
            catalog_name: source.catalog.display_name().to_string(),
            meta_type: source.catalog.kind.clone(),
            items: Load::Loading,
        });

        let client = client.clone();
        let sender = sender.clone();
        let addon_url = source.addon_url.clone();
        let catalog = source.catalog.clone();
        tokio::spawn(async move {
            let result = match catalog_url(&addon_url, &catalog.kind, &catalog.id) {
                Ok(url) => fetch_catalog(&client, &url).await,
                Err(error) => Err(error),
            };
            let result = result.map_err(|error| error.to_string());
            let _ = sender.send(Message::Catalog {
                generation,
                row,
                result,
            });
        });
    }
    rows
}

/// Picks the **addon** to ask about a title: the first installed one that offers `meta` for
/// the type and id. Fetches in the background and returns `Loading`, or `Failed` when no
/// **addon** matches.
fn start_meta(
    client: &reqwest::Client,
    sender: &mpsc::UnboundedSender<Message>,
    manifests: &[(String, Manifest)],
    meta_type: &str,
    meta_id: &str,
    generation: u64,
) -> Load<Meta> {
    let Some((addon_url, _)) = manifests
        .iter()
        .find(|(_, manifest)| manifest.offers("meta", meta_type, meta_id))
    else {
        return Load::Failed(format!(
            "No installed addon can describe {meta_type} {meta_id}."
        ));
    };

    let client = client.clone();
    let sender = sender.clone();
    let addon_url = addon_url.clone();
    let meta_type = meta_type.to_string();
    let meta_id = meta_id.to_string();
    tokio::spawn(async move {
        let result = match meta_url(&addon_url, &meta_type, &meta_id) {
            Ok(url) => fetch_meta(&client, &url, &meta_type, &meta_id).await,
            Err(error) => Err(error),
        };
        let result = result.map_err(|error| error.to_string());
        let _ = sender.send(Message::Meta { generation, result });
    });
    Load::Loading
}

/// Builds a `Loading` group for every installed **addon** that offers `stream` for the type and
/// video id, and fetches them all at once. Each group is answered on its own, so one failing
/// **addon** never touches the others. Returns the **addon** URL of each group with the list.
fn start_streams(
    client: &reqwest::Client,
    sender: &mpsc::UnboundedSender<Message>,
    manifests: &[(String, Manifest)],
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
            let result = match stream_url(&addon_url, &meta_type, &video_id) {
                Ok(url) => fetch_streams(&client, &url).await,
                Err(error) => Err(error),
            };
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

/// What the UI is told about a stream. Only `http://` and `https://` links can be played.
fn stream_item(stream: Stream) -> StreamItem {
    let unplayable_reason = match &stream.source {
        StreamSource::Url(url) if is_http(url) => None,
        StreamSource::Torrent { .. } => Some("Torrent streams are not supported"),
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

fn is_http(url: &str) -> bool {
    url.starts_with("http://") || url.starts_with("https://")
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
    match job {
        StoreJob::Load => first_launch = store.has_no_addon_rows()?,
        StoreJob::Upsert { url, manifest_json } => store.upsert_addon(&url, &manifest_json)?,
        StoreJob::Remove { url } => store.remove_addon(&url)?,
    }
    let mut addons = Vec::new();
    let mut catalogs = Vec::new();
    let mut manifests = Vec::new();
    for stored in store.list_addons()? {
        let Some((addon, manifest)) = installed_addon(stored) else {
            continue;
        };
        for catalog in &manifest.catalogs {
            if !catalog.has_required_extra() {
                catalogs.push(CatalogSource {
                    addon_url: addon.url.clone(),
                    addon_name: addon.name.clone(),
                    catalog: catalog.clone(),
                });
            }
        }
        manifests.push((addon.url.clone(), manifest));
        addons.push(addon);
    }
    Ok(StoreOutput {
        addons,
        catalogs,
        manifests,
        first_launch,
    })
}

/// A saved row whose manifest can no longer be read is left out of the list.
/// Returns the **addon** with its manifest.
fn installed_addon(stored: StoredAddon) -> Option<(InstalledAddon, Manifest)> {
    let manifest = parse_manifest(&stored.manifest_json).ok()?;
    let addon = InstalledAddon {
        url: stored.url,
        id: manifest.id.clone(),
        name: manifest.name.clone(),
        version: manifest.version.clone(),
        description: manifest.description.clone().unwrap_or_default(),
    };
    Some((addon, manifest))
}
