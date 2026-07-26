import { useState } from "react";
import { toast } from "sonner";
import { Trans, useTranslation } from "react-i18next";
import { PencilIcon, Trash2Icon } from "lucide-react";

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
import { useOperations } from "@/lib/operations";
import { ThemeToggle } from "@/components/ckm/ThemeToggle";
import { AboutDialog } from "@/components/ckm/AboutDialog";
import { LanguageToggle } from "@/components/ckm/LanguageToggle";

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
  const { t } = useTranslation();
  const [profileDialogOpen, setProfileDialogOpen] = useState(false);
  const [editingProfile, setEditingProfile] = useState<Profile | null>(null);
  const [draft, setDraft] = useState(emptyDraft);
  const [deleteOpen, setDeleteOpen] = useState(false);
  const [pwOpen, setPwOpen] = useState(false);
  const [password, setPassword] = useState("");
  const [connecting, setConnecting] = useState(false);
  const [keyAuthOpen, setKeyAuthOpen] = useState(false);
  const [settingUpKeyAuth, setSettingUpKeyAuth] = useState(false);
  const { activeOperation } = useOperations();

  const selectedProfile = config.profiles.find((p) => p.id === selectedProfileId) ?? null;
  const canEditOrDelete = status === "disconnected" && !!selectedProfile;

  function openNewProfileDialog() {
    setEditingProfile(null);
    setDraft(emptyDraft);
    setProfileDialogOpen(true);
  }

  function openEditProfileDialog() {
    if (!selectedProfile) return;
    setEditingProfile(selectedProfile);
    setDraft({
      name: selectedProfile.name,
      host: selectedProfile.host,
      port: String(selectedProfile.port),
      user: selectedProfile.user,
      auth: selectedProfile.auth,
    });
    setProfileDialogOpen(true);
  }

  async function submitProfile() {
    if (!draft.name || !draft.host || !draft.user) {
      toast.error(t("connectionBar.toasts.requiredError"));
      return;
    }
    // Editing preserves everything the simple form doesn't show (auth/keyPath/
    // savePassword/discovered) — upsert replaces the whole stored profile, so
    // starting from the original object (not a fresh one) matters here.
    const profile: Profile = editingProfile
      ? { ...editingProfile, name: draft.name, host: draft.host, port: Number(draft.port) || 22, user: draft.user }
      : {
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
      setProfileDialogOpen(false);
      toast.success(
        editingProfile
          ? t("connectionBar.toasts.profileUpdated", { name: profile.name })
          : t("connectionBar.toasts.profileAdded", { name: profile.name })
      );
    } catch (e) {
      toast.error(String(e));
    }
  }

  async function deleteSelectedProfile() {
    if (!selectedProfile) return;
    setDeleteOpen(false);
    try {
      const cfg = await api.deleteProfile(selectedProfile.id);
      onConfigChanged(cfg);
      const fallback = cfg.profiles.find((p) => p.id !== selectedProfile.id);
      if (fallback) onSelectProfile(fallback.id);
      toast.success(t("connectionBar.toasts.profileDeleted", { name: selectedProfile.name }));
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
      toast.success(t("connectionBar.toasts.keyAuthSuccess"));
    } catch (e) {
      toast.error(String(e));
    } finally {
      setSettingUpKeyAuth(false);
    }
  }

  const statusLabel: Record<ConnectionStatus, string> = {
    disconnected: t("connectionBar.status.disconnected"),
    connecting: t("connectionBar.status.connecting"),
    "connected-raw": t("connectionBar.status.connectedRaw"),
    "connected-target": t("connectionBar.status.connectedTarget"),
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
        disabled={status !== "disconnected"}
      >
        <SelectTrigger className="max-w-full min-w-40">
          <SelectValue placeholder={t("connectionBar.selectProfilePlaceholder")}>
            {(value: string | null) =>
              config.profiles.find((p) => p.id === value)?.name ?? t("connectionBar.selectProfilePlaceholder")
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

      <Button
        variant="outline"
        size="sm"
        disabled={status !== "disconnected"}
        onClick={openNewProfileDialog}
      >
        {t("connectionBar.newProfile")}
      </Button>

      <Button
        variant="outline"
        size="icon-sm"
        disabled={!canEditOrDelete}
        onClick={openEditProfileDialog}
        title={t("connectionBar.editProfile")}
      >
        <PencilIcon />
      </Button>

      <Button
        variant="outline"
        size="icon-sm"
        disabled={!canEditOrDelete}
        onClick={() => setDeleteOpen(true)}
        title={t("connectionBar.deleteProfile")}
      >
        <Trash2Icon />
      </Button>

      {status === "disconnected" ? (
        <Button
          size="sm"
          disabled={!selectedProfile}
          onClick={startConnect}
        >
          {t("connectionBar.connect")}
        </Button>
      ) : (
        <Button
          size="sm"
          variant="destructive"
          disabled={activeOperation !== null}
          title={
            activeOperation
              ? t("connectionBar.disconnectBusyTitle", { operation: t(`operations.${activeOperation}`) })
              : undefined
          }
          onClick={onDisconnect}
        >
          {t("connectionBar.disconnect")}
        </Button>
      )}

      <Badge variant={statusVariant[status]}>{statusLabel[status]}</Badge>

      {activeOperation && (
        <Badge variant="secondary" className="animate-pulse">
          {t("connectionBar.operationInProgress", { operation: t(`operations.${activeOperation}`) })}
        </Badge>
      )}

      {status !== "disconnected" && selectedProfile?.auth === "password" && (
        <Button
          variant="outline"
          size="sm"
          disabled={settingUpKeyAuth}
          onClick={() => setKeyAuthOpen(true)}
        >
          {settingUpKeyAuth ? t("connectionBar.settingUpKeyAuth") : t("connectionBar.setUpKeyAuth")}
        </Button>
      )}

      <div className="ml-auto flex items-center gap-2">
        <AboutDialog />
        <LanguageToggle />
        <ThemeToggle />
      </div>

      {/* New/edit profile dialog */}
      <Dialog open={profileDialogOpen} onOpenChange={setProfileDialogOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>
              {editingProfile
                ? t("connectionBar.newProfileDialog.titleEdit", { name: editingProfile.name })
                : t("connectionBar.newProfileDialog.titleNew")}
            </DialogTitle>
            <DialogDescription>
              {editingProfile
                ? t("connectionBar.newProfileDialog.descEdit")
                : t("connectionBar.newProfileDialog.descNew")}
            </DialogDescription>
          </DialogHeader>
          <div className="grid gap-3">
            <Input
              placeholder={t("connectionBar.newProfileDialog.namePlaceholder")}
              value={draft.name}
              onChange={(e) => setDraft((d) => ({ ...d, name: e.target.value }))}
            />
            <div className="flex gap-2">
              <Input
                placeholder={t("connectionBar.newProfileDialog.hostPlaceholder")}
                value={draft.host}
                onChange={(e) => setDraft((d) => ({ ...d, host: e.target.value }))}
              />
              <Input
                placeholder={t("connectionBar.newProfileDialog.portPlaceholder")}
                className="w-20"
                value={draft.port}
                onChange={(e) => setDraft((d) => ({ ...d, port: e.target.value }))}
              />
            </div>
            <Input
              placeholder={t("connectionBar.newProfileDialog.userPlaceholder")}
              value={draft.user}
              onChange={(e) => setDraft((d) => ({ ...d, user: e.target.value }))}
            />
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setProfileDialogOpen(false)}>
              {t("connectionBar.newProfileDialog.cancel")}
            </Button>
            <Button onClick={submitProfile}>
              {editingProfile
                ? t("connectionBar.newProfileDialog.save")
                : t("connectionBar.newProfileDialog.add")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* Delete profile confirm */}
      <Dialog open={deleteOpen} onOpenChange={setDeleteOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t("connectionBar.deleteDialog.title", { name: selectedProfile?.name })}</DialogTitle>
            <DialogDescription>{t("connectionBar.deleteDialog.desc")}</DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setDeleteOpen(false)}>
              {t("connectionBar.deleteDialog.cancel")}
            </Button>
            <Button variant="destructive" onClick={deleteSelectedProfile}>
              {t("connectionBar.deleteDialog.confirm")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* Password prompt */}
      <Dialog open={pwOpen} onOpenChange={setPwOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t("connectionBar.passwordDialog.title", { name: selectedProfile?.name })}</DialogTitle>
            <DialogDescription>
              {t("connectionBar.passwordDialog.desc", {
                user: selectedProfile?.user,
                host: selectedProfile?.host,
              })}
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
              {t("connectionBar.passwordDialog.cancel")}
            </Button>
            <Button onClick={submitPassword} disabled={connecting || !password}>
              {connecting ? t("connectionBar.passwordDialog.connecting") : t("connectionBar.passwordDialog.connect")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* Key auth setup confirm */}
      <Dialog open={keyAuthOpen} onOpenChange={setKeyAuthOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t("connectionBar.keyAuthDialog.title", { name: selectedProfile?.name })}</DialogTitle>
            <DialogDescription>
              <Trans
                i18nKey="connectionBar.keyAuthDialog.desc"
                components={{ 1: <span className="font-mono" /> }}
              />
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setKeyAuthOpen(false)}>
              {t("connectionBar.keyAuthDialog.cancel")}
            </Button>
            <Button onClick={setUpKeyAuth}>{t("connectionBar.keyAuthDialog.confirm")}</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
