// Typed wrappers around the Tauri commands in src-tauri/src/commands.rs. Field casing
// mirrors the Rust structs' `#[serde(rename_all = "camelCase")]` output exactly.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { open, save } from "@tauri-apps/plugin-dialog";

export type AuthMethod = "key" | "password";

export type ServiceKind =
  | { type: "systemd"; unit: string }
  | { type: "docker"; container: string }
  | { type: "process"; stopCmd: string; startCmd: string; logPath: string | null };

export interface Discovered {
  os: string | null;
  serviceKind: ServiceKind | null;
  worldsPath: string | null;
  steamUser: string | null;
  launchScript: string | null;
  probedAt: string;
}

export interface Profile {
  id: string;
  name: string;
  host: string;
  port: number;
  user: string;
  auth: AuthMethod;
  keyPath?: string | null;
  savePassword: boolean;
  discovered?: Discovered | null;
}

export interface WorldDetection {
  patterns: string[];
}

export interface AppConfig {
  profiles: Profile[];
  worldDetection: WorldDetection;
  activeProfileId: string | null;
}

export interface ServiceKindCandidate {
  kind: ServiceKind;
  execStart: string | null;
  workingDirectory: string | null;
  steamUser: string | null;
}

export interface DiscoveryResult {
  os: string | null;
  serviceKind: ServiceKind | null;
  serviceKindCandidates: ServiceKindCandidate[];
  worldsPath: string | null;
  worldsPathCandidates: string[];
  steamUser: string | null;
  launchScript: string | null;
  probedAt: string;
}

export interface ConnectResult {
  profileId: string;
  needsDiscovery: boolean;
}

export interface ServerStats {
  serviceStatus: string;
  activeSince: string | null;
  serverTime: string | null;
  udpListening: string[];
  memTotalKb: number | null;
  memAvailableKb: number | null;
  diskSize: string | null;
  diskUsed: string | null;
  diskAvail: string | null;
  diskUsePct: string | null;
  uptimeSeconds: number | null;
  gameId: string | null;
  recentLogTail: string;
}

// ---- config -----------------------------------------------------------------

export const getConfig = () => invoke<AppConfig>("get_config");

export const upsertProfile = (profile: Profile) =>
  invoke<AppConfig>("upsert_profile", { profile });

export const deleteProfile = (profileId: string) =>
  invoke<AppConfig>("delete_profile", { profileId });

export const setActiveProfile = (profileId: string) =>
  invoke<AppConfig>("set_active_profile", { profileId });

// ---- secrets -----------------------------------------------------------------

export const savePassword = (profileId: string, password: string) =>
  invoke<void>("save_password", { profileId, password });

export const hasSavedPassword = (profileId: string) =>
  invoke<boolean>("has_saved_password", { profileId });

export const deleteSavedPassword = (profileId: string) =>
  invoke<void>("delete_saved_password", { profileId });

// ---- connection / discovery ---------------------------------------------------

export const connect = (profileId: string, password?: string) =>
  invoke<ConnectResult>("connect", { profileId, password });

export const disconnect = () => invoke<void>("disconnect");

/** SPEC §6's one-time setup helper: generates a keypair, installs it on the currently
 * connected host, and switches the profile to key auth. Requires an active connection. */
export const setupKeyAuth = () => invoke<Profile>("setup_key_auth");

export const runDiscovery = () => invoke<DiscoveryResult>("run_discovery");

export const confirmDiscovery = (profileId: string, discovered: Discovered) =>
  invoke<AppConfig>("confirm_discovery", { profileId, discovered });

// ---- stats -----------------------------------------------------------------

export const getStats = () => invoke<ServerStats>("get_stats");

/** Just the log tail — used by the live log drawer so watching it doesn't also re-poll
 * mem/disk/uptime on every tick. */
export const getLogTail = (lines: number) => invoke<string>("get_log_tail", { lines });

// ---- push --------------------------------------------------------------------

export interface LocalWorldCandidate {
  slot: number;
  mainPath: string;
  mainSize: number;
  mainModified: number | null;
  pugbackupPath: string | null;
  pugbackupSize: number | null;
}

// A plain fieldless enum on the Rust side serializes as just its (camelCase) tag.
export type PushState =
  | "uploading"
  | "checkingService"
  | "stopping"
  | "backingUp"
  | "copying"
  | "fixingPerms"
  | "starting"
  | "capturingGameId"
  | "verifying"
  | "done"
  | "rollingBack"
  | "failed";

