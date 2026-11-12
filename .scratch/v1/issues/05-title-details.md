# 05: Title details

**What to build:** Clicking a poster opens a details screen for that title: its name, artwork, description, year, rating and genres. A series also shows a season picker and the episodes of the chosen season. A Back button returns to where the user was.

**Blocked by:** 04

**Status:** ready-for-agent

## Read first

- `CONTEXT.md`
- `crates/core/src/lib.rs`, `crates/core/src/app.rs`, `crates/core/src/addon/client.rs`, `crates/core/src/addon/types.rs`
- `crates/desktop/src/main.rs`, `crates/desktop/src/screens/home.rs`

## Files

- Edit: `crates/core/src/lib.rs`, `crates/core/src/app.rs`, `crates/core/src/addon/client.rs`, `crates/core/src/addon/types.rs`
- New: `crates/desktop/src/screens/details.rs`
- Edit: `crates/desktop/src/main.rs`, `crates/desktop/src/screens/mod.rs`, `crates/desktop/src/screens/home.rs`

## Additions to the interface

```rust
pub struct Details {
    pub meta_type: String,
    pub meta_id: String,
    pub meta: Load<Meta>,
}

pub struct Meta {
    pub id: String,
    pub meta_type: String,
    pub name: String,
    pub poster: Option<String>,
    pub background: Option<String>,
    pub description: Option<String>,
    pub release_info: Option<String>,
    pub rating: Option<String>,
    pub genres: Vec<String>,
    pub videos: Vec<Video>,
}

pub struct Video { pub id: String, pub title: String, pub season: Option<u32>, pub episode: Option<u32>, pub released: Option<String> }

// State
pub details: Option<Details>,

// Command
OpenTitle { meta_type: String, meta_id: String },
CloseTitle,
```

## Addon protocol

- A meta request is `GET {base}/meta/{type}/{id}.json`, answering `{"meta": {...}}`. JSON field names: `releaseInfo`, `imdbRating`, `genres`, `videos`; each video has `id`, `title` (older addons use `name`), `season`, `episode`, `released`.
- Pick the **addon** to ask this way: go through the installed **addons** in order and take the first whose manifest offers the `meta` resource for this type and whose `idPrefixes` (if present) match the start of the id. A `resources` entry given as an object carries its own `types` and `idPrefixes`; a plain string uses the manifest-level ones.
- If no **addon** matches, or the request fails, `meta` becomes `Failed` with a readable message.
- A movie has no `videos`. Treat it as having one video whose id is the meta id.

## Screen

- `screens/details.rs`: background image if any (through `core.poster`), poster, name, release info, rating, genres, description.
- For a series: a season dropdown (seasons sorted ascending, season 0 last and labelled "Specials") and the list of that season's episodes as "E3 · Title".
- Which screen is showing is kept in `streem-desktop` as a plain enum; opening details also sends `OpenTitle`, and Back sends `CloseTitle`.
- Clicking an episode does nothing yet. Ticket 06 uses it.

## Out of scope

Streams, playback, library button, continue-watching.

## Acceptance checks

- [ ] `cargo test -p streem-core` passes, including tests against a `wiremock` server that: `OpenTitle` for a movie reaches `Ready` with the fields filled; a series returns its videos with seasons and episodes; with two **addons** where only the second matches the id prefix, the second is the one queried; no matching **addon** gives `Failed`; `CloseTitle` sets `details` to `None`.
- [ ] `cargo run -p streem-desktop`: clicking a Cinemeta movie poster shows its details; clicking a series shows seasons and episodes; Back returns to Home at the same scroll position.

## Rules

- Use the terms defined in `CONTEXT.md` in names, messages and comments.
- Add crates with `cargo add`. Never type a version number from memory.
- `streem-desktop` does no network or disk work. It sends a `Command` to `Core` and draws `State`.
- Do not install system packages. If something is missing, stop and write what is missing under `## Comments` in this file.
- Do only what this ticket lists. Anything under "Out of scope" belongs to another ticket.
- Finish with `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace`. All three must pass.
- When done, tick the boxes and add a line under `## Comments` saying what you verified and how.

## Comments
