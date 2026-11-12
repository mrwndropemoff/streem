# 04: Browse catalogs as poster rows

**What to build:** The home screen. It shows one horizontal row of posters for each catalog offered by the installed **addons**. Rows and posters fill in as they arrive; the window stays responsive and scrolls smoothly the whole time.

**Blocked by:** 03

**Status:** ready-for-agent

## Read first

- `CONTEXT.md`
- `.scratch/v1/spec.md`, section "Workspace" (the two rules).
- `crates/core/src/lib.rs`, `crates/core/src/app.rs`, `crates/core/src/addon/client.rs`
- The `## Comments` of ticket 03, for whether protocol types come from stremio-core or from `addon/types.rs`.

## Files

- Edit: `crates/core/src/lib.rs`, `crates/core/src/app.rs`, `crates/core/src/addon/client.rs`, `crates/core/src/addon/types.rs`
- New: `crates/core/src/posters.rs`
- New: `crates/desktop/src/screens/home.rs`
- Edit: `crates/desktop/src/main.rs`, `crates/desktop/src/screens/mod.rs`

## Crates

- `streem-core`: `image` (JPEG, PNG and WebP decoding), `sha2` or any stable hash for cache file names.

## Additions to the interface

```rust
pub enum Load<T> { Loading, Ready(T), Failed(String) }

pub struct CatalogRow {
    pub addon_name: String,
    pub catalog_name: String,
    pub meta_type: String,
    pub items: Load<Vec<MetaPreview>>,
}

pub struct MetaPreview { pub id: String, pub meta_type: String, pub name: String, pub poster: Option<String> }

// State
pub home: Vec<CatalogRow>,

// Command
LoadHome,

// Core
/// Returns the decoded poster if it is ready. On the first call for a URL it starts
/// loading in the background and returns None; `on_change` fires when it is ready.
pub fn poster(&self, url: &str) -> Option<Arc<Poster>>;

pub struct Poster { pub width: u32, pub height: u32, pub rgba: Vec<u8> }
```

## Addon protocol

- A catalog request is `GET {base}/catalog/{type}/{id}.json`, where `{base}` is the manifest URL without `/manifest.json`. Percent-encode `{type}` and `{id}`.
- The response is `{"metas": [{"id", "type", "name", "poster", ...}]}`.
- Show a catalog on the home screen only if none of its `extra` entries has `isRequired: true`.
- `LoadHome` creates one `CatalogRow` per such catalog in the `Loading` state straight away, then fetches all of them concurrently. Each row turns `Ready` or `Failed` on its own. One failing **addon** never affects the other rows.
- Load only the first page. Add the comment `// shortcut: first page only, add the "skip" extra when rows need to scroll further`.
- Send `LoadHome` on startup and after an **addon** is installed or removed.

## Posters

- `posters.rs` downloads the image, stores the original bytes in `<data_dir>/posters/<hash of url>`, decodes it on a blocking task, scales it down to at most 300 pixels wide, and keeps the RGBA result in memory.
- A second request for the same URL, in this run or a later one, never hits the network.
- Add the comment `// shortcut: in-memory posters are never evicted, add an LRU cap when memory use matters`.
- `streem-desktop` turns a `Poster` into an egui texture once, and keeps the texture handle in its own map keyed by URL.

## Screen

- `screens/home.rs`: a vertical scroll area of rows. Each row has a title (`catalog_name` and `addon_name`) and a horizontal scroll area of posters with the name under each.
- Call `core.poster(url)` only for posters that are currently visible (`ui.is_rect_visible`). Draw a grey placeholder of the same size while it returns `None`.
- A `Loading` row shows placeholders; a `Failed` row shows its message.
- Add a simple top bar to `main.rs` to switch between Home and Addons.

## Out of scope

Clicking a poster, search, continue-watching, pagination.

## Acceptance checks

- [x] `cargo test -p streem-core` passes, including tests against a `wiremock` server that: two catalogs produce two rows that go from `Loading` to `Ready`; one **addon** returning HTTP 500 gives one `Failed` row while the other is `Ready`; a catalog with a required extra produces no row; requesting the same poster twice makes exactly one HTTP request; a new `Core` on the same data directory serves that poster with zero HTTP requests.
- [x] `cargo run -p streem-desktop` shows Cinemeta's rows with posters.
- [x] `grep -rn "reqwest\|rusqlite\|std::fs" crates/desktop/src` prints nothing.

## Human check

- [x] Scrolling vertically and horizontally stays smooth while posters are still loading.

## Rules

- Use the terms defined in `CONTEXT.md` in names, messages and comments.
- Add crates with `cargo add`. Never type a version number from memory.
- `streem-desktop` does no network or disk work. It sends a `Command` to `Core` and draws `State`.
- Do not install system packages. If something is missing, stop and write what is missing under `## Comments` in this file.
- Do only what this ticket lists. Anything under "Out of scope" belongs to another ticket.
- Finish with `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace`. All three must pass.
- When done, tick the boxes and add a line under `## Comments` saying what you verified and how.

## Comments
**Notes for later tickets**
- Protocol types stay our own (`addon/types.rs`), following ticket 03: added `ManifestCatalog.extra` (`ManifestExtra { name, is_required }`) and `MetaPreview`. A catalog entry without an id, type or name is skipped; the rest of the row still shows.
- `Core` reloads the home rows by itself after every finished disk job: the first load at startup, an install and a removal. That retries `Failed` rows too. So the desktop never has to guess when the list is ready (at startup `State.addons` is still empty). `Command::LoadHome` forces a reload; nothing in the desktop sends it yet because the ticket adds no refresh control.
- Each reload bumps a generation number; answers from an earlier generation are dropped, so a slow fetch from before a removal cannot touch the new rows.
- A catalog row's name is the catalog's `name`, or its `id` when the manifest gives none.
- A catalog answer without a `metas` list is a `Failed` row; an `extra` entry with no name or a null `isRequired` is tolerated so it cannot make a manifest unreadable.
- Posters: `Core::poster` looks in a cache shared with the UI thread (`posters::PosterCache`); the first ask sends a message to the app task, which downloads and decodes in the background and calls `on_change`. A poster that fails to load stays `None` for the rest of the run and is not retried (otherwise the UI would retry every frame). An unreadable saved file is downloaded again.
- The desktop keeps one egui texture per poster URL and only asks for posters whose cell is visible.

**Verified**
- `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`: all pass (10 unit tests, 18 integration tests, 1 ignored). The extra tests written first were trimmed afterwards so the tests match this ticket's acceptance checks, as the spec's "Testing decisions" asks.
- Tests against `wiremock`: two catalogs go `Loading` then `Ready`; HTTP 500 gives one `Failed` row while the other is `Ready`; a required extra gives no row; the same poster requested twice makes one request and arrives scaled to 300 px wide; a new `Core` on the same data directory serves it with no request.
- `cargo run -p streem-desktop` with an empty `STREEM_DATA_DIR` on Windows 11: screenshot shows Cinemeta's rows ("Popular · movie", "Popular · series", "Featured · movie") with posters and names, and 31 files in `<data_dir>/posters`. The Home/Addons top bar was not clicked in the real window.
- `grep -rn "reqwest\|rusqlite\|std::fs" crates/desktop/src` prints nothing.
- Human check done by the user: vertical and horizontal scrolling stayed smooth while posters loaded, and the Home/Addons top bar worked. All posters loaded in their run; the failed-poster case was not seen.