export interface PushProgress {
  state: PushState;
  detail: string | null;
  ok: boolean | null;
}

export interface PushResult {
  slot: number;
  gameId: string | null;
  bytesUploaded: number;
  backupPath: string | null;
  udpListening: boolean;
}

/** Native "pick a file" dialog, scoped to zip/world files. */
export const pickWorldFile = () =>
  open({
    multiple: false,
    filters: [{ name: "Core Keeper world", extensions: ["zip", "gzip"] }],
  });

/** Native "pick a folder" dialog. */
export const pickWorldFolder = () => open({ multiple: false, directory: true });

export const resolveLocalWorld = (path: string) =>
  invoke<LocalWorldCandidate[]>("resolve_local_world", { path });

export const pushWorld = (candidate: LocalWorldCandidate) =>
  invoke<PushResult>("push_world", { candidate });

/** Subscribes to live progress from the running push envelope. Call before `pushWorld`
 * so no early events are missed; the returned function unsubscribes. */
export const onPushProgress = (cb: (p: PushProgress) => void): Promise<UnlistenFn> =>
  listen<PushProgress>("push-progress", (event) => cb(event.payload));

// ---- pull --------------------------------------------------------------------

export interface RemoteWorldInfo {
  slot: number;
  size: number;
  modified: number | null;
}

export type PullState = "checkingService" | "stopping" | "downloading" | "starting" | "done" | "failed";

export interface PullProgress {
  state: PullState;
  detail: string | null;
  ok: boolean | null;
}

export interface PulledWorld {
  slot: number;
  tempDir: string;
  mainPath: string;
  pugbackupPath: string | null;
  bytes: number;
}

export const listRemoteWorlds = () => invoke<RemoteWorldInfo[]>("list_remote_worlds");

export const pullWorld = (slot: number) => invoke<PulledWorld>("pull_world", { slot });

export const packagePulledWorld = (pulled: PulledWorld, destination: string, asZip: boolean) =>
  invoke<string>("package_pulled_world", { pulled, destination, asZip });

export const onPullProgress = (cb: (p: PullProgress) => void): Promise<UnlistenFn> =>
  listen<PullProgress>("pull-progress", (event) => cb(event.payload));

/** Native "save as .zip" dialog, pre-filled with a sensible file name. */
export const saveWorldZip = (defaultName: string) =>
  save({ defaultPath: defaultName, filters: [{ name: "Zip", extensions: ["zip"] }] });

/** Native "pick a destination folder" dialog (for saving as a folder, not a zip). */
export const pickSaveFolder = () => open({ multiple: false, directory: true });

// ---- audit -------------------------------------------------------------------

export interface EventRow {
  id: number;
  sessionId: number | null;
  ts: string;
  kind: string;
  state: string | null;
  detailJson: unknown | null;
  ok: boolean | null;
}

export interface EventFilter {
  kind?: string;
  sessionId?: number;
  ok?: boolean;
  search?: string;
  since?: string;
  until?: string;
  limit?: number;
  offset?: number;
}

export interface SessionRow {
  id: number;
  startedAt: string;
  appVersion: string | null;
  profileId: string | null;
  host: string | null;
  sshUser: string | null;
  authMethod: string | null;
}

export const queryAuditEvents = (filter: EventFilter) =>
  invoke<EventRow[]>("query_audit_events", { filter });

export const listAuditEventKinds = () => invoke<string[]>("list_audit_event_kinds");

export const listAuditSessions = () => invoke<SessionRow[]>("list_audit_sessions");

// ---- terminal ------------------------------------------------------------------

export const terminalOpen = () => invoke<void>("terminal_open");

export const terminalWrite = (data: string) => invoke<void>("terminal_write", { data });

export const terminalResize = (cols: number, rows: number) =>
  invoke<void>("terminal_resize", { cols, rows });

export const terminalClose = () => invoke<void>("terminal_close");

export const onTerminalOutput = (cb: (chunk: string) => void): Promise<UnlistenFn> =>
  listen<string>("terminal-output", (event) => cb(event.payload));

export const onTerminalClosed = (cb: () => void): Promise<UnlistenFn> =>
  listen<void>("terminal-closed", () => cb());
