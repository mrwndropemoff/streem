# 09: Subtitles from addons and track selection

**What to build:** While a video plays, the player has an audio menu and a subtitle menu. The subtitle menu lists the tracks inside the file and the subtitles supplied by **addons**, plus "Off". Choosing one switches immediately.

**Blocked by:** 07

**Status:** ready-for-agent

## Read first

- `CONTEXT.md`
- `crates/core/src/playback.rs`, `crates/core/src/app.rs`, `crates/core/src/addon/client.rs`
- `crates/desktop/src/mpv_view.rs`, `crates/desktop/src/screens/player.rs`
- `prototypes/egui-mpv/src/main.rs`, for how the prototype listed and switched tracks.

## Files

- Edit: `crates/core/src/lib.rs`, `crates/core/src/playback.rs`, `crates/core/src/app.rs`, `crates/core/src/addon/client.rs`, `crates/core/src/addon/types.rs`
- Edit: `crates/desktop/src/mpv_view.rs`, `crates/desktop/src/screens/player.rs`

## Additions to the interface

```rust
pub struct SubtitleTrack { pub lang: String, pub url: String, pub addon_name: String }

// Playback
pub subtitles: Vec<SubtitleTrack>,
```

## Addon protocol

- A subtitles request is `GET {base}/subtitles/{type}/{videoId}.json`, answering `{"subtitles": [{"id", "url", "lang"}]}`.
- When `Play` starts a video, ask every installed **addon** whose manifest offers the `subtitles` resource for this type and id prefix, all at once, in the background. Playback must not wait for the answers.
- As each answer arrives, append its tracks to `Playback.subtitles`, but only if that same video is still playing.
- A failing subtitles **addon** is ignored silently. It must never stop playback or raise a notice.

## Behaviour in `streem-desktop`

- Track switching is local to the player: `mpv_view.rs` may read mpv's track list and set its audio and subtitle track directly. This is the one kind of player control that does not go through a command.
- When a new entry appears in `Playback.subtitles`, add it to mpv as an external subtitle without selecting it, labelled with its language.
- The audio menu lists mpv's audio tracks by language and title. The subtitle menu lists "Off", the embedded tracks, then the **addon** tracks labelled "language · addon name".
- Both menus sit in the player's control bar.

## Out of scope

Preferred languages (ticket 12), subtitle styling, subtitle delay.

## Acceptance checks

- [ ] `cargo test -p streem-core` passes, including tests against a `wiremock` server that: `Play` returns a `playback` immediately while the subtitles request is still pending; tracks appear in `Playback.subtitles` afterwards; a subtitles **addon** answering HTTP 500 leaves `playback` intact and `notice` empty; tracks arriving after `StopPlayback` are discarded.
- [ ] `cargo run -p streem-desktop` with the sample from `prototypes/egui-mpv/sample/` served over HTTP, or any stream with two audio tracks: both menus list the tracks.

## Human check

- [ ] Switching audio changes the sound; switching subtitles shows, changes and hides the text.
- [ ] With a subtitles **addon** installed, its tracks appear in the menu and display when chosen.

## Rules

- Use the terms defined in `CONTEXT.md` in names, messages and comments.
- Add crates with `cargo add`. Never type a version number from memory.
- `streem-desktop` does no network or disk work. It sends a `Command` to `Core` and draws `State`. mpv fetching the video and subtitle files itself is the one exception.
- Do not install system packages. If something is missing, stop and write what is missing under `## Comments` in this file.
- Do only what this ticket lists. Anything under "Out of scope" belongs to another ticket.
- Finish with `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace`. All three must pass.
- When done, tick the boxes and add a line under `## Comments` saying what you verified and how.

## Comments
