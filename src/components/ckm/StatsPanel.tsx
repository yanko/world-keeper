import { useEffect, useState } from "react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
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

export function StatsPanel() {
  const [stats, setStats] = useState<ServerStats | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function refresh() {
    try {
      const s = await api.getStats();
      setStats(s);
      setError(null);
    } catch (e) {
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
        <Button variant="outline" size="sm" onClick={refresh}>
          Refresh
        </Button>
      </CardHeader>
      <CardContent className="flex min-h-0 flex-1 flex-col gap-4">
        {error && <div className="text-sm text-destructive">{error}</div>}
        {stats && (
          <>
            <div className="flex flex-wrap items-center gap-3">
              <Badge variant={statusVariant(stats.serviceStatus)}>
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
