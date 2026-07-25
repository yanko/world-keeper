import { useEffect, useRef, useState } from "react";
import { toast } from "sonner";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { CopyIcon } from "lucide-react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { LogDrawer } from "@/components/ckm/LogDrawer";
import type { ServerStats } from "@/lib/api";
import * as api from "@/lib/api";

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

async function copyGameId(id: string) {
  try {
    await writeText(id);
    toast.success("Game id copied.");
  } catch (e) {
    toast.error(String(e));
  }
}

export function StatsPanel() {
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

  useEffect(() => {
    refresh();
    const id = setInterval(refresh, POLL_INTERVAL_MS);
    return () => clearInterval(id);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return (
    <Card className="flex h-full min-h-0 w-full flex-col">
      <CardHeader className="flex flex-row items-center justify-between">
        <CardTitle>Server stats</CardTitle>
        <div className="flex gap-2">
          <LogDrawer />
          <Button variant="outline" size="sm" onClick={refresh}>
            Refresh
          </Button>
        </div>
      </CardHeader>
      <CardContent className="flex min-h-0 flex-1 flex-col gap-4">
        {error && <div className="text-sm text-destructive">{error}</div>}
        {stats && (
          <>
            <div className="flex flex-wrap items-center gap-3">
              <Badge variant={statusVariant(stats.serviceStatus)} className={statusClasses(stats.serviceStatus)}>
                {stats.serviceStatus}
              </Badge>
              {stats.activeSince && (
                <span className="font-mono text-xs text-muted-foreground">
                  since {stats.activeSince}
                </span>
              )}
              <span className="text-xs text-muted-foreground">
                uptime {formatUptime(stats.uptimeSeconds)}
              </span>
              {stats.serverTime && (
                <span className="font-mono text-xs text-muted-foreground">
                  server clock {stats.serverTime}
                </span>
              )}
            </div>

            <div>
              <div className="text-sm text-muted-foreground">Current world</div>
              {stats.gameId ? (
                <div className="flex items-center gap-1.5">
                  <span className="font-mono text-sm">{stats.gameId}</span>
                  <Button
                    variant="ghost"
                    size="icon-xs"
                    title="Copy game id"
                    onClick={() => copyGameId(stats.gameId!)}
                  >
                    <CopyIcon />
                  </Button>
                </div>
              ) : (
                <div className="text-sm text-muted-foreground">no game id captured yet</div>
              )}
            </div>

            <div className="grid grid-cols-1 gap-3 text-sm sm:grid-cols-2">
              <div>
                <div className="text-muted-foreground">Memory</div>
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
                <div className="text-muted-foreground">Disk (worlds)</div>
                <div className="font-mono">
                  {stats.diskUsed ?? "—"} / {stats.diskSize ?? "—"} (
                  {stats.diskUsePct ?? "—"})
                </div>
              </div>
            </div>

            <div>
              <div className="text-sm text-muted-foreground">UDP listening</div>
              {stats.udpListening.length > 0 ? (
                <pre className="overflow-x-auto rounded-md bg-muted p-2 font-mono text-xs">
                  {stats.udpListening.join("\n")}
                </pre>
              ) : (
                <div className="text-sm text-muted-foreground">nothing found</div>
              )}
            </div>

            <div className="flex min-h-0 flex-1 flex-col">
              <div className="mb-1 text-sm text-muted-foreground">Recent log</div>
              <pre className="min-h-24 flex-1 overflow-y-auto overflow-x-hidden rounded-md bg-muted p-2 font-mono text-xs whitespace-pre-wrap">
                {stats.recentLogTail || "(empty)"}
              </pre>
            </div>
          </>
        )}
      </CardContent>
    </Card>
  );
}
