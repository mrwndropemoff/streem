# 01: Prototype: MKV in an egui window through libmpv (Linux)

**What to build:** A throwaway program that opens one window, plays a video file inside it through libmpv, and draws egui controls on top of the video. It exists to answer one question: can egui host libmpv on this machine (Arch Linux, Wayland, Hyprland, Intel graphics)? Nothing from it ships.

**Blocked by:** None (can start immediately)

**Status:** ready-for-human

## Prerequisites (a human sets these up; do not install them yourself)

Check each one first. If any fails, stop and write which one under `## Comments`.

- `cargo --version` prints a version.
- `pkg-config --exists mpv && echo ok` prints `ok`.
- `ffmpeg -version` prints a version.

## Read first

- `.scratch/v1/spec.md`, sections "Decisions" and "Build order" step 1.

## Files

- `.gitignore` at the repo root, containing `target/` and `prototypes/egui-mpv/sample/`. If the folder is not a git repository, run `git init` first.
- `prototypes/egui-mpv/Cargo.toml`: package name `egui-mpv-prototype`. Add an empty `[workspace]` table at the end so this folder never becomes part of the main workspace.
- `prototypes/egui-mpv/src/main.rs`: the whole program.
- `prototypes/egui-mpv/make-sample.sh`: creates `prototypes/egui-mpv/sample/sample.mkv` with ffmpeg: 60 seconds of test pattern video, two audio tracks tagged `eng` and `fra` (two different sine tones), and one SRT subtitle track with a line every 5 seconds.

## Details

- Use `eframe` with its OpenGL renderer (`glow`). Select the glow renderer explicitly in the native options; newer eframe versions default to a different one, and libmpv can only draw through OpenGL.
- mpv must draw into egui's own OpenGL context through libmpv's render API. Do not use mpv's `wid` option: it does not work on Wayland.
- Try approach A first. Move to B only if A cannot meet the acceptance checks.
  - **A:** the `egui_sharkplayer` crate (a libmpv video widget for egui).
  - **B:** the `libmpv2` crate with its render feature, drawing into a framebuffer from an `egui_glow` paint callback.
- The program takes one argument, a file path or an http(s) URL: `cargo run --release -- sample/sample.mkv`.
- Layout: the video fills the window. A bar at the bottom, drawn by egui over the video, has: a Play/Pause button, a seek slider from 0 to the duration, a `mm:ss / mm:ss` label, an audio track dropdown, and a subtitle track dropdown that includes "Off".
- Ask egui to repaint when mpv signals a new frame. Do not repaint continuously while paused.
- Once per second print one line to stdout: `time-pos=<seconds> paused=<bool> aid=<id> sid=<id>`.

## Out of scope

Addons, catalogs, a `core` crate, settings, Windows. Keep everything in the one `main.rs`.

## Acceptance checks

- [ ] `cd prototypes/egui-mpv && ./make-sample.sh` creates `sample/sample.mkv`; `ffprobe sample/sample.mkv` lists 1 video, 2 audio and 1 subtitle stream.
- [x] `cargo build --release` succeeds in `prototypes/egui-mpv`.
- [x] `timeout 20 cargo run --release -- sample/sample.mkv` is still running when the timeout ends (exit code 124) and printed `time-pos` lines whose values increase.
- [x] No panic and no mpv error line in the output.

## Human check (leave unticked; a person runs the program and looks)

- [x] The video is visible inside the window, not in a second window.
- [x] The control bar is drawn on top of the video and stays usable.
- [x] Play/Pause and the seek slider work.
- [x] Switching the audio track changes the tone; switching subtitles shows and hides the text.
- [x] Resizing the window and going fullscreen keep the picture correct.
- [x] CPU use is near zero while paused.

## When done

Under `## Comments` write: which approach worked (A or B), the exact crate versions from `Cargo.lock`, every problem hit and how it was solved, and anything Wayland-specific. Ticket 02 and ticket 07 read this. If neither approach works, write what failed and stop; do not start on another UI toolkit.

## Comments

**Blocked on prerequisites (2026-11-12). Nothing was built.**

Prerequisite check results:

