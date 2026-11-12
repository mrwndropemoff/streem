# Streem

A Stremio-like media centre for family and friends. It installs **addons** (remote services that speak the Stremio addon protocol), browses their catalogs and plays their streams. Planned differences from Stremio: several **profiles** under one **account**, everything synced over the internet, and **watch parties**.

> **Status: early development.** Today the desktop app has three screens: Home, which shows one row of posters for each catalog offered by your installed addons; Details, opened by clicking a poster, which shows a title's artwork, description, year, rating and genres (and, for a series, a season picker with its episodes), plus the streams your addons offer for the movie or the clicked episode, grouped by addon (click a playable stream to play it); a Player, which plays the stream inside the window with a control bar over the video; and Addons, where you install an addon from a URL, list installed addons, and remove them. Search, subtitles, sync, profiles and watch parties are planned but not built yet (see [Roadmap](#roadmap)).

Target platforms: Linux (Wayland) and Windows. It is written entirely in Rust.

## Contents

- [Download it](#download-it)
- [What you need before running it](#what-you-need-before-running-it)
- [Run it](#run-it)
- [Run the tests](#run-the-tests)
- [CI and releases](#ci-and-releases)
- [Where your data is stored](#where-your-data-is-stored)
- [How the project is organised](#how-the-project-is-organised)
- [What each file does](#what-each-file-does)
- [Roadmap](#roadmap)
- [Licence](#licence)

## Download it

Ready-made builds are on the [Releases page](https://github.com/mrwndropemoff/streem/releases).

- **Windows:** download `streem-<version>-windows-x86_64.zip`, unzip it anywhere and double-click `streem-desktop.exe`. libmpv is included. The program is not code-signed, so the first time Windows may say *"Windows protected your PC"*: click **More info**, then **Run anyway**.
- **Linux:** install libmpv from your distro first (`libmpv2` on Debian/Ubuntu, `mpv` on Arch), then download `streem-<version>-linux-x86_64.tar.gz`, extract it and run `./streem-desktop`. It is built on Ubuntu 24.04, so it needs a distro at least that recent (glibc 2.39, mpv 0.35 or newer).

## What you need before running it

The engine (`crates/core`) needs only a Rust toolchain. The window (`crates/desktop`) also needs **libmpv**, which plays the video.

| Requirement | Why | How to get it |
|---|---|---|
| **Rust** (stable, with Cargo). The project uses the 2024 edition, so Rust 1.85 or newer. Developed with 1.99. | Builds everything | Install with [rustup](https://rustup.rs). Check with `cargo --version`. |
| **libmpv** | `streem-desktop` links it and plays streams through it | **Linux:** your distro's libmpv development package (e.g. `libmpv-dev` on Debian/Ubuntu, `mpv` on Arch). **Windows:** see [libmpv on Windows](#libmpv-on-windows) below. |
| **Windows only:** MSVC build tools | Rust on Windows links with Microsoft's linker | Install "Visual Studio Build Tools" with the *Desktop development with C++* workload. |
| **Linux only:** a C compiler and a graphical session (Wayland or X11) with OpenGL | The bundled SQLite is compiled from C, and the window draws with OpenGL | e.g. `build-essential` on Debian/Ubuntu, `base-devel` on Arch. Your distro's Mesa/GPU drivers provide OpenGL. |
| **Internet access** | The first launch installs the default addon (Cinemeta), installing any addon downloads its manifest, and the Home screen fetches catalogs and poster images | None needed. The tests run offline, apart from one opt-in test. |

You do **not** need ffmpeg, Supabase or a Real-Debrid account to run the app as it is today. Those only matter for features that are not built yet, or for the optional prototype below.

### libmpv on Windows

1. Download libmpv and make an MSVC import library (`mpv.lib`) with the same script CI uses. It needs the MSVC build tools and [7-Zip](https://www.7-zip.org), and fetches the exact mpv build that CI is pinned to:
   ```powershell
   powershell -ExecutionPolicy Bypass -File .github\scripts\fetch-libmpv.ps1 -Dest C:\libmpv
   ```
   (Doing it by hand: download an `mpv-dev-x86_64` archive from the Windows builds linked on [mpv.io](https://mpv.io) and extract it. Recent archives no longer contain `mpv.def`, so write one from `dumpbin /exports libmpv-2.dll` and run `lib /def:mpv.def /machine:x64 /out:mpv.lib`, as the script does.)
2. Tell the linker where it is, in `%USERPROFILE%\.cargo\config.toml`:
   ```toml
   [target.x86_64-pc-windows-msvc]
   rustflags = ["-L", "native=C:\\libmpv"]
   ```
3. After `cargo build`, copy `libmpv-2.dll` next to the program: `target\debug\` (or `target\release\`). Without it, `streem-desktop.exe` and the `streem-desktop` test program do not start.

## Run it

From the repository root:

```bash
cargo run -p streem-desktop
```

The first build downloads and compiles the dependencies and takes a few minutes. Later builds are fast. For a smoother, optimised build, add `--release`:

```bash
cargo run --release -p streem-desktop
```

A window titled **Streem** opens with an **Addons** screen:

1. On the very first launch, **Cinemeta** (the standard Stremio metadata addon) is installed automatically.
2. Paste an addon URL ending in `/manifest.json` into the field (a `stremio://` link also works) and press **Install** or Enter.
3. Click **Remove** next to an addon to uninstall it.
4. If something fails (bad URL, not a manifest, server unreachable), a red banner explains why. Click **Dismiss** to close it.

Installed addons are saved and are still there next time you start the app.

### Running two copies side by side

Set `STREEM_DATA_DIR` to give a copy its own data folder:

```bash
# bash / Git Bash / Linux
STREEM_DATA_DIR=/tmp/streem-a cargo run -p streem-desktop
```

```powershell
# PowerShell
$env:STREEM_DATA_DIR = "C:\temp\streem-a"; cargo run -p streem-desktop
```

This is also a handy way to get a clean first launch: point it at an empty folder.

## Run the tests

```bash
cargo test --workspace
```

The tests live in `crates/core` and use a fake local web server (`wiremock`) and temporary folders, so they run offline and never touch your real data. They cover installing and removing addons, error notices, restarts (data survives), cancelling an install whose addon is removed mid-download, first-launch defaults, the Home catalog rows (loading, ready, one failing addon, catalogs that need extra input are left out), stream lists (one group per addon, loading on their own; which streams are playable and why not; filenames; a failing addon), playback commands (play, pause, seek and its clamping, ended, failed, throttled position reports, and that the stream link never shows in debug output), and poster caching (one request per poster, none after a restart).

One extra test downloads the real Cinemeta manifest from the internet and is skipped by default:

```bash
cargo test -p streem-core -- --ignored
```

## CI and releases

GitHub Actions runs three workflows, all in `.github/workflows/`:

| Workflow | When | What it does |
|---|---|---|
| `ci.yml` | Every push to `main` and every pull request into `main` | On Linux and Windows: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace`. Any formatting difference, clippy warning or failing test turns it red. |
| `live.yml` | Mondays at 06:00 UTC, or by hand from the Actions tab | Runs the `#[ignore]` tests against the real Cinemeta. A red run means an outside service changed; it never blocks a pull request. |
| `release.yml` | Pushing a tag such as `v0.1.0` | Builds the Windows zip and the Linux tarball and attaches them to a **draft** GitHub Release. |

CI uses the latest stable Rust, so a new Rust release can bring new clippy warnings; fix them like any other warning. On Windows, CI gets libmpv from `.github/scripts/fetch-libmpv.ps1`, which pins one mpv build by checksum. To move to a newer mpv, follow the note at the top of that script.

### Making a release

1. Set the new version in the root `Cargo.toml` (`[workspace.package] version`), run `cargo check` so `Cargo.lock` follows, and merge that to `main`.
2. Tag that commit with the same version and push the tag:
   ```bash
   git tag v0.1.0
   ```
   ```bash
   git push origin v0.1.0
   ```
   The workflow stops if the tag and `Cargo.toml` disagree.
3. When it finishes, open the draft on the [Releases page](https://github.com/mrwndropemoff/streem/releases), check the two downloads and the notes, and press **Publish release**.

The Windows zip contains `streem-desktop.exe`, `libmpv-2.dll`, this project's `LICENSE` and mpv's licence files. The usual Windows mpv builds are GPL-licensed, so the zip as a whole is distributed under the GPL's terms.

## Where your data is stored

Streem keeps a single SQLite file, `streem.db`, in its data folder, plus a `posters/` folder with the original poster images it has downloaded, so it never downloads the same poster twice:

| OS | Default folder |
|---|---|
| Windows | `%APPDATA%\streem` |
| Linux | `~/.local/share/streem` |

Override it with the `STREEM_DATA_DIR` environment variable (see above). Deleting the folder resets the app to a fresh first launch.

## How the project is organised

```
streem/
├── .github/                CI and release workflows, and the Windows libmpv script
├── Cargo.toml              Workspace: lists the two crates, excludes the prototype, sets the version
├── crates/
│   ├── core/               "streem-core": the engine (no UI code)
│   └── desktop/            "streem-desktop": the egui window
├── prototypes/egui-mpv/    Throwaway proof that egui can host libmpv (not part of the build)
├── docs/                   Architecture decisions (ADRs) and notes for AI agents
├── .scratch/v1/            The v1 spec and 23 tickets (the project's issue tracker)
├── CONTEXT.md              Glossary of the project's domain terms
├── AGENTS.md               Pointers for AI coding agents
└── LICENSE                 Apache License 2.0
```

The central design rule: **the UI never does network or disk work.** `desktop` sends *commands* to `core` and draws the *state* that `core` hands back. `core` runs on its own background thread, so scrolling and clicking stay smooth while things load or save.

```
 UI (streem-desktop)                       Engine (streem-core, background thread)
 ───────────────────                       ───────────────────────────────────────
 button click ── Core::send(Command) ───▶  App task: updates State, starts
                                           network / disk work in the background
 draw frame  ◀── Core::state() ──────────  publishes a fresh State snapshot
       ▲                                   │
       └──── on_change() → request repaint ┘
```

## What each file does

### Root

| File | What it does |
|---|---|
| `Cargo.toml` | Declares the Cargo workspace with `crates/core` and `crates/desktop`. The prototype is excluded so it never builds with the main app. `[workspace.package] version` is the one version of the app; release tags must match it. |
| `Cargo.lock` | Exact dependency versions, committed so builds are reproducible. |
| `.gitignore` | Ignores `target/` (build output) and the prototype's generated `sample/` video. |
| `CONTEXT.md` | Glossary of the project's vocabulary (Account, Profile, Override, Debrid Service, Download, Watch Party, Addon) and the words to avoid. Use these terms in code and discussion. |
| `AGENTS.md` | Tells AI coding agents where the issue tracker, triage labels and domain docs live. |
| `LICENSE` | Apache License 2.0. |

### `.github/` — CI and releases

| File | What it does |
|---|---|
| `workflows/ci.yml` | Formatting, clippy and tests on Linux and Windows for every push to `main` and every pull request. |
| `workflows/live.yml` | Weekly run of the `#[ignore]` tests that use the real Cinemeta. |
| `workflows/release.yml` | On a `v*` tag: checks the tag matches `Cargo.toml`, builds the Windows zip and Linux tarball, and creates a draft GitHub Release with them. |
| `scripts/fetch-libmpv.ps1` | Downloads the pinned Windows mpv build, checks its SHA-256, makes `mpv.lib` from the DLL's exports and fetches mpv's licence files. In Actions it also puts the folder on `PATH` and sets `RUSTFLAGS`. Works on a developer machine too. |

### `crates/core` — the engine (`streem-core`)

No UI code. Dependencies include `reqwest` (HTTP), `rusqlite` (bundled SQLite), `image` (poster decoding), `tokio` (async runtime), `serde` (JSON) and `thiserror` (errors).

| File | What it does |
|---|---|
| `src/lib.rs` | The public face of the crate. Defines `Core` (the running engine), `Command` (what the UI can ask for: `InstallAddon`, `RemoveAddon`, `DismissNotice`, `LoadHome`, `OpenTitle`, `CloseTitle`, `LoadStreams`, `Play`, `SetPaused`, `Seek`, `StopPlayback`, `ReportPlayback`), `State` (everything the UI draws: the addon list, the Home catalog rows, the opened title's `Details` with its `StreamList`, the `Playback` of the stream being played, and an optional notice) and `Notice` (a message for the user). `Core::poster` hands the UI a decoded poster once it is ready. `Core::start` launches the background thread and installs Cinemeta on first launch; dropping `Core` stops it cleanly, after saving pending work. |
| `src/app.rs` | The single background task that owns the mutable state. It receives commands and the results of its own network/disk jobs as messages, updates `State`, publishes it, and calls the UI's `on_change` callback. It never blocks on network or disk, and handles edge cases such as cancelling an install when the addon is removed mid-download. |
| `src/playback.rs` | What is playing, as plain functions over `State`, with no player involved: `Play` turns a playable stream of the open title into a `Playback` (its link stays here, never in the stream list), `SetPaused` and `Seek` change what the player should do (a seek gets a new id and is kept within the video's length), and `ReportPlayback` copies back what the player says; an ended report clears the playback and a failed one also raises the notice "This stream could not be played. Pick another one." Nothing here logs, because a stream link can later hold a Real-Debrid key. |
| `src/posters.rs` | Poster images. Downloads each image once, saves the original bytes under `<data folder>/posters/`, decodes and scales it to at most 300 pixels wide on a blocking task, and keeps the result in memory. A saved poster is never downloaded again, in this run or a later one. |
| `src/store.rs` | Local SQLite storage in `streem.db`. Opens the file, runs numbered migrations (append new ones, never edit old ones), and saves, loads and removes addon rows. Columns such as `deleted`, `dirty` and `updated_at` are already there for the future sync feature. |
| `src/addon/mod.rs` | Declares the `addon` module's two submodules. |
| `src/addon/types.rs` | Rust types for the Stremio addon manifest (name, version, resources, catalogs and their `extra` options, catalog entries). Only the fields the app uses are read; unknown fields are ignored. A title's description (`Meta`) and episodes (`Video`) are read leniently: an odd optional field, such as a `season` sent as text, never drops an episode, and only an entry with no `id` is skipped. A `Stream` is read the same way, and its filename comes from `behaviorHints.filename` or the last part of its URL. Shapes follow `stremio-core` (MIT). |
| `src/addon/client.rs` | Talks to addons over HTTP: turns `stremio://` links into `https://`, rejects URLs that are not `manifest.json` links, downloads the manifest and parses it, fetches the first page of a catalog (`/catalog/<type>/<id>.json`), fetches a title's details (`/meta/<type>/<id>.json`), and fetches the streams of one video (`/stream/<type>/<video id>.json`). |
| `tests/core.rs` | Black-box tests of `Core`: send commands, check the resulting `State`. Uses a mock web server and temporary folders. Covers the Home rows, posters and title details too (movie, series, which **addon** is asked, no matching **addon**, closing a title) and stream lists. |

### `crates/desktop` — the window (`streem-desktop`)

Built with [egui](https://github.com/emilk/egui) through `eframe`, using the OpenGL (`glow`) renderer, because libmpv (the video player) can only draw through OpenGL.

| File | What it does |
|---|---|
| `src/main.rs` | Program entry point. Opens the "Streem" window, starts `Core` (pointing it at the data folder, or `STREEM_DATA_DIR`) and the video, and each frame draws either the Player (while `State.playback` is set) or the notice banner, a Home/Addons top bar and the chosen screen (Home, Addons or Details) from the latest `State`. Opening a title from Home sends `OpenTitle` and shows Details; Back, or clicking a top-bar tab, sends `CloseTitle`. When playback ends it unloads the file and leaves fullscreen. |
| `src/screens/mod.rs` | Groups the screens and holds shared UI pieces, such as the dismissible red notice banner at the top of the window and `Textures`, which turns each poster or background image from `Core` into one egui texture and keeps it for later frames. |
| `src/screens/home.rs` | The Home screen: a vertical list of catalog rows, each a title and a horizontal strip of posters with names. It asks `Core` for a poster only when its cell is on screen, draws a grey placeholder until it arrives, and returns the title the person clicked so `main.rs` can open it. Its scroll position is kept, so Back lands where you left. Loading rows show placeholders and failed rows show their message. |
| `src/screens/details.rs` | The Details screen: a Back button, then the dimmed background image, poster, name, year, rating, genres and description of the opened title. For a series it adds a season dropdown (seasons ascending, season 0 last as "Specials") and that season's episodes as "E3 · Title". Below a movie, or beside the episodes once one is clicked, it lists the streams under each addon's name; streams the app cannot play (such as torrents) are greyed out with the reason, and clicking a playable one sends `Play`. Shows a placeholder while loading and the message if the request failed. |
| `src/mpv_view.rs` | The one place that talks to libmpv. It owns the mpv instance and draws its video into egui's OpenGL context. Every frame it compares mpv with `State.playback` and corrects mpv (load a different link, pause or resume, do a seek it has not done yet), then sends `ReportPlayback` about once a second while playing, at once when buffering starts or stops, and when the file ends or cannot be loaded. |
| `src/screens/player.rs` | The Player screen, shown in place of everything else while `State.playback` is set: the video fills the window, with a control bar over it (Back, Play/Pause, seek slider, time, title) that hides after 3 seconds without mouse movement. The controls only send `SetPaused`, `Seek` and `StopPlayback`. Keys: Space pauses, Left and Right seek 10 seconds, Escape stops, F toggles fullscreen. |
| `src/screens/addons.rs` | The Addons screen: the URL field and **Install** button, the scrolling list of installed addons (name, version, description), and a **Remove** button on each. It only sends `Command`s; it never does I/O itself. |

### `prototypes/egui-mpv` — throwaway experiment

A small, separate program that proved libmpv can render video inside an egui window (with controls drawn on top, audio and subtitle track switching, seeking and fullscreen) on Linux and Windows. It is **not** part of the workspace and its code is meant to be redone properly when the real player ships. It is only needed if you want to repeat the experiment.

| File | What it does |
|---|---|
| `Cargo.toml` | Dependencies: `eframe` (OpenGL only) and `libmpv2`. Has an empty `[workspace]` table to keep it out of the main workspace. |
| `src/main.rs` | The prototype app: takes a video path as its first argument, lets mpv draw into egui's OpenGL context, and shows play/pause, seek, track and fullscreen controls (Space and F also work). |
| `make-sample.sh` | Uses `ffmpeg` to generate `sample/sample.mkv`: a 60-second test video with two audio tracks (English 440 Hz, French 880 Hz) and subtitles. |
| `Cargo.lock` | Locked dependencies for the prototype. |

To try it you additionally need **libmpv** and, for the sample video, **ffmpeg**:

```bash
cd prototypes/egui-mpv
./make-sample.sh                         # needs ffmpeg
cargo run --release -- sample/sample.mkv
```

- **Linux:** install your distro's libmpv development package (e.g. `libmpv-dev` on Debian/Ubuntu, `mpv` on Arch).
- **Windows:** this is fiddly. Download the `mpv-dev-x86_64` archive from the Windows builds linked on [mpv.io](https://mpv.io), create an MSVC import library with `lib /def:mpv.def /machine:x64 /out:mpv.lib` from an "x64 Native Tools" prompt, point the linker at it (for example `RUSTFLAGS=-L native=<folder with mpv.lib>`), and copy `libmpv-2.dll` next to the built `.exe` in `target\release\`. Exact steps are in `.scratch/v1/issues/02-prototype-windows-and-egui-decision.md`.

### `docs/` — decisions and agent notes

| File | What it does |
|---|---|
| `docs/adr/0001-hosted-supabase-with-local-copy.md` | Decision: use Supabase's hosted free tier for login, sync and watch-party signals, with a local copy of the data on every device. Lists the alternatives rejected and the consequences. |
| `docs/adr/0002-egui-for-the-desktop-ui.md` | Decision: build the UI with egui (OpenGL renderer) because libmpv was proven to render inside it. Lists alternatives (Slint, Tauri, Flutter) and consequences. |
| `docs/agents/issue-tracker.md` | How issues live as markdown files under `.scratch/<feature>/`. |
| `docs/agents/triage-labels.md` | The status labels used on tickets (`needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`). |
| `docs/agents/domain.md` | How the single `CONTEXT.md` + `docs/adr/` setup should be used. |

### `.scratch/v1/` — spec and tickets

| File | What it does |
|---|---|
| `spec.md` | The v1 specification: what the app is, key decisions, the two-crate rule, accounts/profiles/overrides, Real-Debrid via addons, sync, watch parties, build order and open risks. **Read this first** to understand where the project is going. |
| `issues/01` … `issues/23` | One ticket per piece of work, each with a `Status:` line (e.g. `ready-for-agent`, `ready-for-human`). Tickets 01 to 07 have been worked on (their `Status:` lines may lag behind the code); the rest are the backlog. |

## Roadmap

From the spec, in build order (v1 is steps 1 to 5):

1. **Prototype:** egui plays video through libmpv. *Done: egui was chosen.*
2. **Browse and play:** workspace and addon install *(done)*, catalogs on the Home screen *(done)*, title details *(done)*, stream list *(done)*, playing a stream *(done)*, then search, subtitles. *In progress.*
3. **Real-Debrid:** a `{real_debrid_key}` placeholder in addon URLs (Real-Debrid itself is handled by addons such as Torrentio; the app has no torrent engine).
4. **Account and profiles:** sign-up/sign-in, profile picker, per-profile overrides, history, continue-watching and library, all synced through Supabase.
5. **Watch party:** shared play/pause/seek between profiles of one account.

## Licence

Licensed under the [Apache License 2.0](LICENSE).
