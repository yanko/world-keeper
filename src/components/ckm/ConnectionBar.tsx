import { useState } from "react";
import { toast } from "sonner";

import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Input } from "@/components/ui/input";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import type { AppConfig, AuthMethod, Profile } from "@/lib/api";
import * as api from "@/lib/api";
import { ThemeToggle } from "@/components/ckm/ThemeToggle";

export type ConnectionStatus =
  | "disconnected"
  | "connecting"
  | "connected-raw"
  | "connected-target";

interface ConnectionBarProps {
  config: AppConfig;
  selectedProfileId: string | null;
  onSelectProfile: (id: string) => void;
  onConfigChanged: (config: AppConfig) => void;
  status: ConnectionStatus;
  onConnect: (profileId: string, password?: string) => Promise<void>;
  onDisconnect: () => void;
}

const emptyDraft = { name: "", host: "", port: "22", user: "root", auth: "password" as AuthMethod };

export function ConnectionBar({
  config,
  selectedProfileId,
  onSelectProfile,
  onConfigChanged,
  status,
  onConnect,
  onDisconnect,
}: ConnectionBarProps) {
  const [addOpen, setAddOpen] = useState(false);
  const [draft, setDraft] = useState(emptyDraft);
  const [pwOpen, setPwOpen] = useState(false);
  const [password, setPassword] = useState("");
  const [connecting, setConnecting] = useState(false);
  const [keyAuthOpen, setKeyAuthOpen] = useState(false);
  const [settingUpKeyAuth, setSettingUpKeyAuth] = useState(false);

  const selectedProfile = config.profiles.find((p) => p.id === selectedProfileId) ?? null;

  async function submitNewProfile() {
    if (!draft.name || !draft.host || !draft.user) {
      toast.error("Name, host, and user are required.");
      return;
    }
    const profile: Profile = {
      id: crypto.randomUUID(),
      name: draft.name,
      host: draft.host,
      port: Number(draft.port) || 22,
      user: draft.user,
      auth: draft.auth,
      savePassword: false,
      discovered: null,
    };
    try {
      const cfg = await api.upsertProfile(profile);
      onConfigChanged(cfg);
      onSelectProfile(profile.id);
      setAddOpen(false);
      setDraft(emptyDraft);
      toast.success(`Profile "${profile.name}" added.`);
    } catch (e) {
      toast.error(String(e));
    }
  }

  async function startConnect() {
    if (!selectedProfile) return;
    if (selectedProfile.auth === "key") {
      setConnecting(true);
      try {
        await onConnect(selectedProfile.id);
      } catch (e) {
        toast.error(String(e));
      } finally {
        setConnecting(false);
      }
      return;
    }
    setPwOpen(true);
  }

  async function submitPassword() {
    if (!selectedProfile) return;
    setConnecting(true);
    try {
      await onConnect(selectedProfile.id, password);
      setPwOpen(false);
      setPassword("");
    } catch (e) {
      toast.error(String(e));
    } finally {
      setConnecting(false);
    }
  }

  async function setUpKeyAuth() {
    setKeyAuthOpen(false);
    setSettingUpKeyAuth(true);
    try {
      await api.setupKeyAuth();
      const cfg = await api.getConfig();
      onConfigChanged(cfg);
      toast.success("Key auth set up — future connects won't need the password.");
    } catch (e) {
      toast.error(String(e));
    } finally {
      setSettingUpKeyAuth(false);
    }
  }

  const statusLabel: Record<ConnectionStatus, string> = {
    disconnected: "Disconnected",
    connecting: "Connecting…",
    "connected-raw": "Connected (needs discovery)",
    "connected-target": "Connected",
  };
  const statusVariant: Record<ConnectionStatus, "secondary" | "default" | "destructive"> = {
    disconnected: "secondary",
    connecting: "secondary",
    "connected-raw": "secondary",
    "connected-target": "default",
  };

  return (
    <div className="flex flex-wrap items-center gap-3 border-b bg-card px-4 py-3">
      <Select
        value={selectedProfileId ?? undefined}
        onValueChange={(v) => v && onSelectProfile(v)}
      >
        <SelectTrigger className="max-w-full min-w-40">
          <SelectValue placeholder="Select a profile…">
            {(value: string | null) =>
              config.profiles.find((p) => p.id === value)?.name ?? "Select a profile…"
            }
          </SelectValue>
        </SelectTrigger>
        <SelectContent className="w-auto min-w-72" alignItemWithTrigger={false}>
          {config.profiles.map((p) => (
            <SelectItem key={p.id} value={p.id}>
              {p.name}{" "}
              <span className="font-mono text-muted-foreground">
                ({p.user}@{p.host})
              </span>
            </SelectItem>
          ))}
        </SelectContent>
      </Select>

      <Button variant="outline" size="sm" onClick={() => setAddOpen(true)}>
        New profile
      </Button>

      {status === "disconnected" ? (
        <Button
          size="sm"
          disabled={!selectedProfile}
          onClick={startConnect}
        >
          Connect
        </Button>
      ) : (
        <Button size="sm" variant="destructive" onClick={onDisconnect}>
          Disconnect
        </Button>
      )}

      <Badge variant={statusVariant[status]}>{statusLabel[status]}</Badge>

      {status !== "disconnected" && selectedProfile?.auth === "password" && (
        <Button
          variant="outline"
          size="sm"
          disabled={settingUpKeyAuth}
          onClick={() => setKeyAuthOpen(true)}
        >
          {settingUpKeyAuth ? "Setting up key auth…" : "Set up key auth"}
        </Button>
      )}

      <div className="ml-auto">
        <ThemeToggle />
      </div>

      {/* New profile dialog */}
      <Dialog open={addOpen} onOpenChange={setAddOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>New connection profile</DialogTitle>
            <DialogDescription>
              SSH connection details. New profiles start on password auth — once
              connected, use "Set up key auth" to switch to a key so you won't need to
              retype the password.
            </DialogDescription>
          </DialogHeader>
          <div className="grid gap-3">
            <Input
              placeholder="Name (e.g. Core Keeper LXC)"
              value={draft.name}
              onChange={(e) => setDraft((d) => ({ ...d, name: e.target.value }))}
            />
            <div className="flex gap-2">
              <Input
                placeholder="Host"
                value={draft.host}
                onChange={(e) => setDraft((d) => ({ ...d, host: e.target.value }))}
              />
              <Input
                placeholder="Port"
                className="w-20"
                value={draft.port}
                onChange={(e) => setDraft((d) => ({ ...d, port: e.target.value }))}
              />
            </div>
            <Input
              placeholder="User (e.g. root)"
              value={draft.user}
              onChange={(e) => setDraft((d) => ({ ...d, user: e.target.value }))}
            />
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setAddOpen(false)}>
              Cancel
            </Button>
            <Button onClick={submitNewProfile}>Add profile</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* Password prompt */}
      <Dialog open={pwOpen} onOpenChange={setPwOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Connect to {selectedProfile?.name}</DialogTitle>
            <DialogDescription>
              Password for {selectedProfile?.user}@{selectedProfile?.host}. Never stored
              unless you later enable "save password".
            </DialogDescription>
          </DialogHeader>
          <Input
            type="password"
            autoFocus
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") submitPassword();
            }}
          />
          <DialogFooter>
            <Button variant="outline" onClick={() => setPwOpen(false)}>
              Cancel
            </Button>
            <Button onClick={submitPassword} disabled={connecting || !password}>
              {connecting ? "Connecting…" : "Connect"}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* Key auth setup confirm */}
      <Dialog open={keyAuthOpen} onOpenChange={setKeyAuthOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Set up key auth for {selectedProfile?.name}?</DialogTitle>
            <DialogDescription>
              Generates a new SSH key (kept locally, never sent anywhere but this host's
              <span className="font-mono"> ~/.ssh/authorized_keys</span>) and switches
              this profile to key auth. You won't need to type the password again.
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setKeyAuthOpen(false)}>
              Cancel
            </Button>
            <Button onClick={setUpKeyAuth}>Generate &amp; install key</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
