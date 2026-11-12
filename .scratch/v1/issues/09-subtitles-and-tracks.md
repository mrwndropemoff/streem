# 09: Subtitles from addons and track selection

**What to build:** While a video plays, the player has an audio menu and a subtitle menu. The subtitle menu lists the tracks inside the file and the subtitles supplied by **addons**, plus "Off". Choosing one switches immediately.

**Blocked by:** 07

**Status:** ready-for-agent

## Read first

- `CONTEXT.md`
- `crates/core/src/playback.rs`, `crates/core/src/app.rs`, `crates/core/src/addon/client.rs`
- `crates/desktop/src/mpv_view.rs`, `crates/desktop/src/screens/player.rs`
- `prototypes/egui-mpv/src/main.rs`, for how the prototype listed and switched tracks.

## Files

- Edit: `crates/core/src/lib.rs`, `crates/core/src/playback.rs`, `crates/core/src/app.rs`, `crates/core/src/addon/client.rs`, `crates/core/src/addon/types.rs`
- Edit: `crates/desktop/src/mpv_view.rs`, `crates/desktop/src/screens/player.rs`

## Additions to the interface

```rust
pub struct SubtitleTrack { pub lang: String, pub url: String, pub addon_name: String }

// Playback
pub subtitles: Vec<SubtitleTrack>,
```

## Addon protocol

- A subtitles request is `GET {base}/subtitles/{type}/{videoId}.json`, answering `{"subtitles": [{"id", "url", "lang"}]}`.
- When `Play` starts a video, ask every installed **addon** whose manifest offers the `subtitles` resource for this type and id prefix, all at once, in the background. Playback must not wait for the answers.
- As each answer arrives, append its tracks to `Playback.subtitles`, but only if that same video is still playing.
- A failing subtitles **addon** is ignored silently. It must never stop playback or raise a notice.

## Behaviour in `streem-desktop`

- Track switching is local to the player: `mpv_view.rs` may read mpv's track list and set its audio and subtitle track directly. This is the one kind of player control that does not go through a command.
- When a new entry appears in `Playback.subtitles`, add it to mpv as an external subtitle without selecting it, labelled with its language.
- The audio menu lists mpv's audio tracks by language and title. The subtitle menu lists "Off", the embedded tracks, then the **addon** tracks labelled "language · addon name".
- Both menus sit in the player's control bar.

## Out of scope

Preferred languages (ticket 12), subtitle styling, subtitle delay.

## Acceptance checks

- [x] `cargo test -p streem-core` passes, including tests against a `wiremock` server that: `Play` returns a `playback` immediately while the subtitles request is still pending; tracks appear in `Playback.subtitles` afterwards; a subtitles **addon** answering HTTP 500 leaves `playback` intact and `notice` empty; tracks arriving after `StopPlayback` are discarded.
- [x] `cargo run -p streem-desktop` with the sample from `prototypes/egui-mpv/sample/` served over HTTP, or any stream with two audio tracks: both menus list the tracks.

## Human check

- [ ] Switching audio changes the sound; switching subtitles shows, changes and hides the text.
- [ ] With a subtitles **addon** installed, its tracks appear in the menu and display when chosen.

## Rules

- Use the terms defined in `CONTEXT.md` in names, messages and comments.
- Add crates with `cargo add`. Never type a version number from memory.
- `streem-desktop` does no network or disk work. It sends a `Command` to `Core` and draws `State`. mpv fetching the video and subtitle files itself is the one exception.
- Do not install system packages. If something is missing, stop and write what is missing under `## Comments` in this file.
- Do only what this ticket lists. Anything under "Out of scope" belongs to another ticket.
- Finish with `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace`. All three must pass.
- When done, tick the boxes and add a line under `## Comments` saying what you verified and how.

## Comments

- **Core.** `cargo test --workspace` passes, with 4 new wiremock tests in `crates/core/tests/core.rs`:
  - `Play` gives a `playback` with no subtitles while the subtitles answer is held back 600 ms. The tracks appear afterwards, and one request is made. A `Play` on an unplayable stream in between does not throw the tracks away.
  - An HTTP 500 is checked only after the server has received the request. `playback` stays and `notice` is empty.
  - Stop, then play again, while the first answer is still delayed. Only the second answer's tracks are kept.
  - An **addon** whose `idPrefixes` do not match is not asked.

  I broke the code twice on purpose to check that the tests can fail. Dropping the generation check made the stop test fail with `["new", "old"]`. Dropping the http(s) filter made the first test fail. Both were restored. `cargo fmt --all` and `cargo clippy --workspace --all-targets -- -D warnings` are clean.
