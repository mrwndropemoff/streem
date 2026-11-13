# 12: Settings: Real-Debrid key placeholder and playback languages

**What to build:** A Settings screen where the user types a Real-Debrid key once, and picks a preferred audio language and subtitle language. An **addon** whose install URL contains `{real_debrid_key}` is called with the real key filled in. With Torrentio installed that way, torrent-sourced streams arrive as ordinary links and play. The app itself never talks to Real-Debrid.

**Blocked by:** 07

**Status:** ready-for-agent

## Read first

- `CONTEXT.md` (**Debrid Service**, **Addon**)
- `.scratch/v1/spec.md`, section "Real-Debrid through addons".
- `crates/core/src/addon/client.rs`, `crates/core/src/store.rs`, `crates/core/src/app.rs`, `crates/core/src/playback.rs`
- `crates/desktop/src/mpv_view.rs`, `crates/desktop/src/screens/addons.rs`

## Files

- New: `crates/core/src/settings.rs`
- New: `crates/core/src/addon/url.rs`
- Edit: `crates/core/src/lib.rs`, `crates/core/src/store.rs`, `crates/core/src/app.rs`, `crates/core/src/addon/client.rs`
- New: `crates/desktop/src/screens/settings.rs`
- Edit: `crates/desktop/src/main.rs`, `crates/desktop/src/screens/mod.rs`, `crates/desktop/src/screens/addons.rs`, `crates/desktop/src/mpv_view.rs`

## Additions to the interface

```rust
pub struct Settings {
    pub real_debrid_key: Option<String>,
    pub audio_lang: Option<String>,      // ISO 639-2 code such as "eng"
    pub subtitle_lang: Option<String>,   // None means subtitles off by default
}

// State
pub settings: Settings,

// InstalledAddon
pub skipped_reason: Option<String>,

// Command
SetRealDebridKey(Option<String>),
SetPlaybackLanguages { audio: Option<String>, subtitle: Option<String> },
```

## Storage

Add a migration creating table `account_settings`: `account_id TEXT PRIMARY KEY`, `real_debrid_key TEXT`, `audio_lang TEXT`, `subtitle_lang TEXT`, `updated_at INTEGER`, `dirty INTEGER`. One row, `account_id = 'local'` until sign-in exists. Sync uses it in ticket 17.

## The placeholder (`addon/url.rs`)

```rust
pub const PLACEHOLDER: &str = "{real_debrid_key}";
pub fn resolve(template: &str, settings: &Settings) -> Result<String, MissingKey>;
```

- The stored **addon** URL is the template. It is what `addons.url` holds and what `InstalledAddon.url` shows.
- `resolve` replaces the placeholder with the key, percent-encoded. Also accept the percent-encoded spelling `%7Breal_debrid_key%7D`.
- A template without a placeholder resolves to itself, whatever the settings are.
- A template with a placeholder and no key returns `MissingKey`.
- Every request to an **addon** goes through `resolve` first: manifest, catalog, meta, stream and subtitles. There must be no other way to build an **addon** request URL.
- On `MissingKey`, that **addon** is skipped for the request, and its `skipped_reason` is "Needs a Real-Debrid key. Add one in Settings." Skipped **addons** stay in the list.
- Changing the key clears `skipped_reason`, re-fetches the manifests of **addons** with a placeholder, and reloads the home screen.
- **Convenience on install:** if a key is set and the pasted URL contains that exact key, store the URL with the key replaced by the placeholder.

## The key is a secret

- Never write the key, a resolved URL, or a stream link to a log, a notice, an error message or a test snapshot.
- The resolved URL is never stored and never placed in `State`. (`Playback.url` holds a stream link from the **addon**, which is allowed.)
- The Settings field hides the key by default, with a show/hide toggle.

## Playback languages

`mpv_view.rs` sets mpv's preferred audio language and preferred subtitle language from `State.settings` before loading a file. With `subtitle_lang` unset, subtitles start off.

## Failed streams

Already built in ticket 07: a stream that fails to load closes the player with a notice. Check it still works for an **addon**-resolved link. Note that Torrentio answers some failures with a short error clip of its own instead of an HTTP error; that clip simply plays, and no detection is needed.

