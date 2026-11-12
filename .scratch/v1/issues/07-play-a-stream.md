# 07: Play a stream in the embedded player

**What to build:** Clicking a playable stream opens the player and the video plays inside the app window. Play, pause, seek and "back to the stream list" all work. Every one of those actions travels through `streem-core` as a command; `streem-desktop` only makes mpv match what `streem-core` says.

**Blocked by:** 06

**Status:** ready-for-agent

## Read first

- `CONTEXT.md`
- `.scratch/v1/spec.md`, section "Workspace", rule 2.
- `prototypes/egui-mpv/src/main.rs` and the `## Comments` of tickets 01 and 02: they say which libmpv approach works on Linux and Windows and what pitfalls to avoid.
- `crates/core/src/lib.rs`, `crates/core/src/app.rs`
- `crates/desktop/src/main.rs`, `crates/desktop/src/screens/details.rs`

## Files

- New: `crates/core/src/playback.rs`
- Edit: `crates/core/src/lib.rs`, `crates/core/src/app.rs`
- New: `crates/desktop/src/mpv_view.rs` (the libmpv widget, ported from the prototype)
- New: `crates/desktop/src/screens/player.rs`
- Edit: `crates/desktop/src/main.rs`, `crates/desktop/src/screens/mod.rs`, `crates/desktop/src/screens/details.rs`

## Crates

- `streem-desktop`: the same libmpv crate the prototype ended up using. `streem-core` gets no mpv dependency.

## Additions to the interface

```rust
pub struct Playback {
    // what is playing
    pub meta_type: String,
    pub meta_id: String,
    pub video_id: String,
    pub title: String,            // "Name" or "Name · S1E2 · Episode title"
    pub poster: Option<String>,
    pub addon_id: String,
    pub stream_name: String,
    pub filename: Option<String>,
    pub url: String,              // only mpv_view reads this

    // what the player should be doing
    pub paused: bool,
    pub seek: Option<SeekRequest>,

    // what the player last reported
    pub position_secs: f64,
    pub duration_secs: f64,
    pub buffering: bool,
}

pub struct SeekRequest { pub id: u64, pub position_secs: f64 }

pub struct PlaybackReport {
    pub position_secs: f64,
    pub duration_secs: f64,
    pub buffering: bool,
    pub ended: bool,
    pub failed: Option<String>,
}

// State
pub playback: Option<Playback>,

// Command
Play { addon_id: String, stream_index: usize },   // indexes into the current Details.streams group
SetPaused(bool),
Seek { position_secs: f64 },
StopPlayback,
ReportPlayback(PlaybackReport),
```

## Behaviour in `streem-core` (`playback.rs`)

- `Play` looks up the stream in `State.details.streams`, ignores it if it is not playable, and sets `State.playback` with `paused = false` and no seek.
- `SetPaused` sets `paused`. `Seek` sets `seek` to a new `SeekRequest` with an id one higher than the last one used, clamped to `0..=duration_secs`.
- `ReportPlayback` copies position, duration and buffering into `Playback`. Position-only reports must not call `on_change` more than once a second.
- `ended` or `StopPlayback` sets `playback` to `None`.
- `failed` sets `playback` to `None` and raises the notice "This stream could not be played. Pick another one."
- Never log `Playback.url` or any stream link. Later these links contain a Real-Debrid key.
- Keep this logic as plain functions over the state so it is testable without a player.

## Behaviour in `streem-desktop`

- `mpv_view.rs` owns the mpv instance and draws the video. Each frame it compares mpv with `State.playback` and corrects mpv:
  - a different `url` (or none loaded): load it;
  - a different pause state: set mpv's `pause`;
  - a `seek.id` it has not applied yet: seek to `seek.position_secs`, remember the id.
- It sends `ReportPlayback` about once a second while playing, immediately when buffering starts or stops, and once with `ended` or `failed` when mpv ends the file or fails to load it.
- `screens/player.rs` shows the video full-window with a control bar over it: Back, Play/Pause, seek slider, time label, title. The bar hides after 3 seconds without mouse movement.
- The controls only send commands (`SetPaused`, `Seek`, `StopPlayback`). They never call mpv. The seek slider shows `Playback.position_secs`.
- Keyboard: Space toggles pause, Left and Right seek 10 seconds, Escape sends `StopPlayback`, F toggles fullscreen.
- When `State.playback` becomes `None`, tear down the mpv file and show the details screen again.

## Out of scope

Subtitle and audio menus, resume position, watch history, Real-Debrid, **watch parties**.

## Acceptance checks

- [ ] `cargo test -p streem-core` passes, including tests that: `Play` on a playable stream sets `playback` with the right url and `paused == false`; `Play` on an unplayable stream changes nothing; `SetPaused(true)` sets `paused`; two `Seek` commands produce increasing `seek.id` values; a seek beyond the duration is clamped; a report with `ended` clears `playback`; a report with `failed` clears `playback` and sets the notice.
- [ ] `grep -n "mpv" crates/desktop/src/screens/player.rs` shows no direct mpv call for play, pause or seek.
- [ ] `grep -rn "mpv" crates/core/` prints nothing outside comments.
- [ ] `cargo run -p streem-desktop`: pick a title with a direct HTTP stream and click it.

## Human check

- [ ] The video plays inside the window with the control bar over it.
- [ ] Pause, seek, Space, arrows, Escape and Back all work.
- [ ] A stream with a broken link shows the notice and returns to the stream list.

## Rules

- Use the terms defined in `CONTEXT.md` in names, messages and comments.
- Add crates with `cargo add`. Never type a version number from memory.
- `streem-desktop` does no network or disk work. It sends a `Command` to `Core` and draws `State`. mpv fetching the video itself is the one exception.
- Do not install system packages. If something is missing, stop and write what is missing under `## Comments` in this file.
- Do only what this ticket lists. Anything under "Out of scope" belongs to another ticket.
- Finish with `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace`. All three must pass.
- When done, tick the boxes and add a line under `## Comments` saying what you verified and how.

## Comments
