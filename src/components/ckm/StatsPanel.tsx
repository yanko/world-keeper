import { useEffect, useRef, useState } from "react";
import { toast } from "sonner";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { CopyIcon } from "lucide-react";
import { useTranslation } from "react-i18next";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { LogDrawer } from "@/components/ckm/LogDrawer";
import type { ServerStats } from "@/lib/api";
import * as api from "@/lib/api";
import { useOperations } from "@/lib/operations";

const POLL_INTERVAL_MS = 5000;

function formatKb(kb: number | null): string {
  if (kb == null) return "—";
  const mb = kb / 1024;
  return mb < 1024 ? `${mb.toFixed(0)} MiB` : `${(mb / 1024).toFixed(2)} GiB`;
}

function formatUptime(seconds: number | null): string {
  if (seconds == null) return "—";
  const d = Math.floor(seconds / 86400);
  const h = Math.floor((seconds % 86400) / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  return [d && `${d}d`, h && `${h}h`, `${m}m`].filter(Boolean).join(" ");
}

function statusVariant(status: string): "default" | "destructive" | "secondary" {
  if (status === "active") return "default";
  if (status === "failed" || status === "inactive") return "destructive";
  return "secondary";
}

// "active" gets the one confident signal color (SPEC §12); failed/inactive keep the
// badge's own destructive styling, everything else stays neutral.
function statusClasses(status: string): string {
  return status === "active" ? "border-signal bg-signal text-signal-foreground" : "";
}

export function StatsPanel() {
  const { t } = useTranslation();
  const { activeOperation, push, pull, restore } = useOperations();
  const [stats, setStats] = useState<ServerStats | null>(null);
  const [error, setError] = useState<string | null>(null);
  // `collect()` on the backend is several sequential SSH round-trips (and can be slow
  // on a Docker target specifically — see ssh.rs's udp_listening fallback), so the
  // 5s auto-poll and a manual Refresh click can easily have more than one request in
  // flight. Without this guard, whichever happens to *resolve* last wins, not
  // whichever was *sent* last — an older, slower response can overwrite a fresher one
  // and make the panel look like it stopped updating.
  const requestIdRef = useRef(0);

  async function refresh() {
    const requestId = ++requestIdRef.current;
    try {
      const s = await api.getStats();
      if (requestId !== requestIdRef.current) return;
      setStats(s);
      setError(null);
    } catch (e) {
      if (requestId !== requestIdRef.current) return;
      setError(String(e));
    }
  }

  async function copyGameId(id: string) {
    try {
      await writeText(id);
      toast.success(t("stats.toasts.gameIdCopied"));
    } catch (e) {
      toast.error(String(e));
    }
  }

  useEffect(() => {
    refresh();
    const id = setInterval(refresh, POLL_INTERVAL_MS);
    return () => clearInterval(id);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Stats stays live and available the whole time a push/pull/restore is running (it's
  // a read-only view over the same connection, not part of the mutual-exclusion group —
  // see AppState::operation_lock) — so this is a good place to also surface *which* step
  // it's currently on, updating in step with the same progress events the Push/Pull/
  // World-library tabs show, without needing to switch back to that tab to check.
  const currentLog =
    activeOperation === "push" ? push.log : activeOperation === "pull" ? pull.log : activeOperation === "restore" ? restore.log : null;
  const currentStep = currentLog && currentLog.length > 0 ? currentLog[currentLog.length - 1].state : null;

  return (
    <Card className="flex h-full min-h-0 w-full flex-col">
      <CardHeader className="flex flex-row flex-wrap items-center justify-between gap-2">
        <CardTitle>{t("stats.title")}</CardTitle>
        <div className="flex flex-wrap gap-2">
          <LogDrawer />
          <Button variant="outline" size="sm" onClick={refresh}>
            {t("stats.refresh")}
          </Button>
        </div>
      </CardHeader>
      <CardContent className="flex min-h-0 flex-1 flex-col gap-4">
        {activeOperation && (
          <div className="flex items-center gap-2 rounded-md border border-signal/40 bg-signal/10 p-2 text-sm">
            <Badge className="animate-pulse border-signal bg-signal text-signal-foreground">
              {t(`operations.${activeOperation}`)}
            </Badge>
            <span>
              {t("stats.operationInProgress")}{currentStep ? ` — ${currentStep}` : ""}
            </span>
          </div>
        )}
        {error && <div className="text-sm text-destructive">{error}</div>}
        {stats && (
          <>
            <div className="flex flex-wrap items-center gap-3">
              <Badge variant={statusVariant(stats.serviceStatus)} className={statusClasses(stats.serviceStatus)}>
                {stats.serviceStatus}
              </Badge>
              {stats.activeSince && (
                <span className="font-mono text-xs text-muted-foreground">
                  {t("stats.since", { time: stats.activeSince })}
                </span>
              )}
              <span className="text-xs text-muted-foreground">
                {t("stats.uptime", { time: formatUptime(stats.uptimeSeconds) })}
              </span>
              {stats.serverTime && (
                <span className="font-mono text-xs text-muted-foreground">
                  {t("stats.serverClock", { time: stats.serverTime })}
                </span>
              )}
            </div>

            <div>
              <div className="text-sm text-muted-foreground">{t("stats.currentWorld")}</div>
              {stats.gameId ? (
                <div className="flex items-center gap-1.5">
                  <span className="font-mono text-sm">{stats.gameId}</span>
                  <Button
                    variant="ghost"
                    size="icon-xs"
                    title={t("stats.copyGameId")}
                    onClick={() => copyGameId(stats.gameId!)}
                  >
                    <CopyIcon />
                  </Button>
                </div>
              ) : (
                <div className="text-sm text-muted-foreground">{t("stats.noGameId")}</div>
              )}
            </div>

            <div className="grid grid-cols-1 gap-3 text-sm sm:grid-cols-2">
              <div>
                <div className="text-muted-foreground">{t("stats.memory")}</div>
                <div className="font-mono">
                  {formatKb(
                    stats.memTotalKb != null && stats.memAvailableKb != null
                      ? stats.memTotalKb - stats.memAvailableKb
                      : null
                  )}{" "}
                  / {formatKb(stats.memTotalKb)}
                </div>
              </div>
              <div>
                <div className="text-muted-foreground">{t("stats.diskWorlds")}</div>
                <div className="font-mono">
                  {stats.diskUsed ?? "—"} / {stats.diskSize ?? "—"} (
                  {stats.diskUsePct ?? "—"})
                </div>
              </div>
            </div>

            <div>
              <div className="text-sm text-muted-foreground">{t("stats.udpListening")}</div>
              {stats.udpListening.length > 0 ? (
                <pre className="overflow-x-auto rounded-md bg-muted p-2 font-mono text-xs">
                  {stats.udpListening.join("\n")}
                </pre>
              ) : (
                <div className="text-sm text-muted-foreground">{t("stats.nothingFound")}</div>
              )}
            </div>

            <div className="flex min-h-0 flex-1 flex-col">
              <div className="mb-1 text-sm text-muted-foreground">{t("stats.recentLog")}</div>
              <pre className="min-h-24 flex-1 overflow-y-auto overflow-x-hidden rounded-md bg-muted p-2 font-mono text-xs whitespace-pre-wrap">
                {stats.recentLogTail || t("stats.empty")}
              </pre>
            </div>
          </>
        )}
      </CardContent>
    </Card>
  );
}
