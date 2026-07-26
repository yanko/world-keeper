import { useState } from "react";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";

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

export function PushPanel() {
  const { t } = useTranslation();
  const { activeOperation, push, startPush, resetPush } = useOperations();
  const [candidates, setCandidates] = useState<LocalWorldCandidate[]>([]);
  const [chosenSlot, setChosenSlot] = useState<number | undefined>(undefined);
  const [confirmOpen, setConfirmOpen] = useState(false);

  // The linear "happy path" — RollingBack/Failed are shown as a banner instead of a
  // step, since they're not something that happens *in* the pipeline, they're what
  // happens when it breaks.
  const MAIN_STEPS: StepDef<PushState>[] = [
    { state: "uploading", label: t("push.stepper.upload") },
    { state: "checkingService", label: t("push.stepper.checkService") },
    { state: "stopping", label: t("push.stepper.stop") },
    { state: "backingUp", label: t("push.stepper.backUp") },
    { state: "copying", label: t("push.stepper.copyIn") },
    { state: "fixingPerms", label: t("push.stepper.fixPerms") },
    { state: "starting", label: t("push.stepper.start") },
    { state: "capturingGameId", label: t("push.stepper.captureGameId") },
    { state: "verifying", label: t("push.stepper.verify") },
  ];

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
        <CardTitle>{t("push.title")}</CardTitle>
        <CardDescription>{t("push.desc")}</CardDescription>
      </CardHeader>
      <CardContent className="flex min-h-0 flex-1 flex-col gap-4">
        <div className="flex flex-wrap gap-2">
          <Button variant="outline" size="sm" disabled={globalBusy} onClick={() => pickAndResolve("file")}>
            {t("push.chooseFile")}
          </Button>
          <Button variant="outline" size="sm" disabled={globalBusy} onClick={() => pickAndResolve("folder")}>
            {t("push.chooseFolder")}
          </Button>
        </div>

        {activeOperation && activeOperation !== "push" && (
          <div className="rounded-md border border-destructive/40 bg-destructive/10 p-2 text-xs text-destructive">
            {t("push.otherOpBanner", { operation: t(`operations.${activeOperation}`) })}
          </div>
        )}

        {candidates.length > 0 && (
          <div className="grid gap-2 rounded-md border p-3">
            {candidates.length > 1 ? (
              <>
                <div className="text-sm text-muted-foreground">
                  {t("push.foundWorlds", { count: candidates.length })}
                </div>
                <Select
                  value={chosenSlot !== undefined ? String(chosenSlot) : undefined}
                  onValueChange={(v) => v && setChosenSlot(Number(v))}
                >
                  <SelectTrigger className="max-w-full min-w-40">
                    <SelectValue placeholder={t("push.pickSlotPlaceholder")} />
                  </SelectTrigger>
                  <SelectContent className="w-auto min-w-72" alignItemWithTrigger={false}>
                    {candidates.map((c) => (
                      <SelectItem key={c.slot} value={String(c.slot)}>
                        {t("push.slotOption", {
                          slot: c.slot,
                          size: formatBytes(c.mainSize),
                          modified: formatMtime(c.mainModified),
                        })}
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
                    <div>{t("push.pugbackupNote")}</div>
                  </div>
                )}
              </div>
            )}
            <Button size="sm" className="w-fit" disabled={!chosen || globalBusy} onClick={() => setConfirmOpen(true)}>
              {t("push.pushThisWorld")}
            </Button>
          </div>
        )}

        {(push.log.length > 0 || push.running) && (
          <Stepper
            steps={MAIN_STEPS}
            log={push.log}
            doneState="done"
            failedStates={["failed", "rollingBack"]}
            failedBanner={t("push.stepper.failedBanner")}
          />
        )}

        {!push.running && push.result && (
          <div className="grid gap-1 rounded-md border p-3 text-sm">
            <div className="flex items-center gap-2">
              <Badge className="border-signal bg-signal text-signal-foreground">{t("push.resultDone")}</Badge>
              <span>{t("push.resultLive", { slot: push.result.slot })}</span>
            </div>
            <div className="text-muted-foreground">
              {t("push.resultUploaded", { size: formatBytes(push.result.bytesUploaded) })}
              {push.result.gameId && <> {t("push.resultGameId", { id: push.result.gameId })}</>}
            </div>
            {push.result.backupPath && (
              <div className="font-mono text-xs text-muted-foreground">
                {t("push.resultBackup", { path: push.result.backupPath })}
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
            <DialogTitle>{t("push.confirmTitle", { slot: chosen?.slot })}</DialogTitle>
            <DialogDescription>{t("push.confirmDesc")}</DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setConfirmOpen(false)}>
              {t("push.cancel")}
            </Button>
            <Button variant="destructive" disabled={globalBusy} onClick={confirmAndPush}>
              {t("push.confirmButton")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </Card>
  );
}
