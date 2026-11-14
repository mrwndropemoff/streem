# 15: Sign up, sign in, stay signed in

**What to build:** The app opens on a sign-in screen. A person creates an **account** with email and password, or signs in to an existing one, and then reaches the app. After a restart they are still signed in. Signing out returns to the sign-in screen.

**Blocked by:** 03, 14

**Status:** ready-for-agent

## Read first

- `CONTEXT.md` (**Account**)
- `docs/adr/0001-hosted-supabase-with-local-copy.md`
- `.cargo/config.toml` (created in ticket 14)
- `crates/core/src/lib.rs`, `crates/core/src/app.rs`, `crates/core/src/store.rs`
- `crates/desktop/src/main.rs`

## Files

- New: `crates/core/src/backend/mod.rs`, `crates/core/src/backend/auth.rs`
- Edit: `crates/core/src/lib.rs`, `crates/core/src/app.rs`, `crates/core/src/store.rs`
- New: `crates/desktop/src/screens/sign_in.rs`
- Edit: `crates/desktop/src/main.rs`, `crates/desktop/src/screens/mod.rs`

## Additions to the interface

```rust
pub struct BackendConfig { pub url: String, pub key: String }

impl BackendConfig {
    /// From STREEM_SUPABASE_URL and STREEM_SUPABASE_KEY, read at compile time with option_env!.
    pub fn from_build() -> Option<BackendConfig>;
}

impl Core {
    /// `Core::start` now calls this with `BackendConfig::from_build()`. Tests call it directly.
    pub fn start_with_backend(data_dir: PathBuf, backend: Option<BackendConfig>, on_change: impl Fn() + Send + Sync + 'static) -> Core;
}

pub enum Session {
    NotConfigured,
    SignedOut { error: Option<String> },
    Working,
    SignedIn { email: String },
}

// State
pub session: Session,

// Command
SignUp { email: String, password: String },
SignIn { email: String, password: String },
SignOut,
```

## Supabase Auth over plain HTTPS

Use `reqwest` directly; do not add a Supabase SDK crate. Every request carries the header `apikey: <key>`. Check the request and response shapes against the current Supabase Auth REST documentation before relying on them.

- Sign up: `POST {url}/auth/v1/signup`, JSON body `{"email", "password"}`.
- Sign in: `POST {url}/auth/v1/token?grant_type=password`, same body.
- Refresh: `POST {url}/auth/v1/token?grant_type=refresh_token`, body `{"refresh_token"}`.
- Sign out: `POST {url}/auth/v1/logout` with `Authorization: Bearer <access token>`.
- A successful answer contains `access_token`, `refresh_token`, `expires_in` and `user.id`, `user.email`.
- On an error answer, show the server's own message from the JSON body in `SignedOut.error`. On a network failure, show "Could not reach the server. Check your connection."

## Behaviour (`backend/auth.rs`)

- Keep the access token in memory only. Provide one function the rest of `streem-core` calls to get a valid access token; it refreshes first when fewer than 60 seconds remain.
- Save `refresh_token`, user id and email to `<data_dir>/session.json`. On Unix, create it with permissions `0600`. Add the comment `// shortcut: refresh token in a plain file, move it to the OS keyring before the app is shared beyond trusted people`.
- At startup with a session file: go to `SignedIn` straight away and refresh in the background. If the server rejects the refresh token, go to `SignedOut` with "Please sign in again." A network failure keeps the person signed in and retries later.
- `SignOut` calls the logout endpoint (ignoring failures), deletes the session file and goes to `SignedOut`.
- Never log a password, a token or the session file's contents.
- With no `BackendConfig`, the session is `NotConfigured`.

## Local data and the account

- Add a migration creating table `local_state(key TEXT PRIMARY KEY, value TEXT)`.
- On reaching `SignedIn`, compare `local_state.account_id` with the signed-in user id. If it is different or missing, delete every row from every data table, then store the new id. Data stored before sign-in existed is development data and is not carried over.
- From now on every row is written with `account_id` set to the signed-in user id, not `local`. `profile_id` stays `local` until ticket 18.
- The "install Cinemeta when there are no **addons**" rule from ticket 03 now runs after sign-in.

## Screen

