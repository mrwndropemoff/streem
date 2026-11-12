# 16: Profiles and the sync engine

**What to build:** After signing in, the person sees a **profile** picker: choose a **profile**, or create, rename or delete one. A **profile** created on one device shows up on another device signed in to the same **account**. When the backend cannot be reached, the app says it is offline and keeps working from its local copy.

This ticket builds the sync engine that tickets 17 and 18 reuse, and proves it with one table, `profiles`.

**Blocked by:** 15

**Status:** ready-for-agent

## Read first

- `CONTEXT.md` (**Account**, **Profile**)
- `docs/adr/0001-hosted-supabase-with-local-copy.md`
- `supabase/migrations/0001_init.sql`
- `crates/core/src/backend/auth.rs`, `crates/core/src/store.rs`, `crates/core/src/app.rs`
- `crates/desktop/src/main.rs`

## Files

- New: `crates/core/src/backend/rest.rs`, `crates/core/src/backend/sync.rs`
- New: `crates/core/src/profiles.rs`
- Edit: `crates/core/src/lib.rs`, `crates/core/src/app.rs`, `crates/core/src/store.rs`, `crates/core/src/backend/mod.rs`
- New: `crates/desktop/src/screens/profiles.rs`
- Edit: `crates/desktop/src/main.rs`, `crates/desktop/src/screens/mod.rs`

## Crates

- `streem-core`: `uuid` with the v4 feature.

## Additions to the interface

```rust
pub struct Profile { pub id: String, pub name: String }

// State
pub profiles: Vec<Profile>,
pub active_profile: Option<String>,   // a profile id
pub offline: bool,

// Command
CreateProfile { name: String },
RenameProfile { id: String, name: String },
DeleteProfile { id: String },
SelectProfile { id: String },
SwitchProfile,      // back to the picker
SyncNow,
```

## Local table

Add a migration creating `profiles`: `id TEXT PRIMARY KEY`, `account_id TEXT`, `name TEXT`, `real_debrid_key TEXT`, `audio_lang TEXT`, `subtitle_lang TEXT`, `updated_at INTEGER`, `deleted INTEGER`, `dirty INTEGER`. Also `sync_cursor(table_name TEXT PRIMARY KEY, cursor TEXT)`.

The local columns have the same names as the remote ones, plus `dirty`. That is what lets one engine sync every table.

## The sync engine (`backend/sync.rs`)

Describe each synced table with data, not code:

```rust
pub struct SyncTable {
    pub name: &'static str,
    pub key: &'static [&'static str],        // primary key columns
    pub columns: &'static [(&'static str, Kind)],   // Kind: Text, Integer, Real, Bool
}
pub const SYNC_TABLES: &[SyncTable] = &[ /* this ticket registers only `profiles` */ ];
```

Rows travel as JSON objects. `Kind::Bool` is `0/1` locally and `true/false` remotely. `dirty` is never sent.

One sync pass, per table, in the order of `SYNC_TABLES`:

1. **Push.** Read the local rows with `dirty = 1`. Send them as a JSON array: `POST {url}/rest/v1/{table}?on_conflict={key columns, comma separated}` with headers `apikey`, `Authorization: Bearer <access token>`, `Content-Type: application/json`, `Prefer: resolution=merge-duplicates,return=minimal`. On success, clear `dirty` only for rows whose `updated_at` still equals the value that was sent.
2. **Pull.** `GET {url}/rest/v1/{table}?select=*&order=server_updated_at.asc&limit=1000`, adding `&server_updated_at=gte.{cursor}` when a cursor is stored. For each row: write it locally with `dirty = 0` if there is no local row or the remote `updated_at` is greater; otherwise leave the local row alone. Store the largest `server_updated_at` seen as the new cursor. Repeat the pull while a full page of 1000 came back.
3. If the pull changed anything, reload the affected part of `State` from the store.

Rules:

- The server drops stale writes by itself (the trigger from ticket 13), so the most recent change wins on both sides.
- Add the comment `// shortcut: "most recent" compares device clocks, switch to server time if devices with wrong clocks cause lost changes`.
- A pass runs: on sign-in, on `SyncNow`, every 20 seconds, and 1 second after any local write (several writes within that second cause one pass). Never run two passes at once.
- HTTP 401: refresh the token once and retry the request.
- A network failure or a 5xx answer sets `State.offline = true` and ends the pass. Any successful request sets it to `false`.
- Local writes always succeed and never wait for sync.
- Check the request shapes against the current Supabase (PostgREST) documentation before relying on them.

## Profiles (`profiles.rs`)

- Create: name trimmed, 1 to 30 characters, a fresh uuid as id. Rename: same name rule. Delete: set `deleted = 1`.
- `State.profiles` holds the non-deleted rows of this **account**, ordered by name.
- `SelectProfile` sets `active_profile`. `SwitchProfile` clears it. Deleting the active **profile**, or learning through sync that it was deleted elsewhere, also clears it.
- The active **profile** is not remembered between launches: the picker shows every time, as on a TV streaming app.
- There is no limit on the number of **profiles**.

## Screens

- `screens/profiles.rs`: a centred row of large tiles, one per **profile**, each showing the first letter and the name, plus an "Add profile" tile. A "Manage" toggle reveals rename and delete on each tile; delete asks for confirmation.
- `main.rs`: signed in with no `active_profile` shows only the picker. Add "Switch profile" next to the active **profile**'s name in the top bar.
- When `State.offline` is true, show a thin bar on every screen: "Offline. Changes will sync when the connection returns."

## Out of scope

Syncing **addons**, settings, history or library (tickets 17 and 18); **overrides** (ticket 19); **profile** pictures; PINs.

## Acceptance checks

- [ ] `cargo test -p streem-core` passes, including tests against a `wiremock` server standing in for Supabase that: creating a **profile** sends one upsert with the headers above and clears `dirty`; a row edited locally while its push is in flight stays dirty; a pulled row that is new appears in `State.profiles`; a pulled row older than the local one is ignored; a pulled row newer than the local one replaces it; a pulled row with `deleted = true` disappears from `State.profiles`; the second pull sends the stored cursor; a connection failure sets `offline` and a later success clears it; a 401 triggers one refresh and one retry; three quick local writes cause one pass; deleting the active **profile** clears `active_profile`; an empty name and a 31-character name are rejected.
- [ ] `cargo run -p streem-desktop` twice at once with two different `STREEM_DATA_DIR` values, both signed in to the same **account**: a **profile** created in one appears in the other within about 20 seconds, and so do a rename and a delete.
- [ ] With the network disconnected: the offline bar shows, creating a **profile** still works, and it reaches the other copy after reconnecting.

## Rules

- Use the terms defined in `CONTEXT.md` in names, messages and comments.
- Add crates with `cargo add`. Never type a version number from memory.
- `streem-desktop` does no network or disk work. It sends a `Command` to `Core` and draws `State`.
- Do not install system packages. If something is missing, stop and write what is missing under `## Comments` in this file.
- Do only what this ticket lists. Anything under "Out of scope" belongs to another ticket.
- Finish with `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace`. All three must pass.
- When done, tick the boxes and add a line under `## Comments` saying what you verified and how.

## Comments
