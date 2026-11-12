# 02: Run the prototype on Windows and record the egui decision

**What to build:** Confirmation that the ticket 01 prototype also works on Windows, followed by the written decision on the UI toolkit. This is the gate for all app work.

**Blocked by:** 01

**Status:** ready-for-human

## Why a human

It needs a Windows machine and someone looking at the video.

## Read first

- `.scratch/v1/issues/01-prototype-egui-libmpv-linux.md`, especially its `## Comments` and "Human check" list.
- `docs/adr/0001-hosted-supabase-with-local-copy.md`, as an example of the ADR style.

## Steps

1. Finish the "Human check" list in ticket 01 on Linux and tick its boxes.
2. On Windows, install Rust with `rustup`.
3. Get the libmpv development files for Windows: the `mpv-dev-x86_64` archive from the Windows builds linked on mpv.io. It contains `libmpv-2.dll` and the import library. Point the build at them the way the crate used in ticket 01 documents, and put the DLL next to the built `.exe`.
4. Copy `prototypes/egui-mpv/sample/sample.mkv` over, or regenerate it.
5. In `prototypes/egui-mpv`, run `cargo run --release -- sample\sample.mkv`.
6. Go through the same "Human check" list as ticket 01.

## Outcome A: both systems pass

- Write `docs/adr/0002-egui-for-the-desktop-ui.md`. One short paragraph: the app needs libmpv embedded on Linux (Wayland) and Windows; egui was chosen because the UI is plain Rust in a single language, and the prototype proved libmpv works in it on both systems. Add a "Considered Options" list: Slint (a second language to learn; kept as the fallback), Tauri (its libmpv plugin listed Linux window embedding as not working), Flutter (a second toolchain and a bridge to Rust).
- Under `## Comments` here, write the Windows setup steps that worked, exactly. Ticket 07 needs them.

## Outcome B: either system fails

- Write what failed under `## Comments`.
- Do not start tickets 03 to 22. Their UI parts assume egui. Open a new ticket for a Slint prototype with the same checks, and revise the later tickets after it.

## Acceptance checks

- [x] Every "Human check" box in ticket 01 is ticked for Linux.
- [x] The same list passes on Windows, or the failure is written down.
- [x] Either `docs/adr/0002-egui-for-the-desktop-ui.md` exists, or a Slint prototype ticket exists.
- [x] The Windows build steps are written under `## Comments`.

## Comments
**Agent prep notes (2026-11-12). Unverified: nothing here was run on Windows. No box above is ticked.**

- Linux sample check from ticket 01 passes: `ffprobe` on `sample/sample.mkv` lists 1 video, 2 audio, 1 subtitle. The ticket 01 "Human check" boxes are still open.
- Linking: `libmpv2-sys` 4.0.1 emits `cargo:rustc-link-lib=mpv` and nothing else (no Windows logic, no `MPV_SOURCE` unless the `build_libmpv` feature is on). On MSVC the linker looks for `mpv.lib`. The `mpv-dev` archive ships `libmpv.dll.a` and `mpv.def`, not an MSVC `.lib`. Likely fix (try first): in a VS developer prompt run `lib /def:mpv.def /machine:x64 /out:mpv.lib`, then set `RUSTFLAGS=-L native=<dir with mpv.lib>`. Fallback: the `x86_64-pc-windows-gnu` target, which can use `libmpv.dll.a` directly.
- The DLL must be next to the `.exe` (`target\release\`). Check its actual name in the archive before copying; the ticket says `libmpv-2.dll`.
- Rewrite this section with the steps that actually worked.

---

**Outcome A (2026-11-12): both systems pass.** The agent verified the build and a headless run on Windows 11; the human then ran the check list below and all six items passed. `docs/adr/0002-egui-for-the-desktop-ui.md` is written. Tickets 03 onward can start on egui.

### What I verified on this machine

- Toolchain: `rustc 1.99.1`, host and only target `x86_64-pc-windows-msvc`.
- `cargo build --release` in `prototypes/egui-mpv` finishes with exit code 0. The tree was already built, so this was an up-to-date check, not a clean build.
- Run of `target\release\egui-mpv-prototype.exe sample\sample.mkv` for 20 s:
  - still running at the end;
  - one window, titled `egui-mpv prototype`;
  - 19 `time-pos` lines, 0.7 to 18.7, strictly increasing by 1.0 s each;
  - stderr empty, no panic, no mpv warning or error line.
- The first log lines read `aid=1 sid=no`. Subtitles start off on Windows. I did not compare this with Linux.

### How linking is set up here (found, not created by me)

- `C:\libmpv` holds `libmpv-2.dll` and `libmpv.dll.a` (both from the `mpv-dev-x86_64` archive; the DLL reports mpv `v0.42.0-38-gbcde6b93c`), plus `mpv.def`, `mpv.exp`, `mpv.lib` and `exports.txt`. The last four were generated at 09:52 today, which matches the `lib /def:mpv.def /machine:x64 /out:mpv.lib` fix from the prep notes. I did not see the commands run, so confirm that.
- The linker finds `mpv.lib` through `%USERPROFILE%\.cargo\config.toml`:
  ```toml
  [target.x86_64-pc-windows-msvc]
  rustflags = ["-L", "native=C:\\libmpv"]
  ```
  `RUSTFLAGS` and `MPV_SOURCE` are unset. This is a per-user file outside the repo, so a fresh clone on another machine will not link until it is repeated.
- `libmpv-2.dll` is copied by hand into `target\release\` (the DLL name in the archive matches the ticket).
- `lib.exe` and `link.exe` are not on the plain PowerShell PATH, so `mpv.lib` must have been made from a VS developer prompt.

### Human check on Windows (passed)

- [x] Video is inside the window, not a second window.
- [x] Control bar drawn over the video and usable.
- [x] Play/Pause button and seek slider work.
- [x] Audio track switch changes the tone (`eng` and `fra` sine tones); subtitle switch shows and hides text.
- [x] Resize and fullscreen (`F`) keep the picture correct.
- [x] CPU near zero while paused (Task Manager).

### Windows setup steps for ticket 07

These are the steps behind the build that passed. The `mpv.lib` step (3) is inferred from the files in `C:\libmpv`, not watched.

1. `rustup` with the default MSVC toolchain; Visual Studio Build Tools with the C++ workload (needed for `link.exe`, `lib.exe`).
2. Download `mpv-dev-x86_64` from the Windows builds linked on mpv.io; extract to `C:\libmpv` (`libmpv-2.dll`, `libmpv.dll.a`, `mpv.def`, `include\`).
3. From a "x64 Native Tools" prompt in `C:\libmpv`: `lib /def:mpv.def /machine:x64 /out:mpv.lib`.
4. Add the `[target.x86_64-pc-windows-msvc]` block above to `%USERPROFILE%\.cargo\config.toml`.
5. `cd prototypes\egui-mpv`, `cargo build --release`, copy `C:\libmpv\libmpv-2.dll` into `target\release\`.
6. `cargo run --release -- sample\sample.mkv`. `sample.mkv` comes from `make-sample.sh` (needs `ffmpeg`/`ffprobe`, which are not installed on this PATH; the existing file is 1,925,889 bytes).
