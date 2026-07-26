# Core Keeper World Manager — SPEC.md

A cross-platform desktop app (Windows-first) to manage a Core Keeper dedicated server's
worlds over SSH. Push new worlds in, pull the current world out, both wrapped in a safe
stop → operate → restart → verify envelope, with full audit logging.

**The app is target-agnostic.** Transport is always SSH; the endpoint may be a Proxmox
LXC (systemd), a Docker container on the host, or a raw process. How the game server is
stopped/started and where its world files live are **discovered on connect**, not
declared up front. A Windows target may be added later as a distinct transport.

Author context: homelab, Proxmox VE 9.2, an LXC container (`core-keeper`), server
currently runs as user `steam` under systemd, worlds under the Unity3D config dir. This
is the first target, not a hard-coded assumption.

---

## 1. Goals

- Push a world from PC (file / folder / zip) into the LXC, safely swapping the live world.
- Pull the current live world from the LXC down to PC (folder or zip).
- Never corrupt a world: always stop the service before touching files, always back up
  the current world (dated) before overwriting, auto-rollback on failure.
- Capture the game/world ID from the journal after start, and show live server stats.
- Auto-discover paths/service on first run; keep everything configurable.
- Audit every action to a local SQLite DB.

## 2. Non-goals (v1)

- No agent installed on the target — pure SSH/sftp from the app.
- No multi-server orchestration (one connection profile active at a time; multiple
  profiles stored for the dropdown, but one is "connected" at a time).
- No in-app world editing / seed inspection.
- Multi-world *library* with rich metadata is a v2 stretch; v1 tracks backups it creates.
- Windows target (non-SSH transport) is v2+; v1 ships the SSH transport with
  systemd/docker/process service kinds discovered automatically.

---

## 3. Stack

| Layer            | Choice                                   | Why |
|------------------|------------------------------------------|-----|
| Shell            | **Tauri v2**                             | Native desktop, file dialogs, OS keyring, small binary |
| Frontend         | **React + TypeScript + shadcn/ui + Tailwind v4** | Known stack (matches disk-mgmt app), fast to build; v4 is CSS-first (no config file) |
| State machine    | **XState** (frontend) mirroring a Rust enum | Visual stepper maps 1:1 to states |
| Backend          | **Rust** (Tauri commands)                | All real logic here; React layer is thin |
| SSH/SCP          | **`russh` + `russh-sftp`** (pure Rust)   | No dependency on system ssh; key + password auth. Key loading is in `russh` itself now (old `russh-keys` merged). Needs a crypto backend feature (`aws-lc-rs` default). |
| Secrets          | **`keyring`** crate → Windows Credential Manager | No plaintext passwords ever |
| Config           | **`tauri-plugin-store`** (JSON) or TOML in app-config dir | Editable, no rebuild |
| Audit DB         | **`tauri-plugin-sql`** (SQLite/sqlx)     | Forensic trail of every action |
| Zip              | **`zip`** crate                          | Unpack picked zips, package pulls |

> Rejected: **Topcoat** (tokio-rs) — it's a brand-new *server-side web* framework, not a
> desktop/Tauri UI toolkit; wrong shape (no native file dialogs / fs / keyring) and
> explicitly unstable. Revisit for a homelab *web dashboard* later.
> Rejected for now: Dioxus/Leptos "all-Rust UI" — viable, but React+shadcn ships faster
> against existing muscle memory. Backend is already ~all Rust.

---

## 4. Target abstraction (the key design seam)

Everything the app does reduces to a few primitives. Define one **`ServerTarget` trait**;
the state machine calls the trait and never touches SSH, systemd, or docker directly.
Transport is always SSH in v1; only *service control* and *world path/perms* vary, and
those are **discovered**, not configured.

```rust
trait ServerTarget {                                  // SSH impl in v1
    async fn exec(&self, cmd: &Command) -> Result<Output>;
    async fn push_file(&self, local: &Path, remote: &str) -> Result<u64>;   // sftp
    async fn pull_file(&self, remote: &str, local: &Path) -> Result<u64>;   // sftp
    async fn stat(&self, glob: &str) -> Result<Vec<FileEntry>>;             // list world files
    async fn stream_logs(&self) -> Result<LogStream>;                       // per ServiceKind
    async fn service_control(&self, action: ServiceAction) -> Result<()>;   // per ServiceKind
    async fn fix_perms(&self, path: &str) -> Result<()>;                    // no-op where n/a
    fn worlds_path(&self) -> &str;                                          // per ServiceKind
}

enum ServiceAction { Status, Stop, Start }

// Discovered on connect, cached in the profile — NOT a user-declared target type.
enum ServiceKind {
    Systemd { unit: String },               // systemctl stop/start/status <unit>; journalctl -u
    Docker  { container: String },          // docker stop/start <c>; docker logs -f <c>
    Process { stop_cmd: String, start_cmd: String, log_path: Option<String> }, // raw fallback
}
```

