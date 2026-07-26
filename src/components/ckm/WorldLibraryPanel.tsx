import { useEffect, useState } from "react";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Stepper, type StepDef } from "@/components/ckm/Stepper";
import { useOperations } from "@/lib/operations";
import type { BackupRow, RemoteWorldInfo, RestoreState } from "@/lib/api";
import * as api from "@/lib/api";
import { formatBytes, formatMtime } from "@/lib/format";

function formatTs(ts: string): string {
  const d = new Date(ts);
  return Number.isNaN(d.getTime()) ? ts : d.toLocaleString();
}

export function WorldLibraryPanel() {
  const { t } = useTranslation();
  const { activeOperation, restore, startRestore, resetRestore } = useOperations();
  const [worlds, setWorlds] = useState<RemoteWorldInfo[]>([]);
  const [backups, setBackups] = useState<BackupRow[]>([]);
  const [loading, setLoading] = useState(false);
  const [target, setTarget] = useState<BackupRow | null>(null);
  const [confirmOpen, setConfirmOpen] = useState(false);

  const RESTORE_STEPS: StepDef<RestoreState>[] = [
    { state: "checkingService", label: t("worldLibrary.stepper.checkService") },
    { state: "stopping", label: t("worldLibrary.stepper.stop") },
    { state: "backingUp", label: t("worldLibrary.stepper.backUp") },
    { state: "copying", label: t("worldLibrary.stepper.copyIn") },
    { state: "fixingPerms", label: t("worldLibrary.stepper.fixPerms") },
    { state: "starting", label: t("worldLibrary.stepper.start") },
    { state: "capturingGameId", label: t("worldLibrary.stepper.captureGameId") },
    { state: "verifying", label: t("worldLibrary.stepper.verify") },
  ];

  const globalBusy = activeOperation !== null;

  async function refresh() {
    setLoading(true);
    try {
      const [w, b] = await Promise.all([api.listRemoteWorlds(), api.listBackups()]);
      setWorlds(w);
      setBackups(b);
    } catch (e) {
      toast.error(String(e));
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    refresh();
    // Re-list once a restore finishes — a new safety backup was just created, and the
    // "live on server" section changed.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [restore.result]);

  function askRestore(backup: BackupRow) {
    setTarget(backup);
    setConfirmOpen(true);
  }

  function doRestore() {
    if (!target?.lxcPath || target.slot == null) return;
    setConfirmOpen(false);
    resetRestore();
    startRestore(target.lxcPath, target.slot);
  }

  return (
    <Card className="flex h-full min-h-0 w-full flex-col">
      <CardHeader className="flex flex-row items-center justify-between">
        <div>
          <CardTitle>{t("worldLibrary.title")}</CardTitle>
          <CardDescription>{t("worldLibrary.desc")}</CardDescription>
        </div>
        <Button variant="outline" size="sm" onClick={refresh} disabled={loading}>
          {loading ? t("worldLibrary.loading") : t("worldLibrary.refresh")}
        </Button>
      </CardHeader>
      <CardContent className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto">
        {activeOperation && activeOperation !== "restore" && (
          <div className="rounded-md border border-destructive/40 bg-destructive/10 p-2 text-xs text-destructive">
            {t("worldLibrary.otherOpBanner", { operation: t(`operations.${activeOperation}`) })}
          </div>
        )}

        <div>
          <div className="mb-1 text-sm text-muted-foreground">{t("worldLibrary.liveOnServer")}</div>
          {worlds.length > 0 ? (
            <div className="grid gap-1.5">
              {worlds.map((w) => (
                <div
                  key={w.slot}
                  className="flex items-center justify-between rounded-md border p-2 text-sm"
                >
                  <span className="font-mono">slot {w.slot}</span>
                  <span className="text-muted-foreground">
                    {formatBytes(w.size)} · {formatMtime(w.modified)}
                  </span>
                </div>
              ))}
            </div>
          ) : (
            <div className="text-sm text-muted-foreground">{t("worldLibrary.noWorlds")}</div>
          )}
        </div>

        <div className="flex min-h-0 flex-1 flex-col">
          <div className="mb-1 text-sm text-muted-foreground">
            {t("worldLibrary.backups")} {backups.length === 0 && t("worldLibrary.noBackupsYet")}
          </div>
          {backups.length > 0 && (
            <div className="min-h-0 flex-1 overflow-y-auto rounded-md border">
              {backups.map((b) => (
                <div
                  key={b.id}
                  className="flex items-center justify-between gap-2 border-b p-2 text-sm last:border-b-0"
                >
                  <div className="min-w-0">
                    <div>
                      <span className="font-mono">slot {b.slot ?? "?"}</span>{" "}
                      <span className="text-muted-foreground">{formatTs(b.ts)}</span>
                    </div>
                    <div className="truncate font-mono text-xs text-muted-foreground">{b.lxcPath}</div>
                    {b.note && <div className="text-xs text-muted-foreground">{b.note}</div>}
                  </div>
                  <div className="flex shrink-0 items-center gap-2">
                    {b.bytes != null && (
                      <span className="text-xs text-muted-foreground">{formatBytes(b.bytes)}</span>
                    )}
                    <Button
                      size="sm"
                      variant="outline"
                      disabled={globalBusy || !b.lxcPath || b.slot == null}
                      onClick={() => askRestore(b)}
                    >
                      {t("worldLibrary.restoreButton")}
                    </Button>
                  </div>
                </div>
              ))}
            </div>
          )}
        </div>

        {(restore.log.length > 0 || restore.running) && (
          <Stepper
            steps={RESTORE_STEPS}
            log={restore.log}
            doneState="done"
            failedStates={["failed", "rollingBack"]}
            failedBanner={t("worldLibrary.stepper.failedBanner")}
          />
        )}

        {!restore.running && restore.result && (
          <div className="rounded-md border p-2 text-sm text-muted-foreground">
            {t("worldLibrary.restoredResult", { slot: restore.result.slot })}
            {restore.result.gameId && <> {t("worldLibrary.gameId", { id: restore.result.gameId })}</>}
            {restore.result.backupPath && (
              <div className="font-mono text-xs">
                {t("worldLibrary.safetyBackup", { path: restore.result.backupPath })}
              </div>
            )}
          </div>
        )}

        {!restore.running && restore.error && (
          <div className="text-sm text-destructive">{restore.error}</div>
        )}
      </CardContent>

      <Dialog open={confirmOpen} onOpenChange={setConfirmOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t("worldLibrary.confirmTitle", { slot: target?.slot })}</DialogTitle>
            <DialogDescription>
              {t("worldLibrary.confirmDesc")}
              <div className="mt-2 font-mono text-xs">{target?.lxcPath}</div>
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setConfirmOpen(false)}>
              {t("worldLibrary.cancel")}
            </Button>
            <Button variant="destructive" disabled={globalBusy} onClick={doRestore}>
              {t("worldLibrary.confirmButton")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </Card>
  );
}
