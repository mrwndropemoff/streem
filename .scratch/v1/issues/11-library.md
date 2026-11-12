# 11: Library

**What to build:** The details screen gets an "Add to library" button that becomes "Remove from library" once added. A Library screen, reachable from the top bar, shows everything in the library as a poster grid. It survives a restart.

**Blocked by:** 05

**Status:** ready-for-agent

## Read first

- `CONTEXT.md`
- `crates/core/src/lib.rs`, `crates/core/src/store.rs`, `crates/core/src/app.rs`
- `crates/desktop/src/main.rs`, `crates/desktop/src/screens/details.rs`, `crates/desktop/src/screens/home.rs`

## Files

- New: `crates/core/src/library.rs`
- Edit: `crates/core/src/lib.rs`, `crates/core/src/store.rs`, `crates/core/src/app.rs`
- New: `crates/desktop/src/screens/library.rs`
- Edit: `crates/desktop/src/main.rs`, `crates/desktop/src/screens/mod.rs`, `crates/desktop/src/screens/details.rs`

## Additions to the interface

```rust
pub struct LibraryItem { pub meta_type: String, pub meta_id: String, pub name: String, pub poster: Option<String> }

// State
pub library: Vec<LibraryItem>,

// Details
pub in_library: bool,

// Command
AddToLibrary,                          // adds the title currently in State.details
RemoveFromLibrary { meta_id: String },
```

## Storage

Add a migration to `store.rs` creating table `library_items`:

`account_id TEXT`, `profile_id TEXT`, `meta_id TEXT`, `meta_type TEXT`, `name TEXT`, `poster TEXT`, `updated_at INTEGER` (milliseconds since 1970), `deleted INTEGER`, `dirty INTEGER`, primary key `(profile_id, meta_id)`.

Until sign-in and **profiles** exist, `account_id` and `profile_id` are both the text `local`. Removing sets `deleted = 1`; adding again sets it back to 0. Every write sets `dirty = 1` and a fresh `updated_at`. Sync uses these columns in ticket 18.

## Behaviour (`library.rs`)

- `AddToLibrary` does nothing unless `State.details` holds a `Ready` meta.
- `State.library` holds the non-deleted rows, most recently added first, and is loaded from the store at startup.
- `Details.in_library` is kept correct when details open and after every add or remove.

## Screen

- `screens/library.rs`: a wrapping grid of posters with names, vertically scrollable, loading posters only when visible, as on the home screen. Clicking one opens its details. When empty, show "Your library is empty".
- Add "Library" to the top bar.
- The button on the details screen toggles between the two labels according to `in_library`.

## Out of scope

Sync, per-profile separation, sorting and filtering, a library row on the home screen.

## Acceptance checks

- [x] `cargo test -p streem-core` passes, including tests that: `AddToLibrary` with ready details adds the item and sets `in_library`; with no details it changes nothing; `RemoveFromLibrary` removes it; add, remove, add leaves exactly one item; a new `Core` on the same data directory still lists the item; opening details of a title already in the library has `in_library == true`.
- [ ] `cargo run -p streem-desktop`: add a title, see it on the Library screen, restart the app, see it still there, remove it.

## Rules

- Use the terms defined in `CONTEXT.md` in names, messages and comments.
- Add crates with `cargo add`. Never type a version number from memory.
- `streem-desktop` does no network or disk work. It sends a `Command` to `Core` and draws `State`.
- Do not install system packages. If something is missing, stop and write what is missing under `## Comments` in this file.
- Do only what this ticket lists. Anything under "Out of scope" belongs to another ticket.
- Finish with `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace`. All three must pass.
- When done, tick the boxes and add a line under `## Comments` saying what you verified and how.

## Comments

- Verified with `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace`, all passing. `crates/core/tests/core.rs` has a black-box test for every acceptance case (add with ready details, add with no details, remove, add–remove–add leaves one item, a new `Core` on the same data directory, opening a title already in the library). It also tests adding while the details `Failed`, newest-first order, that a library change does not reload the home rows, and that a change sent just before dropping `Core` is saved. `store.rs` unit tests check `local`/`local`, `deleted`, `dirty` and `updated_at`. Mutation checks: removing the `in_library` update on open, or the library handling in the shutdown drain, makes the matching test fail.
- Not verified: the `cargo run -p streem-desktop` click-through. The agent could not drive the GUI, so that box is left for a person to tick.
- For ticket 18: `State.library` is ordered by `updated_at DESC`, which is "most recently added" only while adding is the only write that touches a row that is not deleted. Library writes keep `updated_at` strictly increasing, so two adds in the same millisecond still keep their order. If sync rewrites `updated_at` when it pulls, the order becomes "most recently changed".
- `AddToLibrary` for a title already in the library saves it again, which moves it to the front. The details button never sends it in that case.