- `cargo --version`: **FAILED.** `cargo: command not found`. There is no `cargo`, `rustc` or `rustup` on PATH, none in `/usr/bin`, and no `~/.cargo/bin`. A fish login shell does not find it either. A human needs to install a Rust toolchain (on Arch: `sudo pacman -S rustup && rustup default stable`), then set this ticket to `ready-for-agent` again.
- `pkg-config --exists mpv && echo ok`: passed (`ok`, libmpv 2.5.0).
- `ffmpeg -version`: passed (`n9.0.2`).

Session was Wayland (`wayland-1`), as the ticket assumes.

---

**Implemented (2026-11-12). Approach B works. Acceptance checks pass; the human checks are still open.**

The earlier prerequisite failure is resolved: `cargo 1.99.1` is installed now. `libmpv` 2.5.0 and `ffmpeg n9.0.2` were already there.

### Result

egui hosts libmpv on this machine (Arch, Wayland, Hyprland, Intel). mpv draws into egui's own GL context in a window with no second window, and the egui bar is drawn over the video.

**Approach A (`egui_sharkplayer` 0.5.1) was rejected after reading its source. It was not built.** It does expose `time_pos()` and `paused()`, so the literal "prints time-pos" check could be met. It cannot do what the Details section requires:

- `PlayerState` keeps its `mpv` handle private (`MpvContainer`) and offers no way to get or set a property, run a command, or read `track-list`. That rules out the audio and subtitle dropdowns and the `aid=`/`sid=` log line.
- It draws its own auto-hiding control bar. There is no hook to replace it with ours.
- It pins `eframe 0.35` and `libmpv2 6.0`, so it cannot share eframe 0.37 with anything else.

**Approach B was used:** `libmpv2` with its render API, drawn from an `egui_glow` paint callback. The whole program is `prototypes/egui-mpv/src/main.rs`, about 170 lines.

### Versions (from `Cargo.lock`)

| Crate | Version |
|---|---|
| eframe | 0.37.0 |
| egui / egui_glow | 0.37.0 |
| glow | 0.17.0 |
| glutin | 0.32.3 |
| winit | 0.30.13 |
| libmpv2 | 7.0.1 |
| libmpv2-sys | 4.0.1 |

rustc 1.99.1, edition 2024. eframe is built with `default-features = false, features = ["glow", "wayland", "x11", "default_fonts"]`.

### How it works (what ticket 02 and 07 need)

- **Renderer:** eframe 0.37's default features pull in `wgpu`, and `NativeOptions` has to say `renderer: eframe::Renderer::Glow`. Both are set.
- **Draw target:** inside the paint callback, mpv renders into whatever framebuffer egui has bound. The callback reads `GL_FRAMEBUFFER_BINDING` and passes it as the fbo, with flip-Y on and the width and height from `info.viewport_in_pixels()`. No offscreen framebuffer or texture is needed, because the video panel is the whole window. If a later screen draws the video in only part of the window, this needs rechecking: mpv sets its own viewport to `0,0,w,h`.
- **`Send`/`Sync`:** `libmpv2::render::RenderContext` holds a raw pointer, so it is neither, and egui's `CallbackFn` needs both. It is wrapped in a small struct with `unsafe impl Send + Sync`. That is sound only because the callback runs on the UI/GL thread.
- **Repaint:** `RenderContext::set_update_callback` calls `egui_ctx.request_repaint()`. Nothing else requests repaints, so nothing is drawn while paused.
- **Bar:** the video is a frameless `CentralPanel`. The bar is a bottom-anchored `egui::Area` with a translucent frame, so it overlays the video.
- **1 Hz log line:** it runs on its own thread, because `Mpv` is `Sync`. It still prints while paused, when egui is not repainting.
- **Tracks:** read from mpv's `track-list/N/{type,id,lang,title,selected}`. Selection sets `aid`/`sid` (`"no"` turns subtitles off).
- **mpv setup:** `vo=libmpv`, `keep-open=yes`, `terminal=yes` and `msg-level=all=warn`. libmpv prints nothing without `terminal=yes`, so the "no mpv error line" check means nothing without it.
- **Extra keys:** `F` toggles fullscreen and `Space` toggles pause. These were not in the ticket. They are there so the paused-CPU and fullscreen checks could be run with `hyprctl dispatch sendshortcut`. Pause uses mpv's `cycle pause` (atomic) for both the button and `Space`; `Space` only acts when no widget has focus, otherwise a focused button would toggle twice. `F` reads the real fullscreen state from egui's viewport, so a window-manager exit from fullscreen cannot desync it.

