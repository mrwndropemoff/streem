//! The engine of Streem: **addons**, local storage and (later) sync and playback.
//! It has no UI code. A UI sends [`Command`]s to [`Core`] and draws the [`State`] it publishes.

pub mod addon;
mod app;
mod playback;
mod posters;
mod store;

use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::thread::JoinHandle;

use tokio::sync::{mpsc, oneshot};

pub use addon::types::{Meta, MetaPreview, Video};
pub use posters::Poster;

/// Installed on first launch.
const CINEMETA_URL: &str = "https://v3-cinemeta.strem.io/manifest.json";

/// A request from the UI.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    InstallAddon {
        url: String,
    },
    RemoveAddon {
        url: String,
    },
    DismissNotice,
    /// Fetches the catalogs of the installed **addons** again. `Core` also does this by itself
    /// at startup and whenever the installed **addons** change.
    LoadHome,
    /// Searches every catalog of the installed **addons** that supports search, all at once.
    /// Replaces the results of any earlier search. An empty or whitespace-only query is the
    /// same as [`Command::ClearSearch`].
    Search {
        query: String,
    },
    ClearSearch,
    /// Opens the details of a title. Asks the first installed **addon** that offers `meta`
    /// for this type and id.
    OpenTitle {
        meta_type: String,
        meta_id: String,
    },
    CloseTitle,
    /// Lists the streams of one video of the open title: the meta id for a movie, the
    /// episode's own id for a series. Asks every installed **addon** that offers `stream` for
    /// the title's type and this id, all at once. Ignored when no title is open.
    LoadStreams {
        video_id: String,
    },
    /// Plays one stream of the open title's current stream list: `stream_index` counts within
    /// the group of the **addon** with id `addon_id`. Ignored when there is no such stream or it
    /// is not playable.
    Play {
        addon_id: String,
        stream_index: usize,
    },
    /// Pauses or resumes. Ignored when nothing is playing.
    SetPaused(bool),
    /// Jumps to a position, kept within the video's length. Ignored when nothing is playing.
    Seek {
        position_secs: f64,
    },
    /// Ends playback, which takes the person back to the stream list.
    StopPlayback,
    /// What the player says about itself. Only the player sends this.
    ReportPlayback(PlaybackReport),
}

/// Everything the UI draws. Replaced as a whole after every change.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct State {
    pub addons: Vec<InstalledAddon>,
    pub notice: Option<Notice>,
    /// One row for each catalog the installed **addons** offer, in **addon** order.
    pub home: Vec<CatalogRow>,
    /// The latest search, until it is cleared.
    pub search: Option<Search>,
    /// The title the person opened, until they close it.
    pub details: Option<Details>,
    /// The stream being played, until it ends or the person stops it.
    pub playback: Option<Playback>,
}

/// A title the person opened, and what its **addon** said about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Details {
    pub meta_type: String,
    pub meta_id: String,
    pub meta: Load<Meta>,
    /// The streams of the video last asked for with [`Command::LoadStreams`].
    pub streams: Option<StreamList>,
}

/// The streams of one video, grouped by the **addon** that supplied them, in **addon** order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamList {
    pub video_id: String,
    /// A group that loaded with no streams is dropped.
    pub groups: Vec<StreamGroup>,
}

/// The streams one **addon** offers for a video.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamGroup {
    /// The id in the **addon**'s manifest.
    pub addon_id: String,
    pub addon_name: String,
    pub streams: Load<Vec<StreamItem>>,
}

/// One stream as the UI shows it. Its link stays inside `streem-core`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamItem {
    pub name: String,
    pub description: String,
    pub filename: Option<String>,
    /// True only for an `http://` or `https://` link.
    pub playable: bool,
    /// Text for the person using the app, set when `playable` is false.
    pub unplayable_reason: Option<String>,
}

/// A stream being played: what it is, what the player should be doing, and what the player
/// last said it is doing. The player makes itself match the middle part.
#[derive(Clone, PartialEq)]
pub struct Playback {
    // What is playing.
    pub meta_type: String,
    pub meta_id: String,
    pub video_id: String,
    /// "Name", or for an episode "Name · S1E2 · Episode title".
    pub title: String,
    pub poster: Option<String>,
    /// The id in the **addon**'s manifest.
    pub addon_id: String,
    pub stream_name: String,
    pub filename: Option<String>,
    /// The link to play. Only the player reads it. Never log it: later it can hold a
    /// **debrid service** key.
    pub url: String,

    // What the player should be doing.
    pub paused: bool,
    /// The latest jump asked for. The player does it once for each new `id`.
    pub seek: Option<SeekRequest>,

