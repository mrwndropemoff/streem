# 03: Workspace and installing an addon

**What to build:** The real app starts here. The user opens the app, sees the list of installed **addons** (Cinemeta is there on first launch), pastes an **addon** URL to install another one, and can remove one. The list survives a restart.

**Blocked by:** 01, 02

**Status:** ready-for-agent

## Read first

- `CONTEXT.md`
- `.scratch/v1/spec.md`, sections "Decisions" and "Workspace".
- `docs/adr/0002-egui-for-the-desktop-ui.md`

## Files

- `Cargo.toml` (root): a workspace with members `crates/core` and `crates/desktop`, and `exclude = ["prototypes/egui-mpv"]`.
- `crates/core/Cargo.toml`: package name **`streem-core`**. Never name a package `core`; that collides with Rust's built-in crate.
- `crates/core/src/lib.rs`: `Core`, `Command`, `State` and re-exports.
- `crates/core/src/app.rs`: the single task that owns the state.
- `crates/core/src/store.rs`: local SQLite storage.
- `crates/core/src/addon/mod.rs`, `crates/core/src/addon/types.rs`, `crates/core/src/addon/client.rs`.
- `crates/desktop/Cargo.toml`: package name **`streem-desktop`**, depends on `streem-core` by path.
- `crates/desktop/src/main.rs`, `crates/desktop/src/screens/mod.rs`, `crates/desktop/src/screens/addons.rs`.

## Crates

- `streem-core`: `tokio`, `reqwest` (JSON support, a rustls TLS feature so Windows needs no OpenSSL), `serde`, `serde_json`, `rusqlite` with the `bundled` feature, `thiserror`. Dev only: `wiremock`, `tempfile`.
- `streem-desktop`: `eframe` with the glow renderer selected, as in the prototype; `dirs`.

## The interface between the two crates

This shape is used by every later ticket. Build exactly this.

```rust
// streem-core
pub struct Core { /* ... */ }

impl Core {
    /// Starts the background runtime. `on_change` is called after every state change.
    pub fn start(data_dir: PathBuf, on_change: impl Fn() + Send + Sync + 'static) -> Core;
    pub fn send(&self, command: Command);
    pub fn state(&self) -> Arc<State>;
}

pub enum Command {
    InstallAddon { url: String },
    RemoveAddon { url: String },
    DismissNotice,
}

pub struct State {
    pub addons: Vec<InstalledAddon>,
    pub notice: Option<Notice>,
}

pub struct InstalledAddon { pub url: String, pub id: String, pub name: String, pub version: String, pub description: String }
pub struct Notice { pub id: u64, pub text: String }
```

- `Core::start` spawns one thread running a tokio runtime. One task owns the mutable state and receives commands over a channel.
- That task never waits on the network or the disk. It spawns the work and receives the result back as an internal message.
- After every change it publishes a new `Arc<State>` and calls `on_change`. `streem-desktop` passes a closure that calls egui's `request_repaint`.
- `Core::send` and `Core::state` never block.
- `streem-desktop` reads `STREEM_DATA_DIR` if set, otherwise uses `dirs::data_dir()` joined with `streem`, and passes it to `Core::start`. This lets two copies of the app run side by side in later tickets.

## Addon protocol

- An **addon** is identified by its manifest URL, ending in `/manifest.json`. Accept a `stremio://` URL by turning the scheme into `https://`.
- Installing means: GET the URL, parse the manifest, reject it with a notice if it is not valid, otherwise save it.
- Manifest fields needed now: `id`, `name`, `version`, `description`, `types`, `resources`, `catalogs`, `idPrefixes`. A `resources` entry is either a string or an object with `name`, `types` and `idPrefixes`; support both.
- **Protocol types:** first try to depend on Stremio's own crate: `cargo add stremio-core --git https://github.com/Stremio/stremio-core` and use its manifest type (look in its `src/types/addon`). Keep it only if `cargo build -p streem-core` succeeds and the test below passes. If not, remove the dependency and write the serde structs yourself in `addon/types.rs`, with a header comment crediting stremio-core (MIT licence). Write under `## Comments` which way you went; later tickets follow it.
- Do not use stremio-core's runtime, models or addon transport. The HTTP client in `addon/client.rs` is ours, built on `reqwest`.

## Storage

