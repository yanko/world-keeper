import { Fragment, useEffect, useState } from "react";
import { toast } from "sonner";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import type { EventRow, SessionRow } from "@/lib/api";
import * as api from "@/lib/api";
import { cn } from "@/lib/utils";

const ALL = "__all__";

function sessionLabel(s: SessionRow): string {
  const parts = [s.host, s.sshUser].filter(Boolean);
  return parts.length > 0 ? `#${s.id} — ${parts.join("@")}` : `#${s.id} — ${s.startedAt}`;
}

export function AuditPanel() {
  const [kinds, setKinds] = useState<string[]>([]);
  const [sessions, setSessions] = useState<SessionRow[]>([]);
  const [kind, setKind] = useState(ALL);
  const [sessionId, setSessionId] = useState(ALL);
  const [ok, setOk] = useState(ALL);
  const [search, setSearch] = useState("");
  const [since, setSince] = useState("");
  const [until, setUntil] = useState("");
  const [events, setEvents] = useState<EventRow[]>([]);
  const [expandedId, setExpandedId] = useState<number | null>(null);
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    api.listAuditEventKinds().then(setKinds).catch((e) => toast.error(String(e)));
    api.listAuditSessions().then(setSessions).catch((e) => toast.error(String(e)));
  }, []);

  async function runQuery() {
    setLoading(true);
    try {
      const rows = await api.queryAuditEvents({
        kind: kind === ALL ? undefined : kind,
        sessionId: sessionId === ALL ? undefined : Number(sessionId),
        ok: ok === ALL ? undefined : ok === "ok",
        search: search || undefined,
        since: since || undefined,
        until: until || undefined,
        limit: 200,
      });
      setEvents(rows);
    } catch (e) {
      toast.error(String(e));
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    runQuery();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [kind, sessionId, ok]);

  useEffect(() => {
    const id = setTimeout(runQuery, 300);
    return () => clearTimeout(id);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [search, since, until]);

  return (
    <Card className="flex h-full min-h-0 w-full flex-col">
      <CardHeader className="flex flex-row items-center justify-between">
        <CardTitle>Audit log</CardTitle>
        <Button variant="outline" size="sm" onClick={runQuery} disabled={loading}>
          {loading ? "Loading…" : "Refresh"}
        </Button>
      </CardHeader>
      <CardContent className="flex min-h-0 flex-1 flex-col gap-3">
        <div className="flex flex-wrap gap-2">
          <Select value={kind} onValueChange={(v) => setKind(v ?? ALL)}>
            <SelectTrigger className="max-w-full min-w-32">
              <SelectValue placeholder="Kind" />
            </SelectTrigger>
            <SelectContent className="w-auto min-w-48" alignItemWithTrigger={false}>
              <SelectItem value={ALL}>All kinds</SelectItem>
              {kinds.map((k) => (
                <SelectItem key={k} value={k}>
                  {k}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>

          <Select value={sessionId} onValueChange={(v) => setSessionId(v ?? ALL)}>
            <SelectTrigger className="max-w-full min-w-32">
              <SelectValue placeholder="Session" />
            </SelectTrigger>
            <SelectContent className="w-auto min-w-72" alignItemWithTrigger={false}>
              <SelectItem value={ALL}>All sessions</SelectItem>
              {sessions.map((s) => (
                <SelectItem key={s.id} value={String(s.id)}>
                  {sessionLabel(s)}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>

          <Select value={ok} onValueChange={(v) => setOk(v ?? ALL)}>
            <SelectTrigger className="max-w-full min-w-28">
              <SelectValue placeholder="Status" />
            </SelectTrigger>
            <SelectContent className="w-auto min-w-40" alignItemWithTrigger={false}>
              <SelectItem value={ALL}>Any status</SelectItem>
              <SelectItem value="ok">OK</SelectItem>
              <SelectItem value="failed">Failed</SelectItem>
            </SelectContent>
          </Select>

          <Input
            placeholder="Search kind/state/detail…"
            className="w-48"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
          />
          <Input
            placeholder="Since (ISO8601)"
            className="w-44"
            value={since}
            onChange={(e) => setSince(e.target.value)}
          />
          <Input
            placeholder="Until (ISO8601)"
            className="w-44"
            value={until}
            onChange={(e) => setUntil(e.target.value)}
          />
        </div>

        <div className="min-h-0 flex-1 overflow-auto rounded-md border">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Time</TableHead>
                <TableHead>Kind</TableHead>
                <TableHead>State</TableHead>
                <TableHead>Status</TableHead>
                <TableHead>Detail</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {events.map((e) => (
                <Fragment key={e.id}>
                  <TableRow
                    className="cursor-pointer"
                    onClick={() => setExpandedId(expandedId === e.id ? null : e.id)}
                  >
                    <TableCell className="font-mono text-xs whitespace-nowrap">{e.ts}</TableCell>
                    <TableCell className="text-xs">{e.kind}</TableCell>
                    <TableCell className="font-mono text-xs">{e.state ?? "—"}</TableCell>
                    <TableCell>
                      {e.ok === null ? (
                        <span className="text-xs text-muted-foreground">—</span>
                      ) : (
                        <Badge variant={e.ok ? "default" : "destructive"} className={cn(e.ok && "border-signal bg-signal text-signal-foreground")}>
                          {e.ok ? "ok" : "failed"}
                        </Badge>
                      )}
                    </TableCell>
                    <TableCell className="max-w-96 truncate font-mono text-xs text-muted-foreground">
                      {e.detailJson ? JSON.stringify(e.detailJson) : ""}
                    </TableCell>
                  </TableRow>
                  {expandedId === e.id && e.detailJson != null && (
                    <TableRow>
                      <TableCell colSpan={5} className="bg-muted/50">
                        <pre className="max-h-64 overflow-auto font-mono text-xs whitespace-pre-wrap">
                          {JSON.stringify(e.detailJson, null, 2)}
                        </pre>
                      </TableCell>
                    </TableRow>
                  )}
                </Fragment>
              ))}
              {events.length === 0 && (
                <TableRow>
                  <TableCell colSpan={5} className="text-center text-sm text-muted-foreground">
                    No events match these filters.
                  </TableCell>
                </TableRow>
              )}
            </TableBody>
          </Table>
        </div>
      </CardContent>
    </Card>
  );
}
