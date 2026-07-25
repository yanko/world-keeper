// Typed wrappers around the Tauri commands in src-tauri/src/commands.rs. Field casing
// mirrors the Rust structs' `#[serde(rename_all = "camelCase")]` output exactly.

import { invoke } from "@tauri-apps/api/core";

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
  udpListening: string[];
  memTotalKb: number | null;
  memAvailableKb: number | null;
  diskSize: string | null;
  diskUsed: string | null;
  diskAvail: string | null;
  diskUsePct: string | null;
  uptimeSeconds: number | null;
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

export const connect = (profileId: string, password: string) =>
  invoke<ConnectResult>("connect", { profileId, password });

export const disconnect = () => invoke<void>("disconnect");

export const runDiscovery = () => invoke<DiscoveryResult>("run_discovery");

export const confirmDiscovery = (profileId: string, discovered: Discovered) =>
  invoke<AppConfig>("confirm_discovery", { profileId, discovered });

// ---- stats -----------------------------------------------------------------

export const getStats = () => invoke<ServerStats>("get_stats");
