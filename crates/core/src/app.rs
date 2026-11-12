//! The single task that owns the mutable [`State`].
//!
//! It never waits on the network or the disk. It spawns that work and gets the result back
//! as a [`Message`]. All disk work goes through one worker, so results arrive in the order the
//! writes were made.

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use tokio::sync::{mpsc, oneshot};

use crate::addon::client::{
    AddonError, fetch_manifest, http_client, normalize_url, parse_manifest,
};
use crate::store::{Store, StoreError, StoredAddon};
use crate::{Command, InstalledAddon, Notice, State};

/// What the task receives: the UI's commands, and results of work it started.
pub(crate) enum Message {
    Command(Command),
    /// A manifest was downloaded and is valid: the **addon**'s URL and the manifest text.
    Fetched(Result<(String, String), String>),
    /// A disk job finished. The error is text for a [`Notice`].
    Stored(Result<StoreOutput, String>),
}

/// The **addon** list as saved, after a disk job.
pub(crate) struct StoreOutput {
    addons: Vec<InstalledAddon>,
    /// Set by the first load only: the data directory had no **addon** rows at all.
    first_launch: bool,
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
    pub(crate) sender: mpsc::UnboundedSender<Message>,
    pub(crate) inbox: mpsc::UnboundedReceiver<Message>,
    pub(crate) shutdown: oneshot::Receiver<()>,
}

impl App {
    pub(crate) async fn run(mut self) {
        let (jobs, job_inbox) = mpsc::unbounded_channel();
        tokio::spawn(store_worker(
            self.data_dir.clone(),
            job_inbox,
            self.sender.clone(),
        ));
        let _ = jobs.send(StoreJob::Load);

        let client = http_client();
        let mut state = State::default();
        let mut published = Arc::new(state.clone());
        let mut next_notice_id = 1;

        loop {
            let message = tokio::select! {
                message = self.inbox.recv() => message,
                _ = &mut self.shutdown => None,
            };
            let Some(message) = message else { break };

            let mut to_install = Vec::new();
            match message {
                Message::Command(Command::InstallAddon { url }) => to_install.push(url),
                Message::Command(Command::RemoveAddon { url }) => {
                    let _ = jobs.send(StoreJob::Remove { url });
                }
                Message::Command(Command::DismissNotice) => state.notice = None,
                Message::Fetched(Ok((url, manifest_json))) => {
                    let _ = jobs.send(StoreJob::Upsert { url, manifest_json });
                }
                Message::Fetched(Err(text)) => {
                    state.notice = Some(notice(&mut next_notice_id, text));
                }
                Message::Stored(Ok(output)) => {
                    state.addons = output.addons;
                    if output.first_launch {
                        to_install.extend(self.default_addon_urls.iter().cloned());
                    }
                }
                Message::Stored(Err(text)) => {
                    state.notice = Some(notice(&mut next_notice_id, text));
                }
            }

            for url in to_install {
                spawn_install(&client, self.sender.clone(), url);
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
    }
}

fn notice(next_id: &mut u64, text: String) -> Notice {
    let id = *next_id;
    *next_id += 1;
    Notice { id, text }
}

/// Downloads and checks the manifest in the background, then reports back.
fn spawn_install(client: &reqwest::Client, sender: mpsc::UnboundedSender<Message>, input: String) {
    let client = client.clone();
    tokio::spawn(async move {
        let result = async {
            let url = normalize_url(&input)?;
            let (_manifest, text) = fetch_manifest(&client, &url).await?;
            Ok::<_, AddonError>((url, text))
        }
        .await
        .map_err(|error| format!("Could not install {}: {error}", input.trim()));
        let _ = sender.send(Message::Fetched(result));
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
    let addons = store
        .list_addons()?
        .into_iter()
        .filter_map(installed_addon)
        .collect();
    Ok(StoreOutput {
        addons,
        first_launch,
    })
}

/// A saved row whose manifest can no longer be read is left out of the list.
fn installed_addon(stored: StoredAddon) -> Option<InstalledAddon> {
    let manifest = parse_manifest(&stored.manifest_json).ok()?;
    Some(InstalledAddon {
        url: stored.url,
        id: manifest.id,
        name: manifest.name,
        version: manifest.version,
        description: manifest.description.unwrap_or_default(),
    })
}
