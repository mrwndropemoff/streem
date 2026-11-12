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

- [ ] Every "Human check" box in ticket 01 is ticked for Linux.
- [ ] The same list passes on Windows, or the failure is written down.
- [ ] Either `docs/adr/0002-egui-for-the-desktop-ui.md` exists, or a Slint prototype ticket exists.
- [ ] The Windows build steps are written under `## Comments`.

## Comments
**Agent prep notes (2026-11-12). Unverified: nothing here was run on Windows. No box above is ticked.**

- Linux sample check from ticket 01 passes: `ffprobe` on `sample/sample.mkv` lists 1 video, 2 audio, 1 subtitle. The ticket 01 "Human check" boxes are still open.
- Linking: `libmpv2-sys` 4.0.1 emits `cargo:rustc-link-lib=mpv` and nothing else (no Windows logic, no `MPV_SOURCE` unless the `build_libmpv` feature is on). On MSVC the linker looks for `mpv.lib`. The `mpv-dev` archive ships `libmpv.dll.a` and `mpv.def`, not an MSVC `.lib`. Likely fix (try first): in a VS developer prompt run `lib /def:mpv.def /machine:x64 /out:mpv.lib`, then set `RUSTFLAGS=-L native=<dir with mpv.lib>`. Fallback: the `x86_64-pc-windows-gnu` target, which can use `libmpv.dll.a` directly.
- The DLL must be next to the `.exe` (`target\release\`). Check its actual name in the archive before copying; the ticket says `libmpv-2.dll`.
- Rewrite this section with the steps that actually worked.
