import { useState } from "react";
import { toast } from "sonner";

import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
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
import type { LocalWorldCandidate, PushState } from "@/lib/api";
import * as api from "@/lib/api";
import { formatBytes, formatMtime } from "@/lib/format";

// The linear "happy path" — RollingBack/Failed are shown as a banner instead of a step,
// since they're not something that happens *in* the pipeline, they're what happens when
// it breaks.
const MAIN_STEPS: StepDef<PushState>[] = [
  { state: "uploading", label: "Upload" },
  { state: "checkingService", label: "Check service" },
  { state: "stopping", label: "Stop" },
  { state: "backingUp", label: "Back up" },
  { state: "copying", label: "Copy in" },
  { state: "fixingPerms", label: "Fix perms" },
  { state: "starting", label: "Start" },
  { state: "capturingGameId", label: "Capture game id" },
  { state: "verifying", label: "Verify" },
];

export function PushPanel() {
  const { activeOperation, push, startPush, resetPush } = useOperations();
  const [candidates, setCandidates] = useState<LocalWorldCandidate[]>([]);
  const [chosenSlot, setChosenSlot] = useState<number | undefined>(undefined);
  const [confirmOpen, setConfirmOpen] = useState(false);

  const chosen = candidates.find((c) => c.slot === chosenSlot) ?? null;
  // Disabled while *any* operation is running, not just this one — push/pull/restore
  // all stop and restart the same server, so they can never safely overlap.
  const globalBusy = activeOperation !== null;

  async function pickAndResolve(kind: "file" | "folder") {
    try {
      const path = kind === "file" ? await api.pickWorldFile() : await api.pickWorldFolder();
      if (!path || Array.isArray(path)) return;
      const found = await api.resolveLocalWorld(path);
      setCandidates(found);
      setChosenSlot(found[0]?.slot);
      resetPush();
    } catch (e) {
      toast.error(String(e));
    }
  }

  function confirmAndPush() {
    if (!chosen) return;
    setConfirmOpen(false);
    startPush(chosen);
  }

  return (
    <Card className="flex h-full min-h-0 w-full flex-col">
      <CardHeader>
        <CardTitle>Push a world</CardTitle>
        <CardDescription>
          Stops the live server, backs up the current world, swaps yours in, restarts,
          and verifies — auto-rolls back on any failure. No guarantee against data
          loss — keep your own backups.
        </CardDescription>
      </CardHeader>
      <CardContent className="flex min-h-0 flex-1 flex-col gap-4">
        <div className="flex flex-wrap gap-2">
          <Button variant="outline" size="sm" disabled={globalBusy} onClick={() => pickAndResolve("file")}>
            Choose file (.zip or .world.gzip)
          </Button>
          <Button variant="outline" size="sm" disabled={globalBusy} onClick={() => pickAndResolve("folder")}>
            Choose folder
          </Button>
        </div>

        {activeOperation && activeOperation !== "push" && (
          <div className="rounded-md border border-destructive/40 bg-destructive/10 p-2 text-xs text-destructive">
            A {activeOperation} is in progress — finish or wait before starting a push.
          </div>
        )}

        {candidates.length > 0 && (
          <div className="grid gap-2 rounded-md border p-3">
            {candidates.length > 1 ? (
              <>
                <div className="text-sm text-muted-foreground">
                  Found {candidates.length} worlds — pick one:
                </div>
                <Select
                  value={chosenSlot !== undefined ? String(chosenSlot) : undefined}
                  onValueChange={(v) => v && setChosenSlot(Number(v))}
                >
                  <SelectTrigger className="max-w-full min-w-40">
                    <SelectValue placeholder="Pick a slot…" />
                  </SelectTrigger>
                  <SelectContent className="w-auto min-w-72" alignItemWithTrigger={false}>
                    {candidates.map((c) => (
                      <SelectItem key={c.slot} value={String(c.slot)}>
                        slot {c.slot} — {formatBytes(c.mainSize)}, {formatMtime(c.mainModified)}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </>
            ) : null}
            {chosen && (
              <div className="text-xs text-muted-foreground">
                <div className="font-mono">{chosen.mainPath}</div>
                {chosen.pugbackupPath && (
                  <div className="mt-1">
                    <div className="font-mono">{chosen.pugbackupPath}</div>
                    <div>
                      ↳ Core Keeper&apos;s own rolling backup for this slot, found alongside
                      the main file — included automatically, both travel together.
                    </div>
                  </div>
                )}
              </div>
            )}
            <Button size="sm" className="w-fit" disabled={!chosen || globalBusy} onClick={() => setConfirmOpen(true)}>
              Push this world
            </Button>
          </div>
        )}

        {(push.log.length > 0 || push.running) && (
          <Stepper
            steps={MAIN_STEPS}
            log={push.log}
            doneState="done"
            failedStates={["failed", "rollingBack"]}
            failedBanner="Rolled back — server restart was attempted automatically."
          />
        )}

        {!push.running && push.result && (
          <div className="grid gap-1 rounded-md border p-3 text-sm">
            <div className="flex items-center gap-2">
              <Badge className="border-signal bg-signal text-signal-foreground">Done</Badge>
              <span>world {push.result.slot} is live</span>
            </div>
            <div className="text-muted-foreground">
              {formatBytes(push.result.bytesUploaded)} uploaded
              {push.result.gameId && (
                <>
                  {" "}
                  · game id <span className="font-mono">{push.result.gameId}</span>
                </>
              )}
            </div>
            {push.result.backupPath && (
              <div className="font-mono text-xs text-muted-foreground">
                previous world backed up to {push.result.backupPath}
              </div>
            )}
          </div>
        )}

        {!push.running && push.error && (
          <div className="text-sm text-destructive">{push.error}</div>
        )}
      </CardContent>

      <Dialog open={confirmOpen} onOpenChange={setConfirmOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Swap in world {chosen?.slot}?</DialogTitle>
            <DialogDescription>
              This stops the live Core Keeper server, backs up the current world
              (dated, restorable), copies yours in, restarts, and verifies. On any
              failure it automatically restores the backup and restarts the server.
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setConfirmOpen(false)}>
              Cancel
            </Button>
            <Button variant="destructive" disabled={globalBusy} onClick={confirmAndPush}>
              Stop server &amp; push
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </Card>
  );
}
