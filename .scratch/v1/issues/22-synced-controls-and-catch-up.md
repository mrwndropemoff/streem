# 22: Synced play, pause, seek and catch-up

**What to build:** Inside a **watch party**, when anyone pauses, plays or seeks, it happens for everyone. If one person pauses at 14:33, everyone is paused at 14:33. When one device buffers, the others keep playing, and the device that fell behind jumps forward to rejoin them.

**Blocked by:** 21

**Status:** ready-for-agent

## Read first

- `CONTEXT.md` (**Watch Party**)
- `.scratch/v1/spec.md`, section "Watch party".
- `crates/core/src/party.rs`, `crates/core/src/playback.rs`, `crates/core/src/backend/realtime.rs`, `crates/core/src/app.rs`

## Files

- Edit: `crates/core/src/party.rs`, `crates/core/src/playback.rs`, `crates/core/src/app.rs`
- No new files. No changes in `crates/desktop` are expected: the player already obeys `State.playback`.

## Control messages

A device in a **watch party** broadcasts the event `control` on the **account** channel whenever its own user pauses, plays or seeks:

```json
{ "party_id": "...", "device_id": "...", "action": "pause", "position_secs": 873.4 }
```

`action` is `play`, `pause` or `seek`. `position_secs` is where the sender is (for `seek`, where it is going).

## Applying them

- `SetPaused` and `Seek` from the local user behave as before, and additionally broadcast a `control` when this device is in a party.
- A received `control` for this device's party is applied to `State.playback` through the same functions the local commands use, with a flag saying it came from the party, so that it is **not** broadcast again.
  - `pause`: set paused. If this device is more than 1 second away from the sender's position, also seek to it.
  - `play`: set playing.
  - `seek`: seek to the position; the paused state is unchanged.
- Ignore a `control` whose `party_id` is not this device's party, and any message carrying this device's own `device_id`.
- Anyone in the party may control it. There is no host.
- Send a beacon immediately after any control, sent or received, so estimates stay fresh.

## Catching up

Evaluate on every beacon received and on every `ReportPlayback`, using the estimated positions from ticket 21:

- Take the furthest-ahead estimate among the other members that are playing.
- If this device is playing, is not buffering, and that estimate is more than 2.0 seconds ahead of this device's position: seek to the estimate.
- Never jump backwards. The one who is behind moves; the one who is ahead does nothing. (If both sides jumped, they would swap places forever.)
- At most one catch-up seek every 5 seconds, so a weak connection does not turn into a loop of seeking and buffering.
- A catch-up seek is not broadcast as a `control`.
- Buffering on one device sends nothing and pauses nobody.

## What stays independent

- One member stopping playback or leaving does not stop the others.
- Each device reaches the end of the video on its own.
- Audio and subtitle choices are per device.

## Keep it testable

All of this lives in `party.rs` and `playback.rs` as plain functions: given the current state, an incoming event and the current time, return the new state and the messages to send. The clock is passed in. No test may sleep.

## Out of scope

Chat, reactions, voice, a host role, pausing everyone on buffering, parties across **accounts**.

## Acceptance checks

- [ ] `cargo test -p streem-core` passes, including tests that: a local `SetPaused(true)` in a party emits one `control` with `pause` and the current position, and none when not in a party; a received `pause` sets `paused` and, when 3 seconds away, a seek to the sender's position; when 0.5 seconds away, no seek; a received `play` clears `paused`; a received `seek` sets a new `SeekRequest` and keeps the paused state; a received `control` emits no `control` in response; a `control` for another party is ignored; a message with this device's id is ignored.
- [ ] Catch-up tests with an injected clock: another member 3 seconds ahead causes one seek to its estimate; 1.5 seconds ahead causes none; another member 3 seconds behind causes none; while this device is buffering, none; while this device is paused, none; a paused member ahead is not a target; two triggers 2 seconds apart cause one seek, and a third after 5 seconds causes another; a beacon older than 6 seconds is not a target; the catch-up seek emits no `control`.
- [ ] `cargo run -p streem-desktop` twice with two `STREEM_DATA_DIR` values, same **account**, two **profiles**, both in one **watch party**.

## Human check

- [ ] Pausing in either copy pauses both at the same position, within about a second.
- [ ] Playing in either copy resumes both.
- [ ] Seeking in either copy moves both.
- [ ] Pause one copy's network for ten seconds: the other keeps playing, and the first jumps forward when it recovers.
- [ ] Closing one copy leaves the other playing.

## Rules

- Use the terms defined in `CONTEXT.md` in names, messages and comments.
- Add crates with `cargo add`. Never type a version number from memory.
- `streem-desktop` does no network or disk work. It sends a `Command` to `Core` and draws `State`.
- Do not install system packages. If something is missing, stop and write what is missing under `## Comments` in this file.
- Do only what this ticket lists. Anything under "Out of scope" belongs to another ticket.
- Finish with `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace`. All three must pass.
- When done, tick the boxes and add a line under `## Comments` saying what you verified and how.

## Comments
