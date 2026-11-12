# 18: Per-profile history and library, synced

**What to build:** Each **profile** has its own continue-watching row and its own library. Switching **profile** switches both. A **profile** opened on a second device shows the same continue-watching positions and the same library.

**Blocked by:** 16, 10, 11

**Status:** ready-for-agent

## Read first

- `CONTEXT.md` (**Profile**)
- `.scratch/v1/spec.md`, sections "Account, profiles and overrides" and "Sync".
- `crates/core/src/history.rs`, `crates/core/src/library.rs`, `crates/core/src/profiles.rs`
- `crates/core/src/backend/sync.rs`, `crates/core/src/store.rs`, `crates/core/src/app.rs`
- `supabase/migrations/0001_init.sql`

## Files

- Edit: `crates/core/src/history.rs`, `crates/core/src/library.rs`, `crates/core/src/backend/sync.rs`, `crates/core/src/store.rs`, `crates/core/src/app.rs`
- No new files. No changes in `crates/desktop` are expected.

## What to do

- Add a store migration that deletes every `watch_progress` and `library_items` row whose `profile_id` is `local`. That is development data from before **profiles** existed.
- Every write to those two tables uses the active **profile**'s id as `profile_id` and the signed-in user id as `account_id`.
- Every read is filtered by the active **profile**. With no active **profile**, `State.continue_watching` and `State.library` are empty and nothing is saved.
- On `SelectProfile` and `SwitchProfile`, rebuild `State.continue_watching` and `State.library`.
- `Details.in_library` and the resume position used by `Play` both look only at the active **profile**'s rows.
- Register `watch_progress` and `library_items` in `SYNC_TABLES`, after the existing entries. If a local column name or type differs from the remote one, add a store migration that makes them match.
- When a pull changes either table, rebuild the matching part of `State`, but only if the changed rows belong to the active **profile**.
- If a pull moves the saved position of the video that is playing right now, do not seek. The local player wins while it is playing, and its next save will be the most recent change.
- The same **profile** used on two devices at once: the most recent save wins. This already follows from the engine; add a test for it.
- Rows of a deleted **profile** stay in the tables. Add the comment `// shortcut: rows of deleted profiles are kept, purge them if storage grows`.

## Out of scope

**Overrides** (ticket 19), "because you watched" rows, a history screen, merging two **profiles**.

## Acceptance checks

- [ ] `cargo test -p streem-core` passes, including tests against a `wiremock` server standing in for Supabase that: progress saved under **profile** A is not in `continue_watching` after selecting **profile** B; a library item added under A is not in B's library, and `in_library` is false for B; with no active **profile** both lists are empty; a save pushes a `watch_progress` row carrying the active **profile**'s id with `on_conflict=profile_id,video_id`; a pulled row for the active **profile** appears in `continue_watching`; a pulled row for another **profile** does not change `State`; a pulled row with a newer `updated_at` replaces the local position and an older one does not; `Play` resumes from a position that arrived by sync; a pull during playback of the same video causes no seek; the migration removes `local` rows.
- [ ] `cargo run -p streem-desktop` twice with two `STREEM_DATA_DIR` values on one **account**: watch 30 seconds as **profile** A in one copy, select A in the other, and the title is in "Continue watching" and resumes; select B and it is absent. Repeat for a library item.

## Rules

- Use the terms defined in `CONTEXT.md` in names, messages and comments.
- Add crates with `cargo add`. Never type a version number from memory.
- `streem-desktop` does no network or disk work. It sends a `Command` to `Core` and draws `State`.
- Do not install system packages. If something is missing, stop and write what is missing under `## Comments` in this file.
- Do only what this ticket lists. Anything under "Out of scope" belongs to another ticket.
- Finish with `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace`. All three must pass.
- When done, tick the boxes and add a line under `## Comments` saying what you verified and how.

## Comments
