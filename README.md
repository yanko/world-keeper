# World Keeper

A Windows-first desktop app for managing a [Core Keeper](https://www.corekeepergame.com/)
dedicated server's worlds over SSH — push a world in, pull the current world out, both
wrapped in a safe **stop → operate → restart → verify** envelope with automatic
rollback, plus live server stats, an interactive terminal, and a full audit trail.

The app is target-agnostic: it connects over SSH and **discovers** how the server is
actually run (systemd, Docker, or a raw process) rather than requiring you to declare
it up front. See [SPEC.md](SPEC.md) for the full design and [CONTEXT.md](CONTEXT.md)
for the reasoning behind it.

## What it does

- **Connection profiles** — password or SSH key auth (with a one-time "generate a key
  and install it on the server for me" helper), one active connection at a time.
- **Auto-discovery** — identifies the OS and whether Core Keeper is running under
  systemd, Docker, or a bare process, and locates the worlds directory, all surfaced
  for you to confirm before anything is saved.
- **Push a world** — upload → stop the server → back up the live world (dated,
  restorable) → copy yours in → fix permissions → restart → capture the game id →
  verify it's actually up. Any failure after the backup step auto-restores it and
  restarts the server.
- **Pull a world** — stop just long enough to copy a consistent world file, restart
  immediately, then save it locally as a folder or a `.zip`.
- **World library** — see what's live on the server and every dated backup this app
  has made, with one-click restore (itself backed up first, so a bad restore can be
  undone too).
- **Stats dashboard** — service status, memory/disk/uptime, the server's own clock,
  the currently-loaded game id (with copy-to-clipboard), and a live-updating log
  viewer.
- **Terminal** — a real interactive shell over the same SSH connection, for when you
  need to do something the app doesn't have a button for.
- **Audit log** — every connect, discovery, push, pull, restore, and error is recorded
  to a local SQLite database, filterable/searchable in-app, exportable as a CSV + a
  copy of the raw database.

## Stack

Tauri v2 (Rust backend, all real logic lives here) + React/TypeScript/shadcn-ui
frontend, SSH via `russh` (pure Rust, no system `ssh` dependency), secrets via the OS
keyring, config via `tauri-plugin-store`, audit log via `sqlx`/SQLite. See
[SPEC.md §3](SPEC.md) for the full rationale.

## Prerequisites

- **Node.js** (LTS) and **pnpm** (`corepack enable pnpm` or `npm install -g pnpm`)
- **Rust**, via [rustup](https://rustup.rs/)
- **Windows only:**
  - The MSVC C++ build tools (`rustup` will prompt for these, or install "Desktop
    development with C++" from the Visual Studio Build Tools installer)
  - WebView2 (preinstalled on Windows 11)
  - The **OpenSSH Client** optional feature, if you want to use the in-app "set up key
    auth" helper (it shells out to `ssh-keygen`; Windows 10/11 ship this by default,
    but it can be removed)

## Development

```powershell
pnpm install
pnpm tauri dev
```

This starts the Vite dev server and launches the app in a native window. Frontend
changes hot-reload; Rust changes trigger a recompile and restart the app
automatically.

Useful side commands while working on the Rust side, from `src-tauri/`:

```powershell
cargo check          # fast type-check
cargo clippy          # lints
```

## Building for release

```powershell
pnpm tauri build
```

This produces a native installer under `src-tauri/target/release/bundle/` — on
Windows, both an NSIS installer (`nsis/`) and an MSI (`msi/`). The build is
**unsigned** (no code-signing certificate is configured), so Windows SmartScreen will
warn on first run; that's expected for a personal/homelab tool and not a build error.
macOS is the same story with Gatekeeper's "unidentified developer" warning.

### Cross-platform releases via GitHub Actions

Tauri can't cross-compile — a macOS `.app`/`.dmg` has to be built *on* macOS, a Windows
installer *on* Windows. [`.github/workflows/release.yml`](.github/workflows/release.yml)
handles this by building on both platforms' native GitHub-hosted runners:

- **Push a tag like `v0.1.0`** to build installers for Windows and macOS (universal:
  Intel + Apple Silicon in one `.dmg`) and attach them to a new **draft** GitHub
  Release — review/edit the notes and publish it yourself, nothing goes out
  automatically.
- **Or run the workflow manually** (Actions tab → "release" → "Run workflow") to test
  the build without cutting a release; it uploads the same installers as regular
  workflow artifacts instead.

Before tagging, bump the version in `src-tauri/tauri.conf.json` and `package.json` to
match — the installer *filenames* come from `tauri.conf.json`'s version, not the git
tag, so a mismatch there just looks confusing in the release assets.

## Data locations

The app stores its own data outside the repo, in the OS app-data/app-config
directories for `com.civortech.worldkeeper` (on Windows, under
`%APPDATA%\com.civortech.worldkeeper\`):

- `config.json` — connection profiles (never contains passwords or keys)
- `ckmanager.db` — the SQLite audit log
- `keys/` — SSH keys this app generated via the key-auth setup helper

Passwords and app-generated SSH keys themselves live in the OS keyring (Windows
Credential Manager), never in a plaintext file.

## Project docs

- [SPEC.md](SPEC.md) — full design spec: architecture, config schema, the safe
  push/pull state machine, auto-discovery, audit schema, and the build order this was
  actually built in (including everything added after the initial spec).
- [CONTEXT.md](CONTEXT.md) — handoff notes: the homelab environment this was built
  against (details genericized) and the reasoning behind the key design decisions.

## License

[MIT](LICENSE) — this is a homelab tool, provided as-is with no warranty; see
the disclaimer shown in-app on first launch.
