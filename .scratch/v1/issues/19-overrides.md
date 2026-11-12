# 19: Overrides

**What to build:** A **profile** can have its own Real-Debrid key, audio language and subtitle language. Where it has none, it uses the **account**'s. Example: the **account** has one key used by everyone at home; a friend's **profile**, used from another house, sets its own key, and everything else stays shared. **Overrides** sync like the rest of the **profile**.

**Blocked by:** 17

**Status:** ready-for-agent

## Read first

- `CONTEXT.md` (**Override**, **Profile**, **Account**)
- `.scratch/v1/spec.md`, section "Account, profiles and overrides".
- `crates/core/src/settings.rs`, `crates/core/src/profiles.rs`, `crates/core/src/addon/url.rs`, `crates/core/src/app.rs`
- `crates/desktop/src/screens/settings.rs`

## Files

- Edit: `crates/core/src/lib.rs`, `crates/core/src/settings.rs`, `crates/core/src/profiles.rs`, `crates/core/src/app.rs`
- Edit: `crates/desktop/src/screens/settings.rs`

## Additions to the interface

```rust
pub struct Overrides {
    pub real_debrid_key: Option<String>,
    pub audio_lang: Option<String>,
    pub subtitle_lang: Option<String>,   // Some("off") means: no subtitles for this profile
}

pub enum OverrideSetting { RealDebridKey, AudioLang, SubtitleLang }

// Profile
pub overrides: Overrides,

// State
pub account_settings: Settings,   // the account's own values
// State.settings (already used everywhere) becomes the values resolved for the active profile

// Command
SetOverride { profile_id: String, setting: OverrideSetting, value: Option<String> },   // None removes the override
```

## Resolution (`settings.rs`)

```rust
pub fn resolve_settings(account: &Settings, overrides: Option<&Overrides>) -> Settings;
```

- For each of the three settings: the **profile**'s **override** if it has one, otherwise the **account**'s value.
- A subtitle **override** of `"off"` resolves to `None` (subtitles off). This is how a **profile** turns subtitles off when the **account** has a subtitle language.
- With no active **profile**, the result is the **account**'s values.
- This is a pure function. Nothing else in `streem-core` decides which value applies.
- `State.settings` is recomputed whenever the active **profile** changes, an **override** changes, the **account** settings change, or sync brings a change to any of them.
- Only these three settings can be overridden. The **addon** list and everything else always come from the **account**.

## Effects of a change

When the resolved Real-Debrid key changes for any reason (including `SelectProfile`), do what `SetRealDebridKey` does today: clear `skipped_reason`, re-fetch the manifests of placeholder **addons**, reload the home screen. Move that into one function and call it from every place.

The existing commands `SetRealDebridKey` and `SetPlaybackLanguages` keep changing the **account**'s values.

## Storage and sync

The `profiles` table already has `real_debrid_key`, `audio_lang` and `subtitle_lang`, locally and remotely, and is already synced. `SetOverride` writes the column and marks the row dirty. No schema change.

## Screen

`screens/settings.rs` gets two sections:

- **Account**: the existing three fields, labelled as applying to every **profile** that has no **override**.
- **This profile (name)**: for each of the three settings, a choice between "Use the account's" (showing the **account**'s value, with the key masked) and "Use my own" with a field. The subtitle choice also offers "Off".

Other **profiles'** **overrides** are edited by switching to that **profile**. An **override** key is hidden by default, like the **account** key.

## The key is a secret

The rules from ticket 12 apply to **override** keys too: never log one, never put one in a notice or an error message.

## Out of scope

**Overrides** for anything else, per-**profile** **addon** lists, PINs, editing another **profile**'s **overrides** without switching to it.

## Acceptance checks

- [ ] `cargo test -p streem-core` passes, including table-driven tests of `resolve_settings` covering, for each setting: **account** value only; **override** only; both (the **override** wins); neither; plus subtitle `"off"` over an **account** language, and no active **profile**.
- [ ] Tests against a `wiremock` server that: with an **account** key and **profile** A overriding it, a placeholder **addon** is requested with A's key when A is active and with the **account**'s key when B is active; with no **account** key and an **override** on A, the **addon** is skipped for B and works for A; `SetOverride` pushes the `profiles` row; a pulled `profiles` row with a new **override** key changes `State.settings` for the active **profile**; removing an **override** falls back to the **account** value.
- [ ] A test captures the log output while an **override** key is set and used, and asserts the key does not appear.
- [ ] `cargo run -p streem-desktop`: set an **override** audio language on one **profile**, play something, and it applies; switch **profile** and the **account**'s language applies.

## Rules

- Use the terms defined in `CONTEXT.md` in names, messages and comments.
- Add crates with `cargo add`. Never type a version number from memory.
- `streem-desktop` does no network or disk work. It sends a `Command` to `Core` and draws `State`.
- Do not install system packages. If something is missing, stop and write what is missing under `## Comments` in this file.
- Do only what this ticket lists. Anything under "Out of scope" belongs to another ticket.
- Finish with `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace`. All three must pass.
- When done, tick the boxes and add a line under `## Comments` saying what you verified and how.

## Comments
