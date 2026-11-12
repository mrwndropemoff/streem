# 21: Join a watch party on the same file

**What to build:** The home-screen banner for a running **watch party** gets a "Join" button. Pressing it opens the player on the same file the party is watching, at the party's current position, playing or paused as the party is. The joiner fetches that file through their own **profile**'s Real-Debrid key; no link is ever passed between devices.

**Blocked by:** 20, 19

**Status:** ready-for-agent

## Read first

- `CONTEXT.md` (**Watch Party**, **Override**)
- `.scratch/v1/spec.md`, sections "Watch party" and "Open risks" (the item on matching the same file).
- `crates/core/src/party.rs`, `crates/core/src/backend/realtime.rs`, `crates/core/src/playback.rs`, `crates/core/src/addon/client.rs`, `crates/core/src/addon/url.rs`, `crates/core/src/app.rs`
- `crates/desktop/src/main.rs`, `crates/desktop/src/screens/home.rs`

## Files

- Edit: `crates/core/src/lib.rs`, `crates/core/src/party.rs`, `crates/core/src/backend/realtime.rs`, `crates/core/src/playback.rs`, `crates/core/src/app.rs`
- Edit: `crates/desktop/src/main.rs`, `crates/desktop/src/screens/home.rs`

## Additions to the interface

```rust
// Command
JoinParty { party_id: String },

// ActiveParty
pub joining: bool,     // true from JoinParty until playback has started
```

## Beacons

Every device in a **watch party** broadcasts, on the **account** channel, the event `beacon` every 2 seconds:

```json
{ "party_id": "...", "device_id": "...", "position_secs": 873.4, "paused": false }
```

- The values come from `State.playback`. Send nothing while not in a party.
- A device keeps the latest beacon per other member, with the moment it arrived (from a monotonic clock passed in, never read inside `party.rs`).
- A member's estimated position is its beacon position, plus the time since the beacon arrived if it was not paused.
- A beacon older than 6 seconds is ignored.

## Finding the same file

The party's presence block (ticket 20) names `meta_type`, `video_id`, `addon_id`, `filename` and `stream_name`. To join:

1. Find the installed **addon** whose manifest id equals `addon_id`. If it is missing: notice "This watch party uses an addon that is not installed." and stop.
2. Request the streams for `video_id` from that **addon**, through `addon::url::resolve` with this **profile**'s resolved settings. If the **addon** is skipped for this **profile**: notice "This profile needs a Real-Debrid key to join." and stop.
3. Choose the stream, in this order:
   - the playable stream whose `filename` equals the party's `filename`, when the party's is not empty;
   - otherwise the playable stream whose `name` equals `stream_name`, if exactly one has it.
4. If nothing matches: notice "Could not find the same file for this profile." and stop.

A wrong match would put two people on different cuts of a film, so never fall back to "the first stream".

## Starting at the party's position

1. Wait up to 5 seconds for a beacon from any member of that party. If none arrives: notice "The watch party is not responding." and stop.
2. Start playback of the chosen stream with `seek` set to that member's estimated position and `paused` copied from the beacon. `Playback.title` and `poster` come from the presence block.
3. The saved resume position from watch history is ignored when joining.
4. Update presence with the same `party` block, so this device becomes a member, and start sending beacons.
5. Every "stop" above leaves `State.party` as `None` and `State.playback` untouched.

Watch history is recorded for the joiner's own **profile** as for any playback.

## Screens

- `home.rs`: a "Join" button on each banner. While `joining`, show a spinner on it.
- `main.rs`: the player screen shows whenever `State.playback` is set, whichever screen the person was on, and closing it returns to that screen.

## Out of scope

Play, pause and seek applying to everyone, and catching up (ticket 22). Until then a joiner starts in the right place and may drift.

## Acceptance checks

- [ ] `cargo test -p streem-core` passes, including tests of the matching function that: an equal `filename` wins even when another stream shares the `name`; with an empty party `filename`, a unique equal `name` matches; two streams with that `name` and no filename match give no match; an unplayable stream is never chosen.
- [ ] Tests of the join flow, with a `wiremock` **addon** and a fake realtime transport, that: a successful join sets `playback` with the joiner's own link (not anything from presence), `seek` at the beacon's estimated position and the beacon's paused state; a paused beacon gives a position that does not advance with time; no beacon within 5 seconds gives the "not responding" notice and no playback; a missing **addon**, a skipped **addon** and an unmatched file each give their notice; after joining, the outgoing presence carries the `party` block; the stream request used the joiner's resolved key.
- [ ] A test that beacons are sent every 2 seconds only while in a party, using an injected clock.
- [ ] `cargo run -p streem-desktop` twice with two `STREEM_DATA_DIR` values, same **account**, two **profiles**: start a **watch party** in one, press Join in the other, and the same title opens within a few seconds at about the same position.

## Rules

- Use the terms defined in `CONTEXT.md` in names, messages and comments.
- Add crates with `cargo add`. Never type a version number from memory.
- `streem-desktop` does no network or disk work. It sends a `Command` to `Core` and draws `State`.
- Do not install system packages. If something is missing, stop and write what is missing under `## Comments` in this file.
- Do only what this ticket lists. Anything under "Out of scope" belongs to another ticket.
- Finish with `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace`. All three must pass.
- When done, tick the boxes and add a line under `## Comments` saying what you verified and how.

## Comments