- `screens/sign_in.rs`: email field, password field (hidden), "Sign in" and "Create account" buttons, the error text, and a spinner while `Working`. Enter submits sign-in.
- `main.rs` shows only this screen unless the session is `SignedIn`. For `NotConfigured` it shows "The sync backend is not configured. See ticket 14."
- Add "Sign out" to the Settings screen if it exists, otherwise to the top bar.

## Out of scope

**Profiles**, syncing any data, password reset, changing email or password.

## Acceptance checks

- [x] `cargo test -p streem-core` passes, including tests against a `wiremock` server standing in for Supabase that: `SignUp` reaches `SignedIn` and writes `session.json`; `SignIn` with a rejected password reaches `SignedOut` with the server's message; a new `Core` on the same data directory starts as `SignedIn` and calls the refresh endpoint; a rejected refresh token gives `SignedOut`; a refresh that cannot connect stays `SignedIn`; `SignOut` deletes `session.json`; signing in as a second user id empties the `addons` table; a token with 30 seconds left is refreshed before use; every request carries the `apikey` header.
- [ ] On Unix a test asserts `session.json` has mode `0600`.
- [x] A test captures the log output of a sign-in and asserts neither the password nor a token appears.
- [ ] `cargo run -p streem-desktop`: create an account, see the app, restart, still signed in, sign out, sign in again.

## Rules

- Use the terms defined in `CONTEXT.md` in names, messages and comments.
- Add crates with `cargo add`. Never type a version number from memory.
- `streem-desktop` does no network or disk work. It sends a `Command` to `Core` and draws `State`.
- Do not install system packages. If something is missing, stop and write what is missing under `## Comments` in this file.
- Do only what this ticket lists. Anything under "Out of scope" belongs to another ticket.
- Finish with `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace`. All three must pass.
- When done, tick the boxes and add a line under `## Comments` saying what you verified and how.

## Comments
- 2026-11-13, agent: Implemented on branch `ticket-15-sign-up-and-sign-in`. Verified on Windows: `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` all pass. The `wiremock` tests are in `crates/core/tests/auth.rs`; the log-capture test is `crates/core/tests/auth_logs.rs`, and it checks that each secret really reached the server and that reqwest and hyper_util records were captured. The 30-seconds-left refresh is a unit test in `crates/core/src/backend/auth.rs`. Mutation check: when a connection failure was made to count as a rejection, the cannot-connect test failed.
  - Box 2 is not ticked: `the_session_file_is_readable_by_its_owner_only` is `cfg(unix)` and has not run yet. CI's ubuntu job will run it.
  - Box 4 is not ticked. 2026-11-14: `main` (with ticket 14) merged into this branch; `.cargo/config.toml` is present and the hosted Auth server answers `/auth/v1/health` with 200, sign-up enabled and email auto-confirmed. Creating the account and signing in against the hosted project is left to a person, since it creates a real account with a password. Steps: `cargo run -p streem-desktop`, create an account, see the app, restart, still signed in, sign out in Settings, sign in again. The first sign-in empties the local data tables (pre-sign-in development data).
  - Decision: with no `BackendConfig` (`NotConfigured`), `Core` keeps the behaviour from before this ticket: rows under account `local`, and default addons installed at startup. This keeps the existing test suite offline. `Core::start_with_options` takes both a backend and the default addon list so tests can do both.
  - Decision: sign-out posts to `/auth/v1/logout?scope=local`. Supabase's default scope is `global`, which would sign out every device of the shared **account**. When there is no access token (after a restart), sign-out refreshes first so it can call logout.
  - Decision: on refresh, a 4xx other than 429 counts as rejected. A network failure, a 5xx or a 429 keeps the person signed in and retries every 30 s, so a paused free project does not sign anyone out.
  - Decision: if switching the local data to the signed-in **account** fails, the person is signed out with the reason, rather than rows being written for the previous **account**.
  - Left for later: the reset of per-**account** values in `App::run` (`forget_account`) is a long list of loop locals. Ticket 16 adds sync state and may want them gathered into one struct.
  - Request and response shapes were checked against the supabase/auth `openapi.yaml` and README. With confirmation off, `/signup` answers with tokens. `AccessTokenResponseSchema` lists `user` but requires nothing, so a refresh answer without `user` is accepted (unit-tested). Error messages are read from `msg`, then `error_description`, `message` and `error`. Logout answers 204.
  - Known: signing out while offline waits for the refresh attempt, at most the 20 s client timeout, before reaching `SignedOut`.
