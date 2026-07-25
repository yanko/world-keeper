import { useEffect, useState } from "react";
import { toast } from "sonner";

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
import type { BackupRow, RemoteWorldInfo, RestoreProgress, RestoreState } from "@/lib/api";
import * as api from "@/lib/api";
import { formatBytes, formatMtime } from "@/lib/format";

const RESTORE_STEPS: StepDef<RestoreState>[] = [
  { state: "checkingService", label: "Check service" },
  { state: "stopping", label: "Stop" },
  { state: "backingUp", label: "Back up" },
  { state: "copying", label: "Copy in" },
  { state: "fixingPerms", label: "Fix perms" },
  { state: "starting", label: "Start" },
  { state: "capturingGameId", label: "Capture game id" },
  { state: "verifying", label: "Verify" },
];

function formatTs(ts: string): string {
  const d = new Date(ts);
  return Number.isNaN(d.getTime()) ? ts : d.toLocaleString();
}

export function WorldLibraryPanel() {
  const [worlds, setWorlds] = useState<RemoteWorldInfo[]>([]);
  const [backups, setBackups] = useState<BackupRow[]>([]);
  const [loading, setLoading] = useState(false);
  const [target, setTarget] = useState<BackupRow | null>(null);
  const [confirmOpen, setConfirmOpen] = useState(false);
  const [restoring, setRestoring] = useState(false);
  const [log, setLog] = useState<RestoreProgress[]>([]);
  const [result, setResult] = useState<api.RestoreResult | null>(null);
  const [error, setError] = useState<string | null>(null);

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
  }, []);

  function askRestore(backup: BackupRow) {
    setTarget(backup);
    setConfirmOpen(true);
  }

  async function doRestore() {
    if (!target?.lxcPath || target.slot == null) return;
    setConfirmOpen(false);
    setRestoring(true);
    setLog([]);
    setResult(null);
    setError(null);

    const unlisten = await api.onRestoreProgress((p) => setLog((prev) => [...prev, p]));
    try {
      const r = await api.restoreBackup(target.lxcPath, target.slot);
      setResult(r);
      toast.success(`World ${r.slot} restored from backup.`);
      refresh();
    } catch (e) {
      setError(String(e));
      toast.error(String(e));
    } finally {
      unlisten();
      setRestoring(false);
    }
  }

  return (
    <Card className="flex h-full min-h-0 w-full flex-col">
      <CardHeader className="flex flex-row items-center justify-between">
        <div>
          <CardTitle>World library</CardTitle>
          <CardDescription>What's live on the server, and every dated backup this app has made.</CardDescription>
        </div>
        <Button variant="outline" size="sm" onClick={refresh} disabled={loading}>
          {loading ? "Loading…" : "Refresh"}
        </Button>
      </CardHeader>
      <CardContent className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto">
        <div>
          <div className="mb-1 text-sm text-muted-foreground">Live on server</div>
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
            <div className="text-sm text-muted-foreground">No worlds found on the server.</div>
          )}
        </div>

        <div className="flex min-h-0 flex-1 flex-col">
          <div className="mb-1 text-sm text-muted-foreground">
            Backups {backups.length === 0 && "— none yet; your first push or restore creates one."}
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
                      disabled={restoring || !b.lxcPath || b.slot == null}
                      onClick={() => askRestore(b)}
                    >
                      Restore
                    </Button>
                  </div>
                </div>
              ))}
            </div>
          )}
        </div>

        {(log.length > 0 || restoring) && (
          <Stepper
            steps={RESTORE_STEPS}
            log={log}
            doneState="done"
            failedStates={["failed", "rollingBack"]}
            failedBanner="Rolled back — a safety backup was restored and the server restarted automatically."
          />
        )}

        {result && (
          <div className="rounded-md border p-2 text-sm text-muted-foreground">
            Restored world {result.slot}
            {result.gameId && (
              <>
                {" "}
                · game id <span className="font-mono">{result.gameId}</span>
              </>
            )}
            {result.backupPath && (
              <div className="font-mono text-xs">safety backup: {result.backupPath}</div>
            )}
          </div>
        )}

        {error && <div className="text-sm text-destructive">{error}</div>}
      </CardContent>

      <Dialog open={confirmOpen} onOpenChange={setConfirmOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Restore world {target?.slot} from this backup?</DialogTitle>
            <DialogDescription>
              Stops the live server, backs up whatever's currently live (so this itself
              is undoable), copies this backup into place, restarts, and verifies.
              <div className="mt-2 font-mono text-xs">{target?.lxcPath}</div>
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setConfirmOpen(false)}>
              Cancel
            </Button>
            <Button variant="destructive" onClick={doRestore}>
              Stop server &amp; restore
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </Card>
  );
}