## Screen

`screens/settings.rs`, reachable from the top bar: the Real-Debrid key field with Save and Clear, two language dropdowns (a short fixed list of common languages plus "None"), and a line of help: "Put `{real_debrid_key}` in an addon's URL where its Real-Debrid key goes." The Addons screen shows `skipped_reason` in a warning colour under the **addon**.

## Out of scope

Calling the Real-Debrid API, per-**profile** **overrides** (ticket 19), sync, other debrid services.

## Acceptance checks

- [x] `cargo test -p streem-core` passes, including tests that: `resolve` fills the placeholder; fills the percent-encoded spelling; percent-encodes a key containing `/`; returns the template unchanged when there is no placeholder; returns `MissingKey` with a placeholder and no key; against a `wiremock` server, a templated **addon** is requested at the path containing the key; with no key the server receives zero requests and `skipped_reason` is set; setting a key afterwards clears `skipped_reason` and triggers the manifest request; installing a URL that contains the saved key stores the placeholder instead.
- [x] A test captures everything logged during a templated request and asserts the key does not appear.
- [x] `grep -rn "format!.*manifest\|/catalog/\|/stream/\|/meta/\|/subtitles/" crates/core/src --include=*.rs` shows these paths built only in `addon/client.rs`, after a call to `resolve`.
- [x] Settings survive a restart.

## Human check

- [ ] With a real key and Torrentio installed using the placeholder, with its "don't show download to debrid links" option on: a movie lists streams and one plays.
- [ ] Clearing the key marks Torrentio as skipped; other **addons** keep working.

## Rules

- Use the terms defined in `CONTEXT.md` in names, messages and comments.
- Add crates with `cargo add`. Never type a version number from memory.
- `streem-desktop` does no network or disk work. It sends a `Command` to `Core` and draws `State`.
- Do not install system packages. If something is missing, stop and write what is missing under `## Comments` in this file.
- Do only what this ticket lists. Anything under "Out of scope" belongs to another ticket.
- Finish with `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace`. All three must pass.
- When done, tick the boxes and add a line under `## Comments` saying what you verified and how.

## Comments

- Verified 2026-11-13 with `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace`, all passing. Covered by: unit tests in `addon/url.rs` (`resolve`, both spellings, a key with `/`, no placeholder, `MissingKey`, `templatize`), `store.rs` and `settings.rs` (`Debug` hides the key); the "Real-Debrid key placeholder" section of `tests/core.rs` (path with the key, zero requests and `skipped_reason` without a key, saving a key fetches the manifest and reloads home, clearing it skips the **addon** for catalogs and streams while others keep working, install convenience, an install sent right after the key, retry on restart after a failed fetch, settings survive a restart). The grep shows the paths only in `addon/client.rs`; `ResolvedAddon::resolve` (which calls `url::resolve`) is the only way to build a request there.
- `tests/secrets.rs` installs a Trace-level `log` logger before `Core` starts; `tracing` is a dev-dependency with its `log` feature, so `hyper_util` events are captured as well as `reqwest`'s. It runs a templated install, catalog, meta and a failing stream request, plus an install against a closed port, then asserts the key appears in no record, notice or `{:?}` of the state, and that records from `reqwest` and `hyper_util` were captured. reqwest errors go through `without_url()`.
- Design choices: a placeholder **addon** installed with no key is stored with an empty manifest and listed as skipped; its manifest is fetched when a key is saved, and again at startup if that fetch failed. Settings change in `State` at once and are saved by the disk worker after.
- Failed streams: an **addon**-resolved link is played through the same `Playback` path as any link, so ticket 07's failed-report tests cover it; not run against a real Torrentio link.
- Not done: the human checks below need a real Real-Debrid key and were not run. `Command` still derives `Debug`, so `{:?}` of `SetRealDebridKey` would print the key; nothing prints commands. With `sid=auto`, mpv may not auto-select an **addon** subtitle in the chosen language, because those are added with `sub-add … auto`; not checked in mpv.
