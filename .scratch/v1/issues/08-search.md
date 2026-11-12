# 08: Search

**What to build:** A search box in the top bar. Typing a query and pressing Enter shows poster rows of results, one row per **addon** catalog that supports search. Clicking a result opens its details.

**Blocked by:** 04

**Status:** ready-for-agent

## Read first

- `CONTEXT.md`
- `crates/core/src/lib.rs`, `crates/core/src/app.rs`, `crates/core/src/addon/client.rs`
- `crates/desktop/src/main.rs`, `crates/desktop/src/screens/home.rs`

## Files

- Edit: `crates/core/src/lib.rs`, `crates/core/src/app.rs`, `crates/core/src/addon/client.rs`
- New: `crates/desktop/src/screens/search.rs`
- Edit: `crates/desktop/src/main.rs`, `crates/desktop/src/screens/mod.rs`, `crates/desktop/src/screens/home.rs`

## Additions to the interface

```rust
pub struct Search { pub query: String, pub rows: Vec<CatalogRow> }

// State
pub search: Option<Search>,

// Command
Search { query: String },
ClearSearch,
```

## Addon protocol

- A catalog supports search when its `extra` list contains an entry named `search`.
- The request is `GET {base}/catalog/{type}/{id}/search={query}.json`, with the query percent-encoded. The response has the same shape as any catalog.
- Skip a catalog that has some other extra marked `isRequired: true`.
- `Search` creates one `Loading` row per searchable catalog and fetches them all at once, exactly like `LoadHome`. Reuse that code; move the shared part into one function used by both.
- A new `Search` while one is running replaces `State.search`; results from the older query must be ignored when they arrive.
- An empty or whitespace-only query behaves like `ClearSearch`.

## Screen

- A text field in the top bar. Enter sends `Search` and switches to the search screen.
- `screens/search.rs` draws the rows with the same row widget as the home screen. Move that widget out of `home.rs` into a function both screens call.
- Rows that finished with zero results are hidden. If every row is finished and empty, show "No results".
- If poster clicks already open details (ticket 05 is done), they must do the same here.

## Out of scope

Search-as-you-type, search history, filters.

## Acceptance checks

- [ ] `cargo test -p streem-core` passes, including tests against a `wiremock` server that: only the catalog declaring the `search` extra is queried; the query `the wire` is sent percent-encoded; results from a superseded query never appear in `State.search`; an empty query sets `search` to `None`.
- [ ] `cargo run -p streem-desktop`: searching "matrix" shows Cinemeta movie and series rows.

## Rules

- Use the terms defined in `CONTEXT.md` in names, messages and comments.
- Add crates with `cargo add`. Never type a version number from memory.
- `streem-desktop` does no network or disk work. It sends a `Command` to `Core` and draws `State`.
- Do not install system packages. If something is missing, stop and write what is missing under `## Comments` in this file.
- Do only what this ticket lists. Anything under "Out of scope" belongs to another ticket.
- Finish with `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace`. All three must pass.
- When done, tick the boxes and add a line under `## Comments` saying what you verified and how.

## Comments
