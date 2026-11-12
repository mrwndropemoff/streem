# 06: Stream list for a title

**What to build:** On the details screen, a movie shows its available streams straight away, and a series shows them after an episode is clicked. Streams are grouped by the **addon** that supplied them. Streams the app cannot play are shown greyed out with the reason.

**Blocked by:** 05

**Status:** ready-for-agent

## Read first

- `CONTEXT.md`
- `.scratch/v1/spec.md`, row "Torrent engine" in "Decisions".
- `crates/core/src/lib.rs`, `crates/core/src/app.rs`, `crates/core/src/addon/client.rs`, `crates/core/src/addon/types.rs`
- `crates/desktop/src/screens/details.rs`

## Files

- Edit: `crates/core/src/lib.rs`, `crates/core/src/app.rs`, `crates/core/src/addon/client.rs`, `crates/core/src/addon/types.rs`
- Edit: `crates/desktop/src/screens/details.rs`

## Additions to the interface

```rust
pub struct StreamList {
    pub video_id: String,
    pub groups: Vec<StreamGroup>,
}

pub struct StreamGroup {
    pub addon_id: String,      // the manifest id
    pub addon_name: String,
    pub streams: Load<Vec<StreamItem>>,
}

pub struct StreamItem {
    pub name: String,
    pub description: String,
    pub filename: Option<String>,
    pub playable: bool,
    pub unplayable_reason: Option<String>,
}

// Details
pub streams: Option<StreamList>,

// Command
LoadStreams { video_id: String },
```

The stream's link stays inside `streem-core`. It is not part of `StreamItem`.

## Addon protocol

- A stream request is `GET {base}/stream/{type}/{videoId}.json`, answering `{"streams": [...]}`. For a movie the video id is the meta id. For an episode it is the video's own `id` (for Cinemeta series it looks like `tt0903747:1:2`).
- Ask every installed **addon** whose manifest offers the `stream` resource for this type and id prefix, all at once. Create each group as `Loading` first.
- A stream object has `name`, `description` (older addons use `title`), and one of `url`, `infoHash` (with optional `fileIdx`), `ytId` or `externalUrl`. It may have `behaviorHints.filename`.
- `filename` is `behaviorHints.filename` if present; otherwise the last path segment of `url`, percent-decoded, if it has a file extension; otherwise `None`.
- `playable` is true only when `url` starts with `http://` or `https://`.
- Reasons for the others: `infoHash` gives "Torrent streams are not supported"; anything else gives "This kind of stream is not supported".
- A group with no streams is dropped once it has loaded.

## Screen

- Movie: send `LoadStreams` with the meta id as soon as its details are `Ready`.
- Series: send `LoadStreams` when an episode is clicked, and show the streams for that episode.
- Under each **addon** name, one row per stream with its name and description. Unplayable rows are greyed out and show the reason. Clicking a playable row does nothing yet; ticket 07 uses it.

## Out of scope

Playback, Real-Debrid, sorting or filtering streams.

## Acceptance checks

- [ ] `cargo test -p streem-core` passes, including tests against a `wiremock` server that: two **addons** give two groups that load independently; a stream with `url` is playable; a stream with only `infoHash` is unplayable with the torrent reason; `filename` comes from `behaviorHints.filename`, and from the URL's last segment when the hint is absent; an **addon** without the `stream` resource is not asked; a failing **addon** yields a `Failed` group and leaves the others intact.
- [ ] `cargo run -p streem-desktop` with a stream **addon** installed: a movie shows its stream list; a series shows it after an episode click.

## Rules

- Use the terms defined in `CONTEXT.md` in names, messages and comments.
- Add crates with `cargo add`. Never type a version number from memory.
- `streem-desktop` does no network or disk work. It sends a `Command` to `Core` and draws `State`.
- Do not install system packages. If something is missing, stop and write what is missing under `## Comments` in this file.
- Do only what this ticket lists. Anything under "Out of scope" belongs to another ticket.
- Finish with `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace`. All three must pass.
- When done, tick the boxes and add a line under `## Comments` saying what you verified and how.

## Comments
