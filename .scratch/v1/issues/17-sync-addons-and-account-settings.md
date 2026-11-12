# 17: Sync the addon list and account settings

**What to build:** The **account**'s **addon** list and its settings (Real-Debrid key, audio language, subtitle language) follow the **account**. Install an **addon** or change a setting on one device, and the other devices signed in to that **account** have it shortly after.

**Blocked by:** 16, 12

**Status:** ready-for-agent

## Read first

- `CONTEXT.md` (**Account**, **Addon**)
- `.scratch/v1/spec.md`, section "Sync".
- `crates/core/src/backend/sync.rs` (the engine and `SYNC_TABLES`)
- `crates/core/src/settings.rs`, `crates/core/src/addon/url.rs`, `crates/core/src/store.rs`, `crates/core/src/app.rs`
- `supabase/migrations/0001_init.sql`

## Files

- Edit: `crates/core/src/backend/sync.rs`, `crates/core/src/settings.rs`, `crates/core/src/store.rs`, `crates/core/src/app.rs`
- No new files. No changes in `crates/desktop` are expected.

## What to do

- Register `addons` and `account_settings` in `SYNC_TABLES`, after `profiles`, with the columns from `supabase/migrations/0001_init.sql`. If a local column name or type differs from the remote one, add a store migration that makes them match.
- `account_settings` now has one row per **account**, keyed by the signed-in user id. If the local table still has a `local` row, drop it in the same migration.
- When a pull changes `addons`: rebuild `State.addons` from the store (the synced `manifest_json` means no manifest request is needed) and reload the home screen.
- When a pull changes `account_settings`: rebuild `State.settings`, then do what `SetRealDebridKey` already does after a key change (clear `skipped_reason`, re-fetch placeholder **addon** manifests, reload the home screen).
- **Cinemeta on a new account.** Replace the "install Cinemeta when there are no **addons**" rule with: after the first successful pull of `addons` for this **account**, if the table has no rows at all for it (not even deleted ones), install Cinemeta. This stops a second device from re-adding it after someone removed it, and stops two devices from racing.
- The stored **addon** URL is the template with `{real_debrid_key}`. The resolved URL, with a key in it, must never be written to the `addons` table or sent to the backend.
- `account_settings` is one row, so two devices changing two different settings at the same moment keep only the later change. Add the comment `// shortcut: account settings sync as one row, split per setting if simultaneous edits get lost`.

## Out of scope

Per-**profile** **overrides** (ticket 19), history and library (ticket 18), reordering **addons** in the UI.

## Acceptance checks

- [ ] `cargo test -p streem-core` passes, including tests against a `wiremock` server standing in for Supabase that: installing an **addon** pushes a row to `addons` with the composite `on_conflict=account_id,url`; a pulled **addon** row appears in `State.addons` without any request to the **addon** itself; a pulled row with `deleted = true` removes it; `SetRealDebridKey` pushes `account_settings`; a pulled `account_settings` row with a key updates `State.settings` and clears a placeholder **addon**'s `skipped_reason`; a first pull returning zero **addon** rows installs Cinemeta; a first pull returning only a deleted Cinemeta row does not.
- [ ] A test installs a placeholder **addon** with a key set and asserts that no pushed request body contains the key in the `url` field.
- [ ] `cargo run -p streem-desktop` twice with two `STREEM_DATA_DIR` values on one **account**: an **addon** installed in one appears in the other; a Real-Debrid key saved in one appears in the other's Settings; removing an **addon** in one removes it in the other.

## Rules

- Use the terms defined in `CONTEXT.md` in names, messages and comments.
- Add crates with `cargo add`. Never type a version number from memory.
- `streem-desktop` does no network or disk work. It sends a `Command` to `Core` and draws `State`.
- Do not install system packages. If something is missing, stop and write what is missing under `## Comments` in this file.
- Do only what this ticket lists. Anything under "Out of scope" belongs to another ticket.
- Finish with `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace`. All three must pass.
- When done, tick the boxes and add a line under `## Comments` saying what you verified and how.

## Comments