- **Desktop, run by the agent on Windows 11.** Setup:
  - A throwaway Node server (not committed) stood in as an **addon** with catalog, meta, stream and subtitles. It served `prototypes/egui-mpv/sample/sample.mkv` with Range support and two `.srt` files (`eng`, `fre`), and answered the subtitles request after 2 s.
  - A temporary driver in `main.rs`, removed before committing, installed the **addon**, played the stream and logged `MpvView::tracks` once a second.

  What the log showed:
  - Playback started with 0 subtitles. The two **addon** tracks reached `Playback.subtitles` about 2 s later.
  - mpv's track list then read: audio `eng`, `fra`; subtitles `eng` (embedded), `eng · Local Check`, `fre · Local Check`. Adding them did not change `sid`.
  - Choosing the second audio track and then an **addon** subtitle, "Off", and the embedded track each changed `aid`/`sid` as expected.

  On the real window, with mouse clicks sent by a script and screenshots: both menus sit in the control bar. The subtitle menu lists "Off", "eng", "eng · Local Check" and "fre · Local Check". Clicking "fre · Local Check" showed "Ligne francaise 6" over the video at 00:25.
- **Not checked by the agent:** whether the sound changes (no audio was heard), the embedded subtitle's text on screen (no line was showing when I looked), a real subtitles **addon** such as OpenSubtitles, and Linux. The **Human check** boxes stay unticked.
- **Choices beyond the ticket text:**
  - Only `http(s)` subtitle links are kept, because mpv opens whatever it is given, local paths included. An entry without a `url` is skipped.
  - `Playback`'s hand-written `Debug` prints only the number of subtitles, not their links.
  - mpv's `sub-add` downloads the file before it returns, and libmpv2 has no async command. So **addon** subtitles are added on a small worker thread (`mpv-subtitles`) with the `auto` flag. A job meant for a file that is no longer loaded is dropped.
  - **Addon** tracks are told apart in mpv's track list by `external-filename`, which matched the URL exactly in the run above.
  - The bar stays visible while a menu is open.
  - `main.rs` is not in the Files list, but its `PlayerScreen::ui` call had to change: it passes the `MpvView` instead of a paint closure.
- **Known and left alone, after a two-axis review:**
  - If `Core` replaced `playback` with one that has the same link without a stop in between, `mpv_view` would miss the first new tracks. The person cannot do this by hand.
  - The menus re-read mpv's track list every frame while the bar is visible: about 6 property reads per track.
  - `fetch_subtitles` repeats the shape of `fetch_streams`.

  The review led to these changes: a wrong comment on `Message::Subtitles` was corrected, `file_serial` was renamed to `file_generation` and `AddSubtitle` to `SubtitleJob`, and the track menus return a `Pick` enum instead of `Option<Option<i64>>`. After those changes fmt, clippy and the workspace tests passed again; the desktop run above was done before them.
- **Checked after a second review: a `sub-add` still downloading when a new file loads does not land on it.** The review suspected it could, and the new file's menu would then list it as an embedded track.

  How it was checked, by the agent on Windows 11:
  - A throwaway Node **addon** (not committed) served the sample as two streams, A and B. The first subtitles answer pointed at an `.srt` that took 8 s to answer; later answers pointed at a fast one.
  - A temporary driver in `main.rs`, removed afterwards, played A, then B about 4 s later, logging `MpvView::tracks` once a second.
  - It was run both with `StopPlayback` before the second `Play` and with `Play` replacing A directly.

  In both runs B's subtitle was requested about 3 s before A's slow `.srt` was answered. The worker runs one job at a time, so mpv had already abandoned A's `sub-add` when the file changed. B's menu read `eng`, `bbb · Repro` until the end, and A's subtitle never appeared.

  A fix that removed such a track after the fact made no visible difference in the same runs, so it was not kept. One gap is left: the worker could check the generation, a new file could load, and only then would mpv start the `sub-add`. That gap is tiny and was not seen.
