# CONTEXT.md — Core Keeper World Manager

Handoff notes for building this project in Claude Code. Read alongside `SPEC.md`.
This file captures *why* the decisions were made and the real-world facts discovered,
so you don't have to re-derive them.

---

## What we're building

A Windows-first (cross-platform) **Tauri v2 desktop app** to manage a Core Keeper
dedicated server's worlds over SSH: push a world in, pull the current world out, both
wrapped in a safe **stop → operate → restart → verify** envelope with auto-rollback, plus
live server stats and a full SQLite audit log.

The app is **target-agnostic**: transport is always SSH, but the endpoint could be a
Proxmox LXC (systemd), a Docker container, or a raw process. How to stop/start the server
and where worlds live are **discovered on connect**, never hard-coded.

---

## Real environment (the first target)

- Proxmox VE 9.2 homelab. Core Keeper runs in **LXC 106**, hostname `core-keeper`.
- SSH: `root@10.0.0.68`.
- Server runs as user **`steam`** under **systemd**:
  - unit: `corekeeper` — `User=steam`,
    `WorkingDirectory=/home/steam/corekeeper`,
    `ExecStart=/home/steam/corekeeper/_launch.sh`
  - worlds dir:
    `/home/steam/.config/unity3d/Pugstorm/Core Keeper/DedicatedServer/worlds`
- **World files (confirmed by `ls -la`):**
  ```
  0.world.gzip            5046105 bytes   (live world, gzip blob)
  0.world.gzip.pugbackup  5047192 bytes   (CK's own rolling backup, ~1 save behind)
  ```
  So **a world = `N.world.gzip` + `N.world.gzip.pugbackup`** (slot N). They move together
  as an atomic set. `.pugbackup` is one save-cycle older than the live file.
- After any write to world files on a systemd target: `chown steam:steam`.

Dev's machine: **Windows 11**, connecting over the LAN. Uses OpenSSH from PowerShell today.

---

## Key decisions & the reasoning behind them

1. **Tauri v2 + React + shadcn/ui + Tailwind; all real logic in Rust.**
   - Dev knows this stack (used it for a prior disk-management Tauri app). React layer is
     thin; SSH, state machine, file detection, keyring, audit all live in Rust commands.

2. **Rejected Topcoat (tokio-rs).** The dev asked about it. We checked: it's a brand-new
   (trending ~mid-July 2026), explicitly *experimental* **server-side web** framework —
   server-rendered Rust components, htmx, Tailwind-without-Node. Wrong *shape* for a
   desktop tool (no native file dialogs, no fs/keyring, no desktop shell) and unstable.
   Great for a future homelab *web dashboard*, not this. Dioxus/Leptos were noted as the
   real "all-Rust UI" options but React+shadcn ships faster here.

3. **SSH via `russh` + `russh-sftp` (pure Rust).** No dependency on the system `ssh`
   binary. Auth: **SSH key primary**, password fallback (first connect / key install).
   - Important correction made during the chat: `~/.ssh/config` is **not** a credential
     store — it's just a CLI alias file. The app does NOT rely on it. Connection profiles
     replace it for the dropdown UX. Key auth is what removes password typing.

4. **Secrets never in plaintext.** Use the `keyring` crate → Windows Credential Manager.
   Config files store only *which* key/user and a `savePassword` flag, never the secret.

5. **Target abstraction = one `ServerTarget` trait.** The state machine calls the trait;
   only `service_control`, `fix_perms`, and `worlds_path` vary by a **discovered**
   `ServiceKind` (Systemd | Docker | Process). See SPEC §4.
   - Evolution of this idea in the chat: first proposed a per-profile `targetType` field.
     Dev pushed back — "does it need to know? you're already doing SSH; discover it."
     Correct. There is **no user-declared target type**; a discovery probe detects
     systemd vs docker vs process (and the OS, and the worlds path) and caches it. This
     is strictly simpler and more general.

6. **Discovery, not assumption** — mirrors how paths were found by hand in the chat.
   Every probe result is **shown for confirmation, never silently applied**, then cached
   in `profile.discovered`. Probe commands are in SPEC §8.

7. **SQLite audit log from app start** via `tauri-plugin-sql`. Every session, state
   transition, file op, backup, and error is a row. Secrets never logged. Schema in
   SPEC §9. Motivation: this tool does *destructive remote ops*, so a forensic trail
   ("what happened to world 0 on the 24th", "which step did the swap die on") matters.

8. **v1 scope: BOTH push and pull.** Push stops the server for the whole swap; pull stops
   only long enough to copy a consistent file, restarts immediately, then packages on the
   PC side. Same safe envelope, different middle. State machine is the heart of the app
   and the visual signature (an animated "swap stepper").

9. **World detection is smart.** Local: user picks file/folder/zip; app recursively scans
   (unzips to temp if needed), globs `*.world.gzip`, groups by slot, picker if >1. Remote
   worlds path comes from discovery. Config-driven patterns so it's tweakable.

---

## Design direction (see SPEC §12)

Vibe: *server rack meets game HUD* — precise instrumentation, not SaaS card grid.
Cool slate/graphite base, ONE signal color for healthy/live (phosphor teal or amber),
danger red reserved for stop/rollback. Monospace for all machine data (paths, sizes,
journal, game id); grotesk for UI chrome. **Signature element: the swap stepper** — a
horizontal pipeline that animates through stages, each showing its live journal line,
turning the scary destructive swap into something you watch with confidence. Avoid the
generic AI-default looks (cream+serif+terracotta; black+acid-green).

---

## Things to verify from the live server during the build (SPEC §14)

- **Exact journal line** for the world/game id on load → regex for `CapturingGameId`.
  Get it: start the server and watch `journalctl -u corekeeper -f`.
- **`cat /home/steam/corekeeper/_launch.sh`** — does it pin a specific slot / world name?
  Determines whether a pushed world must be renamed to a specific slot to be the one that
  loads. **Most important open question.**
- **UDP port** actually in use → `ss -ulnp | grep -i corekeeper` (default assumed 27015).
- **Core Keeper Steam app id** for the "update available" badge → verify `1963720`.

---

## Suggested first tasks in Claude Code

1. Confirm the scaffold built and runs (`pnpm tauri dev`).
2. Wire `tauri-plugin-store` (config), `tauri-plugin-sql` (audit), `keyring`.
3. Define the `ServerTarget` trait + `ServiceKind` enum; implement the SSH transport with
   `russh`/`russh-sftp`; get a password connect + `exec` working against `root@10.0.0.68`.
4. Build the discovery probe; confirm it correctly identifies the systemd/`corekeeper`
   target and the worlds path on LXC 106.
5. Then the read-only stats, then the push state machine (backup+rollback before copy),
   then pull, then key auth, then the Docker ServiceKind.

Build order detail is in SPEC §13. Keep all target-specific logic inside the SSH+
ServiceKind impl; the state machine must stay target-agnostic.