- One SQLite file, `streem.db`, in the data directory. Run SQLite calls inside `tokio::task::spawn_blocking`.
- Keep a schema version in `PRAGMA user_version`, with a list of migrations applied in order. Later tickets append to that list.
- Table `addons`: `account_id TEXT`, `url TEXT`, `position INTEGER`, `manifest_json TEXT`, `updated_at INTEGER` (milliseconds since 1970), `deleted INTEGER`, `dirty INTEGER`, primary key `(account_id, url)`. Until sign-in exists, `account_id` is the text `local`. Removing sets `deleted = 1`; it does not delete the row. Every write sets `dirty = 1` and a fresh `updated_at`. These columns are used by sync in ticket 16.
- On first launch (no rows at all), install `https://v3-cinemeta.strem.io/manifest.json`.

## Screen

`screens/addons.rs`: a text field with an "Install" button, and a list showing each **addon**'s name, version and description with a "Remove" button. Show `State.notice` as a dismissible banner at the top of the window.

## Out of scope

Catalogs, posters, playback, settings, sign-in.

## Acceptance checks

- [x] `cargo test -p streem-core` passes, including tests that: install from a `wiremock` server serving a valid manifest and see it in `State.addons`; reject invalid JSON with a notice and no new entry; remove an **addon** and see it gone from `State.addons`; restart `Core` on the same data directory and still see the installed **addon**.
- [x] One test marked `#[ignore]` installs the real Cinemeta manifest; `cargo test -p streem-core -- --ignored` passes with internet access.
- [x] `cargo run -p streem-desktop` opens a window listing Cinemeta.
- [x] `grep -rn "reqwest\|rusqlite\|std::fs" crates/desktop/src` prints nothing.

## Rules

- Use the terms defined in `CONTEXT.md` in names, messages and comments.
- Add crates with `cargo add`. Never type a version number from memory.
- `streem-desktop` does no network or disk work. It sends a `Command` to `Core` and draws `State`.
- Do not install system packages. If something is missing, stop and write what is missing under `## Comments` in this file.
- Do only what this ticket lists. Anything under "Out of scope" belongs to another ticket.
- Finish with `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace`. All three must pass.
- When done, tick the boxes and add a line under `## Comments` saying what you verified and how.

## Comments

**Protocol types: own structs, not `stremio-core`.** The `stremio-core` git dependency was added with `cargo add` but never built or tested: the auto-mode classifier blocked `cargo test` while the git dependency was in `Cargo.toml`, so I asked and was told to skip it. `crates/core/src/addon/types.rs` holds our own serde structs (`Manifest`, `ManifestResource`, `ManifestCatalog`) with a header crediting stremio-core (MIT), copying its leniency: `description` and `idPrefixes` optional, `catalogs` defaulted, `types` and `resources` required, unknown fields ignored. `version` is a plain `String`, not semver. Later tickets follow this: extend our structs, do not depend on `stremio-core`.

**Notes for later tickets**
- reqwest 0.13 has no `rustls-tls` feature; the one used is `rustls` (with `default-features = false`, `json`). It builds on this Windows machine with no extra tools.
- `Core::start_with_defaults(data_dir, on_change, default_addon_urls)` is the same as `Core::start` but names the addons to install on first launch; tests pass an empty list so they stay offline. "First launch" means no `addons` rows at all, removed rows included, so removing Cinemeta does not bring it back.
- The raw manifest text is what is stored in `manifest_json`; a state change is published only after the row is committed. All disk work runs through one worker so results cannot overtake each other.
- Dropping `Core` stops and joins its thread. Before it stops, removals still in the inbox and manifests already downloaded are saved, and every queued disk job finishes; installs still downloading are given up.
- A saved row whose manifest no longer parses is left out of `State.addons` without a notice. If a later ticket tightens `Manifest`, handle old rows then.
- Removing an addon while its install is still downloading cancels that install (the app task keeps a map of downloads in flight).
- Installing a URL that is already installed refreshes its manifest and keeps its place; installing a removed one brings the row back at the end.

**Verified**
- `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`: all pass (10 unit tests, 13 integration tests, 1 ignored).
- `cargo test -p streem-core -- --ignored` passes with internet access (real Cinemeta through a `stremio://` URL).
- `cargo run -p streem-desktop` with an empty `STREEM_DATA_DIR`: took a screenshot of the window on Windows 11; it shows the install field, and Cinemeta 3.0.14 with its description and a Remove button. Not run on Linux. Install and Remove were not clicked in the real window; they are covered by the `Core` tests.
- `grep -rn "reqwest\|rusqlite\|std::fs" crates/desktop/src` prints nothing.
- Notice banner: started the app with `STREEM_DATA_DIR` under an existing file so the store could not open; a screenshot shows the red banner with a Dismiss button above the list. The Dismiss click itself is covered by a `Core` test, not clicked in the window.
