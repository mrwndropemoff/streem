# 20: Start a watch party and see who is in it

**What to build:** While a title is playing, a **profile** can start a **watch party**. On every other device signed in to the same **account**, the home screen shows that it is running ("Sam is watching Dune") and who is in it. The person who started it sees the member list in the player and can leave. Joining comes in ticket 21.

**Blocked by:** 18

**Status:** ready-for-agent

## Read first

- `CONTEXT.md` (**Watch Party**, **Profile**, **Account**)
- `.scratch/v1/spec.md`, section "Watch party".
- `docs/adr/0001-hosted-supabase-with-local-copy.md`
- `crates/core/src/backend/auth.rs`, `crates/core/src/playback.rs`, `crates/core/src/profiles.rs`, `crates/core/src/app.rs`
- `crates/desktop/src/screens/home.rs`, `crates/desktop/src/screens/player.rs`
- Supabase documentation: Realtime → Protocol, Realtime → Presence, Realtime → Authorization. The message shapes below are a summary; the documentation is the authority.

## Files

- New: `crates/core/src/backend/realtime.rs` (the websocket client)
- New: `crates/core/src/party.rs` (the rules; no networking in this file)
- Edit: `crates/core/src/lib.rs`, `crates/core/src/app.rs`, `crates/core/src/backend/mod.rs`, `crates/core/src/store.rs`
- New: `supabase/migrations/0002_realtime.sql`
- Edit: `crates/desktop/src/screens/home.rs`, `crates/desktop/src/screens/player.rs`

## Crates

- `streem-core`: `tokio-tungstenite` with a rustls TLS feature, `futures-util`.

## Additions to the interface

```rust
pub struct PartySummary { pub party_id: String, pub title: String, pub members: Vec<String> }   // profile names
pub struct ActiveParty { pub party_id: String, pub members: Vec<String> }

// State
pub parties: Vec<PartySummary>,     // running in this account, not counting the one this device is in
pub party: Option<ActiveParty>,     // the one this device is in

// Command
StartParty,
LeaveParty,
```

## How devices find each other

- Every signed-in device with an active **profile** keeps one websocket open to Supabase Realtime and joins one channel named `account:{account id}`.
- The channel is **private**: only that **account** may join it. `supabase/migrations/0002_realtime.sql` adds the policies on `realtime.messages` that allow an authenticated user to read and write only the topic `account:` followed by their own user id. Follow the Realtime Authorization documentation for the exact policy.
- Each device announces itself with Presence, keyed by its device id. The payload:

  ```json
  {
    "device_id": "...", "profile_id": "...", "profile_name": "Sam",
    "party": null
  }
  ```

  and, when in a **watch party**:

  ```json
  "party": {
    "party_id": "...", "meta_type": "movie", "meta_id": "tt...", "video_id": "tt...",
    "title": "Dune", "poster": "https://...", "addon_id": "...",
    "filename": "Dune.2021.2160p.mkv", "stream_name": "..."
  }
  ```

- The payload describes which file is playing. It must never contain a stream link or a Real-Debrid key.
- A **watch party** has no row in any database. It exists while at least one device's presence carries its `party_id`, and its members are exactly those devices.
- The device id is a uuid created once and kept in `local_state` under `device_id`.

## Realtime in brief

- Connect to `wss://{project host}/realtime/v1/websocket?apikey={key}&vsn=1.0.0`.
- Join by sending `phx_join` on topic `realtime:account:{account id}` with a payload carrying the channel config (`private: true`, presence key = device id, broadcast `self: false`) and the current `access_token`.
- Send a `heartbeat` on topic `phoenix` every 25 seconds.
- Announce and update presence with a `presence` event of type `track`. Read the others from `presence_state` and `presence_diff`.
- When the access token is refreshed, send the new one on the channel.
- On disconnect, reconnect after 1, 2, 4, then up to 30 seconds, and track presence again. While disconnected, `parties` is empty and `party.members` shows only this device's **profile**.

## Rules (`party.rs`)

Write these as plain functions over values, taking presence snapshots and commands in and giving the new state and the presence payload to send out. No sockets, no clock reads.

- `StartParty` works only while `State.playback` is set and this device is not already in a **watch party**. It creates a new uuid, fills the `party` block from `Playback`, and updates presence.
- `LeaveParty` sets `party` back to `null`.
- Leaving also happens automatically on: `StopPlayback`, the video ending or failing, starting a different video, `SwitchProfile`, `SignOut`.
- `State.party.members` and each `PartySummary.members` are the **profile** names of the devices carrying that `party_id`, sorted.
- `State.parties` lists the other parties in the channel, newest first.

## Screens

- `home.rs`: above the first row, one banner per entry in `State.parties`: "Sam is watching Dune" for one member, "Sam and Alex are watching Dune" for two, "Sam and 2 others are watching Dune" beyond that. No button yet.
- `player.rs`: a "Start watch party" button in the control bar. Once in one, it becomes a small panel listing the members with a "Leave" button.

## Human step inside this ticket

A person must run `supabase/migrations/0002_realtime.sql` in the Supabase SQL editor before the manual check below can pass. Say so under `## Comments` when the code is ready.

## Out of scope

Joining (ticket 21), synced controls (ticket 22), chat, invites, parties across **accounts**.

## Acceptance checks

- [ ] `cargo test -p streem-core` passes, including tests of `party.rs` that: `StartParty` without playback does nothing; with playback it produces a presence payload with a `party` block copied from `Playback`; the payload, serialised, contains neither the stream link nor the Real-Debrid key; a presence snapshot with two other devices sharing a `party_id` yields one `PartySummary` with both names; the device's own party is in `State.party` and not in `State.parties`; a device leaving presence removes it from the member list; the last member leaving removes the party; `StopPlayback`, `SwitchProfile` and starting another video each clear the party.
- [ ] One test starts a local websocket server with `tokio-tungstenite`, points `realtime.rs` at it, and asserts: the join message has the topic `realtime:account:{id}`, `private` set to true and an access token; a heartbeat follows; after the server drops the connection the client reconnects and joins again.
- [ ] `cargo run -p streem-desktop` twice with two `STREEM_DATA_DIR` values, same **account**, two different **profiles**: start a **watch party** in one; the other's home screen shows the banner within a few seconds; leaving or closing the first copy removes it.

## Rules

- Use the terms defined in `CONTEXT.md` in names, messages and comments.
- Add crates with `cargo add`. Never type a version number from memory.
- `streem-desktop` does no network or disk work. It sends a `Command` to `Core` and draws `State`.
- Do not install system packages. If something is missing, stop and write what is missing under `## Comments` in this file.
- Do only what this ticket lists. Anything under "Out of scope" belongs to another ticket.
- Finish with `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace`. All three must pass.
- When done, tick the boxes and add a line under `## Comments` saying what you verified and how.

## Comments
