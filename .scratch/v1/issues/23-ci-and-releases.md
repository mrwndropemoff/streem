# 23: CI and releases

**What to build:** GitHub Actions that check every change on Linux and Windows, run the internet tests weekly, and turn a version tag into a draft GitHub Release with a Windows zip and a Linux tarball.

**Blocked by:** none

**Status:** ready-for-human

## Decisions

- **CI** (`ci.yml`) runs on pushes to `main` and pull requests into `main`, on `ubuntu-latest` and `windows-latest`. It fails on `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings` or `cargo test --workspace`. Latest stable Rust, no pinned toolchain, no minimum-version job.
- **Internet tests** (`live.yml`): the `#[ignore]` tests run weekly and on demand, never on pull requests.
- **Releases** (`release.yml`): a `v*` tag must match `[workspace.package] version`. It builds `streem-<ver>-windows-x86_64.zip` (exe, `libmpv-2.dll`, `LICENSE`, mpv's licence files) and `streem-<ver>-linux-x86_64.tar.gz` (binary and `LICENSE`, using the system libmpv), and creates a **draft** release. No code signing; the README explains the SmartScreen prompt. x86_64 only.
- **libmpv on Windows** is one build pinned by URL and SHA-256 in `.github/scripts/fetch-libmpv.ps1`, from SourceForge (the GitHub copies are deleted after about 30 days).
- One app version in the root `Cargo.toml`, starting at `0.1.0`.

## Left for a human

- [ ] In GitHub → Settings → Branches, make the `CI` checks (`rustfmt`, `clippy and tests (ubuntu-latest)`, `clippy and tests (windows-latest)`) required for `main`.
- [ ] Cut `v0.1.0` (README → "Making a release") and check both downloads start on a real machine.

## Comments

- Recent `mpv-dev-x86_64` archives no longer ship `mpv.def`, only `libmpv-2.dll`, `libmpv.dll.a` and headers. The script writes `mpv.def` from `dumpbin /exports` (206 exports) and runs `lib /def`. Verified locally: with `RUSTFLAGS=-L native=<that folder>` and the DLL on `PATH`, `cargo test -p streem-desktop` built and passed.
- The plan said to build the Linux release on Ubuntu 22.04 for older distros. That cannot work: `libmpv2` needs mpv 0.35 (`libmpv.so.2`) and 22.04 ships 0.34. Releases build on `ubuntu-24.04` instead, so the Linux binary needs glibc 2.39 or newer.
- `release.yml` also runs as a dry run on any pull request that touches it or the libmpv script: it builds and uploads both packages as workflow artifacts but skips the tag check and creates no release. The tag check and the draft release itself are only exercised by the first real tag.
- `streem-desktop` has no `#![windows_subsystem = "windows"]`, so the release `.exe` also opens a console window. Not changed here.
