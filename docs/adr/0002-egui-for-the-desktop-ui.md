# egui for the desktop UI

The app must play video through libmpv inside its own window on Linux (Wayland) and Windows. We build the UI with egui (through eframe and its OpenGL renderer) because the UI is plain Rust in a single language, and a prototype proved that libmpv renders into egui's own OpenGL context, with egui controls drawn on top, on both systems. The checks covered playback, an overlaid control bar, seeking, audio and subtitle track switching, resizing, fullscreen, and near-zero CPU use while paused.

## Considered Options

- **Slint**: a second language to learn for the UI. Kept as the fallback if egui stops working for us.
- **Tauri**: its libmpv plugin listed Linux window embedding as not working.
- **Flutter**: a second toolchain and a bridge to Rust.

## Consequences

- libmpv can only draw through OpenGL, so eframe must use the `glow` renderer, not its default `wgpu`.
- mpv draws through its render API into egui's framebuffer. The `wid` option is not used because it does not work on Wayland.
- The prototype under `prototypes/egui-mpv` is throwaway. Its `libmpv2` approach and the shortcuts listed in ticket 01's comments need redoing properly when the player ships (ticket 07).
- Windows builds need an MSVC import library (`mpv.lib`) made from the mpv development archive, and `libmpv-2.dll` next to the `.exe`. Ticket 02's comments have the exact steps.
