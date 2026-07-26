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
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Stepper, type StepDef } from "@/components/ckm/Stepper";
import { useOperations } from "@/lib/operations";
import type { PullState, RemoteWorldInfo } from "@/lib/api";
import * as api from "@/lib/api";
import { formatBytes, formatMtime } from "@/lib/format";

const PULL_STEPS: StepDef<PullState>[] = [
  { state: "checkingService", label: "Check service" },
  { state: "stopping", label: "Stop" },
  { state: "downloading", label: "Download" },
  { state: "starting", label: "Restart" },
];

export function PullPanel() {
  const { activeOperation, pull, startPull, resetPull } = useOperations();
  const [worlds, setWorlds] = useState<RemoteWorldInfo[]>([]);
  const [chosenSlot, setChosenSlot] = useState<number | undefined>(undefined);
  const [savedPath, setSavedPath] = useState<string | null>(null);
  const [confirmOpen, setConfirmOpen] = useState(false);
  const [saving, setSaving] = useState(false);

  const globalBusy = activeOperation !== null;

  async function refresh() {
    try {
      const found = await api.listRemoteWorlds();
      setWorlds(found);
      setChosenSlot((prev) => prev ?? found[0]?.slot);
    } catch (e) {
      toast.error(String(e));
    }
  }

  useEffect(() => {
    refresh();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  function confirmAndPull() {
    if (chosenSlot === undefined) return;
    setConfirmOpen(false);
    setSavedPath(null);
    resetPull();
    startPull(chosenSlot);
  }

  async function saveAs(kind: "folder" | "zip") {
    if (!pull.result) return;
    try {
      const destination =
        kind === "folder" ? await api.pickSaveFolder() : await api.saveWorldZip(`world-${pull.result.slot}.zip`);
      if (!destination || Array.isArray(destination)) return;
      setSaving(true);
      const finalPath = await api.packagePulledWorld(pull.result, destination, kind === "zip");
      setSavedPath(finalPath);
      toast.success(`Saved to ${finalPath}`);
    } catch (e) {
      toast.error(String(e));
    } finally {
      setSaving(false);
    }
  }

  const chosen = worlds.find((w) => w.slot === chosenSlot) ?? null;

  return (
    <Card className="flex h-full min-h-0 w-full flex-col">
      <CardHeader>
        <CardTitle>Pull a world</CardTitle>
        <CardDescription>
          Stops the live server just long enough to copy a consistent world, restarts
          immediately, then lets you save it locally. No guarantee against data loss —
          keep your own backups.
        </CardDescription>
      </CardHeader>
      <CardContent className="flex min-h-0 flex-1 flex-col gap-4">
        <div className="flex flex-wrap items-center gap-2">
          <Button variant="outline" size="sm" disabled={globalBusy} onClick={refresh}>
            Refresh
          </Button>
          {worlds.length > 1 && (
            <Select
              value={chosenSlot !== undefined ? String(chosenSlot) : undefined}
              onValueChange={(v) => v && setChosenSlot(Number(v))}
            >
              <SelectTrigger className="max-w-full min-w-40">
                <SelectValue placeholder="Pick a slot…" />
              </SelectTrigger>
              <SelectContent className="w-auto min-w-72" alignItemWithTrigger={false}>
                {worlds.map((w) => (
                  <SelectItem key={w.slot} value={String(w.slot)}>
                    slot {w.slot} — {formatBytes(w.size)}, {formatMtime(w.modified)}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          )}
          {worlds.length === 0 && (
            <span className="text-sm text-muted-foreground">No worlds found on the server.</span>
          )}
        </div>

        {activeOperation && activeOperation !== "pull" && (
          <div className="rounded-md border border-destructive/40 bg-destructive/10 p-2 text-xs text-destructive">
            A {activeOperation} is in progress — finish or wait before starting a pull.
          </div>
        )}

        {chosen && (
          <Button size="sm" className="w-fit" disabled={globalBusy} onClick={() => setConfirmOpen(true)}>
            Pull world {chosen.slot}
          </Button>
        )}

        {(pull.log.length > 0 || pull.running) && (
          <Stepper
            steps={PULL_STEPS}
            log={pull.log}
            doneState="done"
            failedStates={["failed"]}
            failedBanner="Failed — a restart was attempted automatically regardless."
          />
        )}

        {!pull.running && pull.result && !savedPath && (
          <div className="grid gap-2 rounded-md border p-3 text-sm">
            <div className="text-muted-foreground">
              {formatBytes(pull.result.bytes)} downloaded — save world {pull.result.slot} as:
            </div>
            <div className="flex gap-2">
              <Button size="sm" variant="outline" disabled={saving} onClick={() => saveAs("folder")}>
                Folder…
              </Button>
              <Button size="sm" variant="outline" disabled={saving} onClick={() => saveAs("zip")}>
                .zip…
              </Button>
            </div>
          </div>
        )}

        {savedPath && (
          <div className="font-mono text-xs text-muted-foreground">saved to {savedPath}</div>
        )}

        {!pull.running && pull.error && <div className="text-sm text-destructive">{pull.error}</div>}
      </CardContent>

      <Dialog open={confirmOpen} onOpenChange={setConfirmOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Pull world {chosen?.slot}?</DialogTitle>
            <DialogDescription>
              Briefly stops the live Core Keeper server to copy a consistent world file,
              then restarts it immediately. Nothing on the server is modified.
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setConfirmOpen(false)}>
              Cancel
            </Button>
            <Button variant="destructive" disabled={globalBusy} onClick={confirmAndPull}>
              Stop server &amp; pull
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </Card>
  );
}
