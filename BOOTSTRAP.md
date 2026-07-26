# BOOTSTRAP — create the project, then hand off to Claude Code

Run these on your **Windows 11** machine (PowerShell). Prereqs: Node LTS, **pnpm**
(`npm install -g pnpm`, or `corepack enable pnpm`), Rust (`rustup`), and the Tauri v2
prerequisites for Windows (WebView2 is preinstalled on Win 11; you need the MSVC C++
build tools — `rustup` will prompt, or install "Desktop development with C++" from the
VS Build Tools).

> **pnpm + Tauri note:** pnpm is fully supported and the official Tauri examples use it.
> Start on pnpm's default (isolated/symlinked) linker. *Only* if you hit a module-
> resolution / tsc error, add a `.npmrc` at the repo root with `node-linker=hoisted`
> (flat `node_modules`, like npm). Don't do this pre-emptively — it discards pnpm's disk
> savings. On Windows pnpm already uses command shims automatically, so no exec-symlink
> config is needed.

## 1. Scaffold a Tauri v2 + React + TS app

```powershell
# creates ./ck-world-manager with a Tauri v2 + React/TypeScript template
pnpm create tauri-app@latest ck-world-manager --template react-ts --manager pnpm

cd ck-world-manager
pnpm install
```

When the interactive prompts appear, choose: **TypeScript / React**, package manager
**pnpm**. (The `--template react-ts` flag preselects most of this.)

## 2. Add Tailwind CSS **v4** + shadcn/ui

Tailwind v4 dropped the old `init -p` flow entirely — **no `tailwind.config.js`, no
`postcss.config.js`, no autoprefixer** (they're built into the official Vite plugin).
Setup is: install two packages, add one Vite plugin line, add one CSS import.

```powershell
# Tailwind v4 + its official Vite plugin, plus node types for the @ alias
pnpm add tailwindcss @tailwindcss/vite
pnpm add -D @types/node
```

**2a. Wire the Vite plugin** — edit `vite.config.ts`:

```ts
import { defineConfig } from "vite"
import react from "@vitejs/plugin-react"
import tailwindcss from "@tailwindcss/vite"
import path from "path"

export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: { "@": path.resolve(__dirname, "./src") },   // shadcn needs this
  },
  // Tauri expects a fixed port and ignores src-tauri while watching:
  clearScreen: false,
  server: { port: 1420, strictPort: true, watch: { ignored: ["**/src-tauri/**"] } },
})
```

**2b. Replace `src/index.css`** (or your main CSS) with the v4 one-liner import + a
theme block. This is where all Tailwind config now lives (CSS-first):

```css
@import "tailwindcss";

/* design tokens go here in v4 — e.g. the instrument palette from SPEC §12 */
@theme {
  /* --color-signal: oklch(...);  danger, mono font, etc. added later */
}
```

**2c. Add the `@` alias to `tsconfig.json`** (shadcn reads this):

```jsonc
{
  "compilerOptions": {
    "baseUrl": ".",
    "paths": { "@/*": ["./src/*"] }
  }
}
```
(If your project split configs, also mirror `baseUrl`/`paths` into `tsconfig.app.json`.)

**2d. Init shadcn/ui and add components:**

```powershell
pnpm dlx shadcn@latest init      # style: new-york (default is deprecated); base color: slate; CSS variables: yes
pnpm dlx shadcn@latest add button card dialog select input badge tabs sonner
```

> In `components.json`, the `tailwind.config` path is left **blank** for Tailwind v4 —
> the shadcn CLI handles this automatically during init. Don't create a `tailwind.config.js`.

## 3. Add the Tauri plugins (config store, SQLite, keyring will be a Rust crate)

```powershell
pnpm tauri add store
pnpm tauri add sql
```

Then add the Rust deps. **Don't hand-pin versions** — let `cargo add` resolve the latest
compatible releases at build time (the SSH crates move fast; `russh` was 0.58 as of this
writing and `russh-keys` has been folded into `russh` itself). Run inside `src-tauri/`:

```powershell
cd src-tauri
cargo add russh russh-sftp
cargo add keyring
cargo add tokio --features full
cargo add serde --features derive
cargo add serde_json anyhow zip
cargo add chrono --features serde
cd ..
```

> **`russh` crypto backend gotcha:** `russh` requires a crypto backend feature — enable
> **one** of `aws-lc-rs` (default, faster) or `ring`, or it won't compile. `cargo add
> russh` pulls the default, which is fine; just know that's why a bare/no-default-features
> add would fail.
> **Key loading:** on current `russh`, key parsing/loading lives in the main `russh`
> crate (the old separate `russh-keys` is merged), so you import key helpers from `russh`,
> not a standalone crate. Claude Code should follow the version it actually resolves.

## 4. Drop in the planning docs

Copy `SPEC.md` and `CONTEXT.md` into the repo root (next to `package.json`), then:

```powershell
git init
git add .
git commit -m "scaffold + spec + context"
```

## 5. Verify it runs

```powershell
pnpm tauri dev
```

You should get a native window with the default Tauri template. Once that's confirmed,
open the folder in **Claude Code** and start with the tasks in `CONTEXT.md` →
"Suggested first tasks".

---

### Handoff prompt to paste into Claude Code

> I'm building a Tauri v2 desktop app to manage a Core Keeper dedicated server's worlds
> over SSH. Read `SPEC.md` and `CONTEXT.md` in the repo root — they contain the full
> design, the real server facts, and the reasoning. Start with the "Suggested first
> tasks" in CONTEXT.md: wire up the config store, SQLite audit, and keyring, then define
> the `ServerTarget` trait + `ServiceKind` enum and implement the `russh` SSH transport
> with a password connect + `exec` against `root@192.168.1.50`, then the discovery probe.
> Keep all target-specific logic inside the SSH+ServiceKind impl; the state machine stays
> target-agnostic. Ask me to run any server-side commands you need output from (e.g. the
> `_launch.sh` contents and the journal line for the game id).