### Problems hit

1. **Approach A could not do the job.** See above.
2. **API drift in eframe 0.37.** `App::ui(&mut self, ui, frame)` is the required method, not `update`. `CentralPanel::show_inside` is deprecated in favour of `show`. `Id::new` is deprecated; `Id::unique("bar")` is the stable replacement (it hashes its source, so it is the same every frame). The build has no warnings.
3. **`make-sample.sh` first wrote `sample.srt` next to `sample.mkv`.** mpv auto-loads that file as a second, external subtitle track. The log showed `sid=2`, and the dropdown would have listed a duplicate. The script now builds the SRT in a temp file that is removed on exit.
4. **mpv `aid`/`sid` are strings** (`"no"` is possible), so the code reads them as `String`, not `i64`.

### Wayland and environment notes

- Session is Wayland (`wayland-1`). The `wid` option is not used. Rendering goes through glutin's EGL context. No Wayland-specific workaround was needed.
- `Cannot load libcuda.so.1` is printed to stderr once at startup. This mpv is built with CUDA interop, and the machine has no NVIDIA driver. It is harmless and is not an mpv log line.
- `Audio device underrun detected.` shows up intermittently at startup (3 of about 8 runs). It is an mpv warning, not an error, and it never appeared after playback was running. Not investigated further.
- **Colours:** the picture matches an ffmpeg-extracted frame in layout and geometry. Colours differ a little from a plain ffmpeg PNG (red is `255,24,0`, not `255,0,0`). The pixels in the egui window are identical to a frame dumped by the mpv CLI (`--vo=image`, sampled at three points), so the render path adds no colour error. The sample is untagged (`color_space=unknown`), so how each tool guesses the matrix is the likely cause; that was not checked further.

### Evidence gathered by the agent

- Acceptance run, final binary: exit 124, 19 `time-pos` lines, strictly increasing, no panic, no mpv log lines.
- Screenshot of the running window (`grim`, cropped to the window's `hyprctl` rect): video visible inside the one window, letterboxed, with the bar at the bottom reading `00:03 / 01:00`.
- Pause via `Space`: CPU fell from 27.3% to 0.0% (`top`, 4 s average), and the log line kept printing with `time-pos` frozen at 6.6 and `paused=true`.
- Fullscreen via `F`: the window went to 1920x1080, the picture filled it at 16:9, and the bar stayed on top.

- Normal window close (`hyprctl dispatch closewindow`): exit code 0, no extra output. The code never frees the render context in a controlled order (see shortcuts below), but nothing went wrong in practice.
- http URL argument: `python3 -m http.server` serving `sample/`, then `egui-mpv-prototype http://localhost:8765/sample.mkv` for 10 s: exit 124, `time-pos` lines printed. mpv warned `Stream is not seekable` because Python's server has no range support; that is the server, not the app.
- `cargo clippy --release` is clean.

### Not checked by the agent (left for the human check above)

The agent could not click, so these are untested: the Play/Pause button, dragging the seek slider, switching the audio tone, showing and hiding subtitles, and live-resizing the window. The `Space` pause and `F` fullscreen paths were exercised, but those go through different handlers than the button. The slider calls `seek <pos> absolute` on every `changed()`, so dragging sends one seek per frame; that may feel rough on a long file.

### Shortcuts left in the code (marked `shortcut:` in `main.rs`)

- `Mpv` is leaked with `Box::leak` so the render context can borrow it for `'static`. Process exit frees it; there is no clean shutdown. Fix this when the code ships (ticket 02).
- The seek slider sends a synchronous `seek` on every changed frame while dragging.
- The UI only repaints on mpv's render-update callback. Audio-only media or a still image would not refresh the time label or slider; add `request_repaint_after` before supporting that.
- The 1 Hz log thread has no stop signal and is never joined. It is a prototype-only aid.
- Track labels with no language and no title read `2: ` (trailing space). Bar widths are hard-coded (`w - 520`) and clip when track titles are long.
- The track list is re-read every frame. That is fine for a handful of tracks; cache it on `file-loaded` if it ever shows in a profile.
