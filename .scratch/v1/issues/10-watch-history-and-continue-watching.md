# 10: Watch history and continue-watching

**What to build:** The app remembers how far the user got in everything they play. The home screen gains a "Continue watching" row at the top. Playing a title again resumes where it stopped. All of it survives a restart.

**Blocked by:** 07

**Status:** ready-for-agent

## Read first

- `CONTEXT.md`
- `crates/core/src/playback.rs`, `crates/core/src/store.rs`, `crates/core/src/app.rs`
- `crates/desktop/src/screens/home.rs`

## Files

- New: `crates/core/src/history.rs`
- Edit: `crates/core/src/lib.rs`, `crates/core/src/playback.rs`, `crates/core/src/store.rs`, `crates/core/src/app.rs`
- Edit: `crates/desktop/src/screens/home.rs`

## Additions to the interface

```rust
pub struct ContinueItem {
    pub meta_type: String,
    pub meta_id: String,
    pub video_id: String,
    pub name: String,       // Playback.title
    pub poster: Option<String>,
    pub position_secs: f64,
    pub duration_secs: f64,
}

// State
pub continue_watching: Vec<ContinueItem>,

// Command
DismissContinue { meta_id: String },
```

## Storage

Add a migration to `store.rs` creating table `watch_progress`:

`account_id TEXT`, `profile_id TEXT`, `video_id TEXT`, `meta_id TEXT`, `meta_type TEXT`, `name TEXT`, `poster TEXT`, `position_secs REAL`, `duration_secs REAL`, `updated_at INTEGER` (milliseconds since 1970), `deleted INTEGER`, `dirty INTEGER`, primary key `(profile_id, video_id)`.

Until sign-in and **profiles** exist, `account_id` and `profile_id` are both the text `local`. Every write sets `dirty = 1` and a fresh `updated_at`. Sync uses these columns in ticket 18.

## Behaviour (`history.rs`)

- While something plays, save its position when a `ReportPlayback` arrives, at most once every 5 seconds. Always save once more on `StopPlayback`, on `ended`, and when a different video starts.
- Do not save anything for a video that never got past 10 seconds.
- A video counts as finished when its position is at or beyond 90% of its duration.
- `continue_watching` is rebuilt after every save: unfinished, non-deleted rows; for a series keep only the most recently updated video per `meta_id`; newest first.
- `DismissContinue` sets `deleted = 1` on every row of that `meta_id`.
- `Play` resumes: if a saved, unfinished position exists for that `video_id`, start with `seek` set to it. A finished video starts from 0.
- Load `continue_watching` from the store at startup.

## Screen

- "Continue watching" is the first row on the home screen, shown only when not empty, drawn with the same row widget.
- Each poster has a thin progress bar along its bottom edge and a small "×" that sends `DismissContinue`.
- Clicking a poster opens its details. For an episode, the season and episode of `video_id` should be pre-selected and its stream list loaded.

## Out of scope

Sync, per-profile separation, a full history screen, marking episodes as watched in the episode list.

## Acceptance checks

- [ ] `cargo test -p streem-core` passes, including tests (with an injected clock, no sleeping) that: reports 1 second apart cause at most one store write per 5 seconds; `StopPlayback` saves the last position; a new `Core` on the same data directory lists the item in `continue_watching`; a position at 95% is not listed; two episodes of one series produce one item, the newer; `Play` on a saved video sets `seek` to the saved position; `Play` on a finished video sets no seek; `DismissContinue` removes the item and it stays removed after a restart; a video stopped at 5 seconds is not saved.
- [ ] `cargo run -p streem-desktop`: play something for 30 seconds, press Escape, and see it in "Continue watching"; click it and play again, and it resumes.

## Rules

- Use the terms defined in `CONTEXT.md` in names, messages and comments.
- Add crates with `cargo add`. Never type a version number from memory.
- `streem-desktop` does no network or disk work. It sends a `Command` to `Core` and draws `State`.
- Do not install system packages. If something is missing, stop and write what is missing under `## Comments` in this file.
- Do only what this ticket lists. Anything under "Out of scope" belongs to another ticket.
- Finish with `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace`. All three must pass.
- When done, tick the boxes and add a line under `## Comments` saying what you verified and how.

## Comments
