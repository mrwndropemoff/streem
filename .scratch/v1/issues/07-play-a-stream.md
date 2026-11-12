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

- [x] `cargo test -p streem-core` passes, including tests that: `Play` on a playable stream sets `playback` with the right url and `paused == false`; `Play` on an unplayable stream changes nothing; `SetPaused(true)` sets `paused`; two `Seek` commands produce increasing `seek.id` values; a seek beyond the duration is clamped; a report with `ended` clears `playback`; a report with `failed` clears `playback` and sets the notice.
- [x] `grep -n "mpv" crates/desktop/src/screens/player.rs` shows no direct mpv call for play, pause or seek.
- [x] `grep -rn "mpv" crates/core/` prints nothing outside comments.
- [x] `cargo run -p streem-desktop`: pick a title with a direct HTTP stream and click it.

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

- **Core.** `cargo test --workspace` passes: 13 new wiremock tests in `crates/core/tests/core.rs` (play with link and title, an episode's "Name · S1E2 · Title", unplayable or missing stream changes nothing, pause, increasing seek ids, clamping both ways, report fields, ended and stop clear playback with no notice, failed clears it and raises the notice, late commands and reports after the end are ignored, position reports raise `on_change` at most once a second, and the link is not in `{:?}` output). I broke the throttle on purpose to see its test fail (50 `on_change` calls), then restored it. `cargo fmt --all` and `cargo clippy --workspace --all-targets -- -D warnings` are clean. Both greps in the acceptance list print nothing.
- **Desktop, run by the agent on Windows 11** with a small local Node server standing in as an **addon** (manifest, meta, one good and one 404 stream, and `sample.mkv` with Range support), and a throwaway script (not committed) that sent `Command`s to `Core` and logged `State` once a second. Seen: the duration arrives after load; pause freezes the position; a seek while paused moves the position and playback continues from there after resume; a seek past the end ends the file, clears `playback` and shows details again with no notice; the 404 stream raises "This stream could not be played. Pick another one." and returns to the stream list; playing the same stream again after Stop works; closing the window while playing exits with code 0. By real mouse and keyboard on the running window, with screenshots: Home poster, Details, clicking a stream row (a good one plays, the broken one raises the notice), the control bar over the video with title, Back, Pause, slider and time, the bar hiding after 3 seconds and coming back on mouse movement, a plain click on the seek slider (seeked to the click), Back, Space, Left (-10 s), Right (+10 s), Escape, and F going to fullscreen and Escape leaving it.
- **Not checked by the agent:** dragging the slider (only a plain click was tried), the buffering spinner, Linux, a very long file, and the **Human check** list below, which stays unticked for a person to run.
- **Windows setup for this ticket** (full steps are in the README): `libmpv-2.dll` must be copied next to the program, `target\debug\` for `cargo run` and `cargo test -p streem-desktop`; without it the program does not start. The linking setup from ticket 02 (`mpv.lib` and `%USERPROFILE%\.cargo\config.toml`) was already in place on this machine. The crate is `libmpv2` 7.0.1, the same as the prototype.
- **Choices that go beyond the ticket text:**
  - `Seek` also sets `position_secs` to the target, so the slider does not jump back until the next report. While the duration is still 0 (unknown), a seek is only clamped at 0.
  - `CloseTitle` also stops playback, since the stream list it came from is gone.
  - `Core` keeps each stream's link in a list parallel to the stream groups (`stream_links` in `app.rs`). `Playback` has a hand-written `Debug` that leaves the link out. `State` and `Command` no longer derive `Eq`, because they now hold `f64`.
  - The player screen replaces the whole window while `State.playback` is set (no top bar, no notice banner), and `after_playback` in `main.rs` leaves fullscreen when playback ends.
  - If libmpv cannot start, the app still opens; playing a stream then reports a failure, which raises the same notice.
  - One mpv instance is created at startup and never freed (`Box::leak`, as in the prototype). Freeing the render context needs the OpenGL context current, which is not guaranteed at shutdown.
  - mpv's `terminal` is off, so mpv never prints a stream link.
  - Space, the arrow keys and Escape are consumed before the widgets are drawn, so a focused button or slider does not act on the same key press twice. A click on the seek slider that is not a drag is handled too (`drag_stopped` does not fire for it).
- **After a two-axis review:** the `clock` unit test in `player.rs` was removed (the spec says `streem-desktop` is not unit tested). A `loadfile` command that mpv refuses now sends a `failed` report instead of leaving the spinner up forever. `mpv_view.rs` got `forget_file` and `send_report` in place of repeated code, and `App::play` became `show_player`. Known and left alone: a Stop followed by a Play of the same link inside one frame would not reload mpv (the person cannot do this by hand); the first click on a hidden bar also reaches the button under it; the arrow keys use the position as last published, which is up to a second old while paused. After these changes `cargo fmt`, `clippy -D warnings` and `cargo test --workspace` pass and the app starts. The click-through run above was done before this small refactor and was not repeated.
- The existing "← Back" button on the details screen shows a missing-glyph box on this Windows machine (the default egui font has no arrow). The player's button says just "Back". The details one is from an earlier ticket and was left alone.
- **After a second review:** the player and `Core` both used a 1 s interval, so a position report that arrived a little early was held back until the next one, and the published position could lag by about 2 s. `Core` now publishes a held-back position itself once the second is up, so the slider and the arrow keys are at most about a second behind, and `on_change` still runs at most once a second for position-only reports. `position_reports_publish_at_most_once_a_second` now checks that the last held-back position is published within 1.5 s without another report (it timed out before the change). `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` pass.
- **Paste in the Addons field:** while testing this ticket by hand, Ctrl+V did nothing in the addon URL field. `streem-desktop` turns off eframe's default features and never turned `clipboard` back on (since ticket 03), so egui had no system clipboard. Added with `cargo add -p streem-desktop eframe --features clipboard`. fmt, clippy and the workspace tests pass. A person then confirmed on Windows that pasting an addon URL works and the addon installs.