Per-ServiceKind behaviour:

| ServiceKind | stop / start | logs | worlds path source | perms |
|-------------|--------------|------|--------------------|-------|
| **Systemd** | `systemctl stop/start <unit>` | `journalctl -u <unit> -f` | Unity config dir (found via `find`) | `chown steam:steam` |
| **Docker**  | `docker stop/start <container>` | `docker logs -f <container>` | bind-mount source from `docker inspect` | container UID (often no-op, or `docker exec chown`) |
| **Process** | run `stop_cmd` / `start_cmd` (`_launch.sh`, `pkill`) | tail `log_path` if any | `find` result | best-effort |

Because control goes through the trait, the swap/pull spine, backup logic, audit log,
and UI stepper are all **target-agnostic**. All LXC/systemd specifics live in one impl.

**World file grouping (target-independent):** a world = the slot files that travel
together —
- `N.world.gzip`            (live world, gzip blob)
- `N.world.gzip.pugbackup`  (Core Keeper's own rolling backup, ~1 save-cycle behind)
where `N` is the slot index. Operating on "world N" means operating on **all files
matching `N.world.gzip*`** as an atomic set. Detection globs `*.world.gzip` and groups by
leading slot index. (Confirmed from a real LXC: `0.world.gzip` + `0.world.gzip.pugbackup`.)

---

## 5. Configuration

Stored in OS app-config dir (e.g. `%APPDATA%/com.civortech.ckmanager/config.json`).
All fields editable in a Settings screen; "Discover" buttons can fill them.

The profile stores SSH connection + auth. The `discovered` block is **written by the
discovery probe** (§8), not typed by the user; it's cached so probing only happens on
first connect or manual re-scan. There is deliberately **no `targetType` field** — the
app figures out the ServiceKind itself.

```jsonc
{
  "profiles": [
    {
      "id": "ck-lxc-1",
      "name": "Core Keeper LXC",
      "host": "192.168.1.50",
      "user": "root",
      "auth": "key",                 // "key" | "password"
      "keyPath": "~/.ssh/id_ed25519",// when auth=key
      "savePassword": false,         // when auth=password → keyring (never here)

      // ── written by discovery probe, editable in Settings ──
      "discovered": {
        "os": "Debian 12",
        "serviceKind": {             // one of: systemd | docker | process
          "type": "systemd",
          "unit": "corekeeper"
        },
        "worldsPath": "/home/steam/.config/unity3d/Pugstorm/Core Keeper/DedicatedServer/worlds",
        "steamUser": "steam",        // for chown on systemd targets
        "launchScript": "/home/steam/corekeeper/_launch.sh",
        "probedAt": "2026-07-24T23:40:00Z"
      }
    }
  ],
  "worldDetection": { "patterns": ["*.world.gzip"] },
  "activeProfileId": "ck-lxc-1"
}
```

Passwords are **never** in this file. When `auth=password` + `savePassword=true`, the
secret lives in Credential Manager keyed by profile id.

---

## 6. Auth (per earlier decision: SSH key primary, password fallback)

- **Primary: key auth.** App reads private key (from `keyPath`, or a keyring-stored copy)
  and authenticates via `russh`. One-time setup helper: app can append your public key
  to `root@host:~/.ssh/authorized_keys` after a first password connect.
- **Fallback: password**, entered on first connect; optionally saved to Credential Manager.
- `~/.ssh/config` is **not** a credential store — it's only a CLI alias file; the app
  does not rely on it. Connection profiles replace it for the dropdown UX.

Connection dropdown (top bar): pick profile → app pulls key/secret → connects. No retyping.

---

## 7. Core: the safe operation envelope (state machine)

Both push and pull share one spine. Model as a Rust enum; mirror in XState for the stepper.

```
Idle
 ├─(push)→ SelectingWorld → ResolvingLocalWorld → Uploading
 │                                                    │
 │                                                    ▼
 │  ┌──────────────── SAFE ENVELOPE ─────────────────┐
 └─→│ CheckingService → Stopping → BackingUp →        │
    │ (push) Copying / (pull) Downloading →           │
    │ Fixing­Perms → Starting → CapturingGameId →      │
    │ Verifying → Done                                │
    └──── on any error → RollingBack → Failed ────────┘
```

**Push states**

1. `SelectingWorld` — native dialog: file / folder / zip.
2. `ResolvingLocalWorld` — recursively scan selection (unzip to temp if zip); glob
   `*.world.gzip`; group by slot; if >1 world, present picker with slot #, size, mtime;
   validate the `.pugbackup` sibling is grabbed too if present.
3. `Uploading` — sftp the world file set into a staging path on the LXC
   (`<worldsPath>/.staging/`), verify sizes/checksums.
4. `CheckingService` — `target.service_control(Status)`; record prior state.
5. `Stopping` — `target.service_control(Stop)`; wait until stopped (poll, timeout).
   (systemd: `systemctl stop`; docker: `docker stop`; process: `stop_cmd`.)
6. `BackingUp` — copy current live world set to
   `<worldsPath>/.backups/<slot>.<YYYY-MM-DD_HHMMSS>/` (dated). Record in DB.
7. `Copying` — move staged files into place as `N.world.gzip(.pugbackup)`.
8. `FixingPerms` — `target.fix_perms(worldsPath)` (systemd: `chown steam:steam`;
   docker/process: no-op or kind-specific).
9. `Starting` — `target.service_control(Start)`; wait until running.
10. `CapturingGameId` — `target.stream_logs()` (journalctl / docker logs / file tail),
    regex the game-ID / world-loaded line, with timeout. Store it.
11. `Verifying` — service active + UDP port listening (`ss -ulnp`) + game id captured.
12. `Done`.

**Pull states** (same envelope, different middle)

- `CheckingService` → `Stopping` → `Downloading` (sftp the current `N.world.gzip*` set to
  temp) → `Starting` (restart immediately — server back up fast) → then locally:
  native "save as" (folder or zip) → package → `Done`.
- Rationale: stop only for the instant needed to copy a consistent file, restart right
  away, do PC-side packaging after the server's already back up.

**Rollback**

- If any push step after `BackingUp` fails → restore the dated backup into place,
  `chown`, attempt `Starting`, mark `Failed` with the failing step + reason in DB.
- If `Stopping`/`Starting` times out → surface exact journal lines; never leave service
  in unknown state silently.

Every transition writes an audit row (see §9).

---

## 8. Auto-discovery (the "find it for me" flow)

Runs once on first connect (or manual re-scan), writes results to `profile.discovered`,
and **always shows findings for confirmation before saving** — never silently applied.
This is what makes the app target-agnostic: it answers "how do I stop the server here,
and where are its worlds," rather than the user declaring a target type.

**Step A — identify the OS (informational):**
```bash
cat /etc/os-release        # Debian? Ubuntu? → shown to user, not branched on
```

**Step B — determine ServiceKind (the important probe), in order:**
```bash
# 1) systemd service serving Core Keeper?
systemctl list-units --type=service 2>/dev/null | grep -i core
#    hit → ServiceKind::Systemd { unit }
#    then: systemctl cat <unit> | grep -iE 'ExecStart|WorkingDirectory|User='
#          → launchScript, steamUser

# 2) else a docker container serving it?
docker ps --format '{{.Names}} {{.Image}}' 2>/dev/null | grep -i core
#    hit → ServiceKind::Docker { container }
#    then: docker inspect <container> --format '{{json .Mounts}}'
#          → bind-mount source = worldsPath on host

# 3) else raw process from the launch script?
pgrep -af _launch.sh || pgrep -af CoreKeeperServer
#    hit → ServiceKind::Process { stop_cmd, start_cmd, log_path }
```
(Handles systemd-and-docker-both-present: whichever actually serves Core Keeper wins.)

**Step C — locate worlds path** (per ServiceKind; systemd/process use find):
```bash
find /home -name "*.world.gzip" 2>/dev/null   # report parent dir
```
- 1 hit → "Found worlds here: `…/worlds` — use it?" [Use it] [No]
- >1 hit → list, user selects.
- 0 hits → manual entry.

**Confirmation UI:** "This looks like a **systemd** service `corekeeper` on **Debian 12**;
I'll stop it with `systemctl stop corekeeper` and worlds live at `…/worlds`. Correct?
[Yes] [It's actually…]". On confirm, write `profile.discovered` + audit-log the probe.

---

## 9. Audit log (SQLite)

DB in app-data dir. Append-only in spirit. Secrets never stored (log *which* key/user,
never the key/password).

```sql
CREATE TABLE sessions (
  id INTEGER PRIMARY KEY,
  started_at TEXT NOT NULL,          -- ISO8601
  app_version TEXT,
  profile_id TEXT,
  host TEXT,
  ssh_user TEXT,
  auth_method TEXT                   -- 'key' | 'password'
);

CREATE TABLE events (
  id INTEGER PRIMARY KEY,
  session_id INTEGER REFERENCES sessions(id),
  ts TEXT NOT NULL,
  kind TEXT NOT NULL,                -- app_start, connect, disconnect, discover,
                                     -- push_begin, state_enter, file_op, rollback,
                                     -- push_done, pull_begin, pull_done, error
  state TEXT,                        -- state-machine state when kind=state_enter
  detail_json TEXT,                  -- structured payload
  ok INTEGER                         -- 1 success / 0 failure / NULL n/a
);

CREATE TABLE file_ops (
  id INTEGER PRIMARY KEY,
  event_id INTEGER REFERENCES events(id),
  op TEXT,                           -- upload, download, backup, copy, chown, delete
  src TEXT,
  dst TEXT,
  bytes INTEGER,
  slot INTEGER
);

CREATE TABLE backups (
  id INTEGER PRIMARY KEY,
  session_id INTEGER REFERENCES sessions(id),
  ts TEXT NOT NULL,
  slot INTEGER,
  lxc_path TEXT,                     -- .backups/<slot>.<datetime>/
  bytes INTEGER,
  game_id_before TEXT,
  note TEXT
);
```

Logged from app start: `app_start` → `connect` (host/user/auth) → every `state_enter`
→ each `file_op` → `push_done`/`pull_done` or `error` + `rollback`. "What happened to
world 0 on the 24th" is a single query; a failed swap names the exact step it died on.

---

## 10. Stats dashboard (read-only SSH one-liners)

- **Service:** `systemctl is-active` + `systemctl show -p ActiveEnterTimestamp`.
- **Listening:** `ss -ulnp | grep -i corekeeper` (UDP, default 27015).
- **Resources:** `MemoryCurrent`, CPU from `/proc`, `df -h /home/steam`, uptime.
- **Game:** players / world name parsed from recent journal lines (best-effort; low
  expectations — Core Keeper has no clean query API).
- **Steam build id (stretch):** `steamcmd +app_info_print 1963720` → "update available"
  badge vs installed build.
- **Live log drawer:** raw `journalctl -u <service> -f` stream, pop-open.

Poll cheap stats on an interval; stream the log only while the drawer is open.

---

## 11. Screens

1. **Connection bar** (persistent top): profile dropdown, status dot (state + CPU/mem
   glance), Connect/Disconnect.
2. **World Library / Current**: the live world (slot, size, mtime, captured game id) +
   list of dated backups this app created, with Restore.
3. **Swap panel**: the state-machine stepper — Upload → Stop → Backup → Copy → Perms →
   Start → Capture ID → Verify — live, with rollback surfaced on failure.
4. **Pull panel**: Stop → Download → Restart → Save-as → Package.
5. **Stats dashboard**: service/resources/disk/uptime + Steam badge + log drawer.
6. **Settings**: profiles CRUD, paths, Discover buttons, auth setup (key install helper).

---

## 12. Design direction (for the build, not final CSS)

Subject: a homelab operator's control panel for a survival game — think *server rack
meets game HUD*. It should feel like precise instrumentation, not a consumer SaaS card
grid, and specifically avoid the generic "cream + serif + terracotta" and "black + acid
green" AI-default looks.

- **Palette:** cool slate/graphite base (not pure black), one confident signal color for
  live/healthy state (a phosphor teal or amber — instrument readout, used *only* for
  status/active), plus a distinct danger red reserved strictly for stop/rollback/destroy.
  Muted neutrals everywhere else so status color carries meaning.
- **Type:** a monospace utility face for all machine data (paths, sizes, journal, game
  id) so terminal output feels native; a clean grotesk for UI chrome/labels. The mono is
  the character — it's a tool that talks to a server, lean into that.
- **Signature element:** the **swap stepper** as the memorable object — a horizontal
  "pipeline" of stages that fills/animates as the operation runs, each stage showing its
  live journal line, turning the scary part (destructive remote swap) into something you
  watch with confidence. Spend the boldness here; keep everything else quiet.
- **Copy:** action names stay identical through a flow (the button says "Swap in world",
  the toast says "World swapped"). Errors state what happened + the failing step + how to
  recover, in the tool's voice. Empty states invite action ("No backups yet — your first
  swap creates one.").
- Quality floor: responsive, visible keyboard focus, reduced-motion respected, the log
  stream virtualized.

> **Tailwind v4 note:** config is CSS-first — there is no `tailwind.config.js`. The
> palette/type tokens above go in an `@theme { … }` block in the main CSS file (e.g.
> `--color-signal`, `--font-mono`), consumed as normal utility classes. shadcn/ui writes
> its own CSS variables alongside; keep the instrument palette in `@theme` and let
> shadcn's semantic tokens (background/foreground/primary) map onto it.

---

## 13. Build order

1. Tauri v2 + React/shadcn scaffold; config store; SQLite schema + `app_start` logging.
2. `ServerTarget` trait + `russh` SSH impl (password first); connection profiles +
   dropdown; keyring.
3. Discovery probe (OS + ServiceKind + worlds path) writing `profile.discovered`;
   read-only stats. Safe, no writes — proves the SSH+trait layer end to end.
4. Push flow behind the full state machine, all control via the trait
   (backup + rollback first, then copy).
5. Game-id capture via `stream_logs`.
6. Pull flow.
7. Key-auth + public-key install helper.
8. Docker ServiceKind impl (exec through the same SSH host) — proves the abstraction.
9. Design pass on the stepper; log drawer; Steam badge (stretch).
10. Post-v1 refinements (§15): profile edit/delete UI, persistent current-world
    display, locked connection dropdown while connected, server clock + copyable game id.
11. Terminal + Audit tabs (§16).
12. Cross-tab operation-lock robustness, About dialog, MIT release prep, GitHub Actions
    release workflow, localization (§17).

---

## 14. Open items / to verify during build

- Exact journal line format for the game/world id on load (regex TBD from real logs).
- Whether `_launch.sh` pins a specific slot / world name (read it:
  `cat /home/steam/corekeeper/_launch.sh`) — determines whether a pushed world must be
  renamed to a specific slot to be the one that loads.
- UDP port actually in use (confirm via `ss -ulnp`) for the "listening" check.
- Core Keeper Steam app id for `app_info_print` (verify `1963720`).

---

## 15. Post-v1 refinements (found during real use, 2026-07-25)

All of v1's build order (§13) shipped and was exercised for real against both the
reference systemd LXC and a Docker-hosted instance. These four gaps surfaced from
actually living with the app day-to-day; tracked here so they don't get lost.

- **Profile edit/delete UI** (refines §11 screen 6, Settings). `upsert_profile` and
  `delete_profile` commands existed backend-side since §13 item 1 but were never wired
  to anything beyond "New profile" — there's no way to edit an existing profile's
  host/port/user, or remove one, without hand-editing the config file. Add edit (reuse
  the same dialog, pre-filled) and delete (with confirmation, since it silently drops
  `profile.discovered` too) to the profile dropdown.
- **Persistent current-world visibility** (refines §11 screen 2, World Library /
  Current — not the full backups-list-with-restore version of that screen, just the
  "what's live right now" half of it). Today the captured game id only ever appears
  transiently during a push's `CapturingGameId` step; there's no way to see which world
  is currently loaded outside of running a push/pull. Surface it persistently in the
  Stats panel instead, parsed from the log tail the same way push already does.
- **Lock the connection dropdown while connected** (refines §11 screen 1, Connection
  bar). Switching profiles mid-session was never a supported operation — the dropdown
  should be disabled whenever `status !== "disconnected"`, not just functionally
  ignored.
- **Server clock + copyable game id** (refines §10, Stats dashboard). Add the server's
  current wall-clock time to the status row (distinct from uptime, which only measures
  time since boot) and a copy-to-clipboard affordance next to the game id — using
  `@tauri-apps/plugin-clipboard-manager` rather than the raw browser clipboard API,
  which Tauri webviews don't reliably grant without it.

---

## 16. New screens: Terminal + Audit (requested 2026-07-25)

Two additions to §11's screen list, each a new top-level tab alongside Stats/Push/Pull.

### Terminal

A real interactive shell over the same SSH connection — not another `exec()` call, an
actual PTY session. Architecturally distinct from everything in §13: every SSH
interaction so far has been request/response (`exec` sends one command, waits for exit,
returns output); a terminal is a persistent bidirectional stream.

- Backend: request a PTY + shell on a dedicated channel (`channel.request_pty`,
  `channel.request_shell`). The channel splits into read/write halves, so a background
  task loops on the read half emitting output chunks as Tauri events
  (`terminal-output`), while a `terminal_write` command feeds keystrokes into the write
  half independently; `terminal_resize` forwards window-change events so full-screen
  apps (`vim`, `top`, `less`) redraw correctly. Concurrent with, and independent of, the
  app's normal `exec()` traffic (stats polling, etc.) over the same authenticated
  connection.
- Frontend: needs a real terminal emulator to interpret ANSI escapes, cursor movement,
  and color — a plain `<pre>` can't render a shell session. New dependency: `xterm.js`.
- One terminal session per connection (mirrors §2's "one profile connected at a time");
  closing the tab or disconnecting tears down the channel and background task.
- Security note: this is a raw shell as whatever user the profile connects as (`root` on
  the reference LXC) — the same trust level as `ssh root@host` today, not a new
  privilege, but worth being explicit that it's real and unsandboxed, not a toy.

### Audit

A filterable, searchable view over the SQLite audit log (§9's `sessions`/`events`/
`file_ops`/`backups` tables), which has been fully populated since the very first build
phase but has no read path yet — `audit.rs` is write-only so far.

- New Rust query commands (kept typed and in Rust, consistent with "all real logic in
  Rust" — not raw SQL handed to the frontend via `tauri-plugin-sql`'s generic JS
  `select()`): filter by `kind`, `session_id`, `ok`/failed, and a date range; free-text
  search over `state` and `detail_json`. Paginated.
- UI: a results table (timestamp, kind, state, ok/fail, detail) with filter controls
  above it and a search box; click a row to expand full `detail_json`.

---

## 17. Localization (requested 2026-07-25)

A cycle-on-click icon toggle (globe/languages icon, next to About/Theme in
`ConnectionBar`'s header) — mirrors `ThemeToggle`'s system → light → dark pattern rather
than a dropdown. Cycle: **System → English → Français → (any further installed
languages)**. "System" matches the OS/browser locale (`navigator.languages`) against
what's installed, falling back to English; the explicit choice (or "system") persists
in `localStorage` and switches in-place with no reload.

- **Stack**: `react-i18next` — the standard choice for this, not a bespoke solution.
  Resources are bundled at build time, but *discovered* rather than hand-listed:
  `src/lib/i18n.ts` uses `import.meta.glob("../locales/*.json", { eager: true })` to
  pick up every file in `src/locales/` automatically. Adding a language later (e.g.
  Spanish) is just "drop `es.json` in that folder, with its own `meta.nativeName` key,
  and rebuild" — no array or component to touch, it just joins the cycle.
  `src/i18next.d.ts` augments react-i18next's types against `en.json`'s literal shape,
  so a typo'd or missing translation key is a compile error, not a silent runtime
  fallback (this only validates key *names*, not that every language has a translation
  for each one — a missing key in a non-English file just falls back to English text at
  runtime).
- Each locale file self-declares its own name under `meta.nativeName` (e.g. `fr.json`:
  `"Français"`), read via `i18n.getResource(lang, ...)` rather than `t()` — so the
  toggle always shows a language's name *in itself*, never translated into whatever's
  currently active, and a new language's label ships inside its own file.
- **Scope — what gets translated and what doesn't**:
  - Translated: all static UI chrome — labels, buttons, dialog titles/descriptions,
    toasts, table headers, placeholders — across every panel.
  - Left in English, deliberately: raw backend error strings (`anyhow` chains, SSH/
    `docker`/`ss` command output surfaced in error messages) and all audit log content.
    These are diagnostic/forensic text; translating dynamic error text isn't reliably
    achievable, and an audit trail that flips language depending on whatever locale was
    active when a row was written would be harder to read back, not easier.
  - Domain terms (SSH, Docker, systemd, world, slot) are left untranslated in French
    too, matching how the rest of the industry handles this class of technical term —
    translating them would make the app less recognizable, not more.
- **Structure**: one flat JSON file per language, keys nested by component
  (`push.stepper.upload`, `connectionBar.newProfileDialog.titleNew`, etc.) rather than
  i18next's namespace-per-file convention — the app is small enough that splitting
  namespaces now would be premature; revisit if a file gets unwieldy.
- Caveat worth recording: the French translation is an AI-assisted best-effort pass, not
  reviewed by a native speaker — solid for a v1, but worth a native-speaker pass before
  calling it finished.