    // What the player last said.
    pub position_secs: f64,
    /// Zero until the player knows it.
    pub duration_secs: f64,
    pub buffering: bool,
}

// By hand, so that no `{:?}` of the state, in a log or a failed test, can print the link.
impl std::fmt::Debug for Playback {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Playback")
            .field("title", &self.title)
            .field("video_id", &self.video_id)
            .field("addon_id", &self.addon_id)
            .field("stream_name", &self.stream_name)
            .field("paused", &self.paused)
            .field("seek", &self.seek)
            .field("position_secs", &self.position_secs)
            .field("duration_secs", &self.duration_secs)
            .field("buffering", &self.buffering)
            .finish_non_exhaustive()
    }
}

/// A jump to a position. `id` grows with every request, so the player can tell a new one from
/// one it has already done.
#[derive(Debug, Clone, PartialEq)]
pub struct SeekRequest {
    pub id: u64,
    pub position_secs: f64,
}

/// What the player tells `Core` about itself, with [`Command::ReportPlayback`].
#[derive(Debug, Clone, PartialEq)]
pub struct PlaybackReport {
    pub position_secs: f64,
    pub duration_secs: f64,
    pub buffering: bool,
    /// The stream played to its end.
    pub ended: bool,
    /// The stream could not be played. The text is for the log, not for the person.
    pub failed: Option<String>,
}

/// Something that is being fetched, or has been.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Load<T> {
    Loading,
    Ready(T),
    /// Text for the person using the app.
    Failed(String),
}

/// One catalog of one **addon**, shown on the home or search screen as a row of posters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogRow {
    pub addon_name: String,
    pub catalog_name: String,
    pub meta_type: String,
    pub items: Load<Vec<MetaPreview>>,
}

/// The results of a search: one row for each catalog that supports search, in **addon** order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Search {
    /// What was searched for, without surrounding whitespace.
    pub query: String,
    pub rows: Vec<CatalogRow>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledAddon {
    pub url: String,
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
}

/// A message for the person using the app, such as why an install failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub id: u64,
    pub text: String,
}

/// The running engine. Dropping it stops the background thread.
pub struct Core {
    commands: mpsc::UnboundedSender<app::Message>,
    state: Arc<RwLock<Arc<State>>>,
    posters: Arc<posters::PosterCache>,
    shutdown: Option<oneshot::Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl Core {
    /// Starts the background runtime. `on_change` is called after every state change.
    pub fn start(data_dir: PathBuf, on_change: impl Fn() + Send + Sync + 'static) -> Core {
        Core::start_with_defaults(data_dir, on_change, vec![CINEMETA_URL.to_string()])
    }

    /// Like [`Core::start`], but names the **addons** to install on first launch
    /// (when the data directory has no **addon** rows at all). Tests use it to stay offline.
    pub fn start_with_defaults(
        data_dir: PathBuf,
        on_change: impl Fn() + Send + Sync + 'static,
        default_addon_urls: Vec<String>,
    ) -> Core {
        let state = Arc::new(RwLock::new(Arc::new(State::default())));
        let (commands, inbox) = mpsc::unbounded_channel();
        let (shutdown, shutdown_signal) = oneshot::channel();
        let posters = Arc::new(posters::PosterCache::default());

        let app = app::App {
            data_dir,
            on_change: Arc::new(on_change),
            default_addon_urls,
            state: state.clone(),
            posters: posters.clone(),
            sender: commands.clone(),
            inbox,
            shutdown: shutdown_signal,
        };
        let thread = std::thread::Builder::new()
            .name("streem-core".into())
            .spawn(move || {
                tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(2)
                    .enable_all()
                    .build()
                    .expect("the background runtime can start")
                    .block_on(app.run());
            })
            .expect("the background thread can start");

        Core {
            commands,
            state,
            posters,
            shutdown: Some(shutdown),
            thread: Some(thread),
        }
    }

    /// Queues a command. Never blocks.
    pub fn send(&self, command: Command) {
        // Fails only when the background thread has already stopped.
        let _ = self.commands.send(app::Message::Command(command));
    }

    /// Returns the decoded poster if it is ready. On the first call for a URL it starts
    /// loading in the background and returns `None`; `on_change` fires when it is ready.
    /// Never waits on the network or the disk.
    pub fn poster(&self, url: &str) -> Option<Arc<Poster>> {
        let (poster, start_loading) = self.posters.lookup(url);
        if start_loading {
            let _ = self
                .commands
                .send(app::Message::LoadPoster(url.to_string()));
        }
        poster
    }

    /// The latest published state. Never waits on the network or the disk.
    pub fn state(&self) -> Arc<State> {
        self.state
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }
}

impl Drop for Core {
    fn drop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
