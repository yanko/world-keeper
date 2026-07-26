import { useState } from "react";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import type { Discovered, DiscoveryResult, ServiceKind } from "@/lib/api";
import * as api from "@/lib/api";

function serviceKindLabel(kind: ServiceKind): string {
  switch (kind.type) {
    case "systemd":
      return `systemd: ${kind.unit}`;
    case "docker":
      return `docker: ${kind.container}`;
    case "process":
      return `process: ${kind.startCmd}`;
  }
}

interface DiscoveryConfirmProps {
  profileId: string;
  result: DiscoveryResult;
  onRescan: () => void;
  onConfirmed: () => void;
}

export function DiscoveryConfirm({
  profileId,
  result,
  onRescan,
  onConfirmed,
}: DiscoveryConfirmProps) {
  const { t } = useTranslation();
  const [candidateIdx, setCandidateIdx] = useState<string | undefined>(
    result.serviceKindCandidates.length > 0 ? "0" : undefined
  );
  const [manualUnit, setManualUnit] = useState("");
  const [worldsPath, setWorldsPath] = useState<string | undefined>(
    result.worldsPath ?? result.worldsPathCandidates[0]
  );
  const [manualWorldsPath, setManualWorldsPath] = useState("");
  const [confirming, setConfirming] = useState(false);

  const resolvedKind: ServiceKind | null =
    result.serviceKind ??
    (candidateIdx !== undefined
      ? result.serviceKindCandidates[Number(candidateIdx)]?.kind ?? null
      : manualUnit
        ? { type: "systemd", unit: manualUnit }
        : null);

  const resolvedSteamUser: string | null =
    result.steamUser ??
    (candidateIdx !== undefined
      ? result.serviceKindCandidates[Number(candidateIdx)]?.steamUser ?? null
      : null);

  const resolvedWorldsPath =
    result.worldsPath ?? worldsPath ?? (manualWorldsPath || null);

  async function confirm() {
    if (!resolvedKind) {
      toast.error(t("discoveryConfirm.toasts.pickServiceError"));
      return;
    }
    if (!resolvedWorldsPath) {
      toast.error(t("discoveryConfirm.toasts.pickWorldsPathError"));
      return;
    }
    const discovered: Discovered = {
      os: result.os,
      serviceKind: resolvedKind,
      worldsPath: resolvedWorldsPath,
      steamUser: resolvedSteamUser,
      launchScript: result.launchScript,
      probedAt: result.probedAt,
    };
    setConfirming(true);
    try {
      await api.confirmDiscovery(profileId, discovered);
      toast.success(t("discoveryConfirm.toasts.confirmed"));
      onConfirmed();
    } catch (e) {
      toast.error(String(e));
    } finally {
      setConfirming(false);
    }
  }

  return (
    <Card className="w-full">
      <CardHeader>
        <CardTitle>{t("discoveryConfirm.title")}</CardTitle>
        <CardDescription>
          {t("discoveryConfirm.descPrefix")}{" "}
          <span className="font-mono">{result.os ?? t("discoveryConfirm.unknownOs")}</span>
        </CardDescription>
      </CardHeader>
      <CardContent className="grid gap-4">
        <div className="grid gap-1.5">
          <div className="text-sm font-medium">{t("discoveryConfirm.service")}</div>
          {result.serviceKind ? (
            <div className="rounded-md border p-2 font-mono text-sm">
              {serviceKindLabel(result.serviceKind)}
              {result.launchScript && (
                <div className="text-muted-foreground">{result.launchScript}</div>
              )}
            </div>
          ) : result.serviceKindCandidates.length > 0 ? (
            <>
              <Select
                value={candidateIdx}
                onValueChange={(v) => setCandidateIdx(v ?? undefined)}
              >
                <SelectTrigger className="max-w-full min-w-40">
                  <SelectValue placeholder={t("discoveryConfirm.pickServicePlaceholder")} />
                </SelectTrigger>
                <SelectContent className="w-auto min-w-72" alignItemWithTrigger={false}>
                  {result.serviceKindCandidates.map((c, i) => (
                    <SelectItem key={i} value={String(i)}>
                      {serviceKindLabel(c.kind)}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              {candidateIdx !== undefined && (
                <div className="font-mono text-xs text-muted-foreground">
                  {result.serviceKindCandidates[Number(candidateIdx)]?.execStart}
                </div>
              )}
            </>
          ) : (
            <Input
              placeholder={t("discoveryConfirm.manualUnitPlaceholder")}
              value={manualUnit}
              onChange={(e) => setManualUnit(e.target.value)}
            />
          )}
        </div>

        <div className="grid gap-1.5">
          <div className="text-sm font-medium">{t("discoveryConfirm.worldsPath")}</div>
          {result.worldsPath ? (
            <div className="rounded-md border p-2 font-mono text-sm">{result.worldsPath}</div>
          ) : result.worldsPathCandidates.length > 0 ? (
            <Select value={worldsPath} onValueChange={(v) => setWorldsPath(v ?? undefined)}>
              <SelectTrigger className="max-w-full min-w-40">
                <SelectValue placeholder={t("discoveryConfirm.pickWorldsPathPlaceholder")} />
              </SelectTrigger>
              <SelectContent className="w-auto min-w-72" alignItemWithTrigger={false}>
                {result.worldsPathCandidates.map((p) => (
                  <SelectItem key={p} value={p} className="font-mono">
                    {p}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          ) : (
            <Input
              placeholder={t("discoveryConfirm.manualWorldsPathPlaceholder")}
              value={manualWorldsPath}
              onChange={(e) => setManualWorldsPath(e.target.value)}
            />
          )}
        </div>

        <div className="flex gap-2">
          <Button variant="outline" size="sm" onClick={onRescan}>
            {t("discoveryConfirm.rescan")}
          </Button>
          <Button size="sm" onClick={confirm} disabled={confirming}>
            {confirming ? t("discoveryConfirm.confirming") : t("discoveryConfirm.confirm")}
          </Button>
        </div>
      </CardContent>
    </Card>
  );
}
