# Streem v1

Terms in **bold** are defined in `CONTEXT.md`. Decisions with lock-in are in `docs/adr/`.

## What it is

A Stremio-like desktop app for one family-and-friends group. It installs Stremio-protocol **addons**, browses their catalogs, and plays their streams in an embedded player. Its differences from Stremio are **profiles** under one **account**, with everything synced over the internet, and **watch parties**.

It is never a paid product. It may be open-sourced later.

## Decisions

| Area | Decision |
|---|---|
| Platforms | Linux (Wayland) and Windows, both from the start. Android is after v1. |
| Language | Rust for everything. |
| UI | egui with its OpenGL backend. Gated on the step 1 prototype; fallback is Slint. |
| Player | libmpv embedded through its OpenGL render API. `egui_sharkplayer` is the first candidate. |
| Extension mechanism | **Addons** only. There is no in-app plugin system. |
| Protocol code | Reuse the addon protocol types and HTTP client from `stremio-core` (MIT). Do not reuse its state, account or library model. |
| Real-Debrid | Handled by addons, not by the app. The app has no Real-Debrid API client. |
| Torrent engine | None. The app never speaks the torrent protocol. |
| Sync backend | Supabase hosted free tier: login, data, and watch-party signals. See ADR-0001. |
| Local data | Each device keeps a local copy of the account's data. The UI reads only the local copy. |

## Workspace

Two crates. The package names carry a prefix because a package named plain `core` collides with Rust's built-in crate.

- `streem-core` in `crates/core`: addons, local storage, sync, playback commands, watch parties. No UI code.
- `streem-desktop` in `crates/desktop`: egui screens and the mpv player surface.

Two rules hold from step 2 onward:

1. `desktop` never does network or disk work. It sends requests to `core` and draws the state `core` hands back. This is what keeps scrolling smooth while catalogs and posters load.
2. Every play, pause and seek is a command sent to `core`. The UI never drives mpv directly. A **watch party** is then the same commands arriving from another device.

## Account, profiles and overrides

- An **account** is created and signed in to with email and password inside the app.
- An **account** has any number of **profiles**. After sign-in the app shows a profile picker.
- Everyone signed in to the **account** can change anything, including other **profiles**. There are no roles and no PINs in v1.
- The **account** owns the settings and the **addon** list. Every **profile** sees the same **addon** list.
- A **profile** may set an **override** for exactly two things: the Real-Debrid key, and playback preferences (audio language, subtitle language). Every other setting comes from the **account**.
- A setting resolves to the **profile**'s **override** if set, otherwise the **account**'s value.
- Each **profile** has its own watch history, continue-watching positions and library.

## Real-Debrid through addons

Torrentio keeps its own records of which torrents Real-Debrid already holds, and resolves a stream to a playable link on its own server. With its "don't show download to debrid links" option, it lists only streams that are ready. The app therefore receives ordinary HTTP links and plays them.

The Real-Debrid key sits inside the **addon**'s install URL in plain text. So that the **addon** list can stay shared while the key differs per **profile**:

- An **addon** URL stored on the **account** may contain the placeholder `{real_debrid_key}`.
- When a **profile** talks to that **addon**, the app replaces the placeholder with the **profile**'s resolved key.
- If the **profile** has no key, that **addon** is skipped for the **profile** and the app says why.

Torrentio's records are built from what its users played, so a listed stream can occasionally fail. The app then says the stream could not be played and returns to the stream list.

## Sync

- Synced: **account** settings, **addon** list, **profiles**, **overrides**, watch history, continue-watching positions, library.
- The Real-Debrid key is stored on the sync server, readable only by its **account**.
- The local copy is written first; sync runs in the background. The most recent change wins, including when one **profile** is used on two devices at once.
- v1 needs internet. When the sync backend cannot be reached the app says it is offline. There is no offline mode until **downloads** exist.

## Watch party

- Between **profiles** of the same **account** only. No invite links or codes.
- A **profile** starts one from a playing title. Other **profiles** see that it is running and can join.
- Anyone in the party can play, pause and seek. It applies to everyone.
- When one device buffers, the others keep playing. A device more than about two seconds off the party's position jumps to it.
- The party shows who is in it. No chat, reactions or voice.
- Everyone plays the same file, each through their own Real-Debrid key. The party shares which file is playing, never a link that contains a key.
- Signals travel over Supabase Realtime.

## Build order

Each step is usable before the next one starts. v1 is steps 1 to 5.

1. **Prototype.** An egui window plays an MKV with subtitles and a second audio track through libmpv, on Linux and on Windows. Throwaway code. If it fails, try Slint before anything else is built.
2. **Browse and play.** Install an **addon** by URL, browse catalogs, search, open a title, list its streams, play a direct HTTP stream. Subtitles come from **addons**. One local **profile**, no login.
3. **Real-Debrid.** The `{real_debrid_key}` placeholder in **addon** URLs, and the failed-stream message.
4. **Account and profiles.** Sign-up, sign-in, profile picker, **overrides**, watch history, continue-watching, library, all synced.
5. **Watch party.**

## After v1

**Downloads** and offline mode, "because you watched X" rows built from TMDB (the first feature that needs a TMDB key in the app), profile PINs, Android.

## Not planned

A torrent engine, an in-app plugin system, chat or voice, payments, **watch parties** across different **accounts**.

## Assumptions

- No limit on **profiles** per **account**.
- English text only.
- Posters are cached on disk.

## Open risks

- **libmpv in egui** is untested on Hyprland and on Windows. Step 1 exists to settle it.
- **Matching the same file in a watch party.** Each device gets its own key-specific link from the **addon**, so the party has to identify the file some other way, likely by the filename the **addon** reports in each **profile**'s own stream list. To be settled at the start of step 5.
- **The placeholder only works for addons with plain-text URLs**, such as Torrentio. **Addons** that encode their configuration cannot take a per-profile key this way.
- **Real-Debrid playback depends on a third-party addon** continuing to exist and to track what Real-Debrid holds.
- **`stremio-core` as a dependency** is unverified. If its protocol types cannot be depended on alone, copy them with attribution.
- **Supabase free tier** pauses a project after a week without database activity. Data is kept and the project is restored from the dashboard.
