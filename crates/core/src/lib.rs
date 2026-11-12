//! The engine of Streem: **addons**, local storage and (later) sync and playback.
//! It has no UI code. A UI sends [`Command`]s to [`Core`] and draws the [`State`] it publishes.

pub mod addon;
mod app;
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
#[derive(Debug, Clone, PartialEq, Eq)]
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
    /// Opens the details of a title. Asks the first installed **addon** that offers `meta`
    /// for this type and id.
    OpenTitle {
        meta_type: String,
        meta_id: String,
    },
    CloseTitle,
}

/// Everything the UI draws. Replaced as a whole after every change.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State {
    pub addons: Vec<InstalledAddon>,
    pub notice: Option<Notice>,
    /// One row for each catalog the installed **addons** offer, in **addon** order.
    pub home: Vec<CatalogRow>,
    /// The title the person opened, until they close it.
    pub details: Option<Details>,
}

/// A title the person opened, and what its **addon** said about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Details {
    pub meta_type: String,
    pub meta_id: String,
    pub meta: Load<Meta>,
}

/// Something that is being fetched, or has been.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Load<T> {
    Loading,
    Ready(T),
    /// Text for the person using the app.
    Failed(String),
}

/// One catalog of one **addon**, shown on the home screen as a row of posters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogRow {
    pub addon_name: String,
    pub catalog_name: String,
    pub meta_type: String,
    pub items: Load<Vec<MetaPreview>>,
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
