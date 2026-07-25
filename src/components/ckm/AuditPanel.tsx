import { Fragment, useEffect, useState } from "react";
import { toast } from "sonner";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import type { CommandRow, ErrorRow, EventRow, SessionRow } from "@/lib/api";
import * as api from "@/lib/api";
import { cn } from "@/lib/utils";

const ALL = "__all__";

function sessionLabel(s: SessionRow): string {
  const parts = [s.host, s.sshUser].filter(Boolean);
  return parts.length > 0 ? `#${s.id} — ${parts.join("@")}` : `#${s.id} — ${s.startedAt}`;
}

export function AuditPanel() {
  const [sessions, setSessions] = useState<SessionRow[]>([]);
  const [confirm1Open, setConfirm1Open] = useState(false);
  const [confirm2Open, setConfirm2Open] = useState(false);
  const [clearing, setClearing] = useState(false);
  const [exporting, setExporting] = useState(false);
  const [refreshKey, setRefreshKey] = useState(0);

  useEffect(() => {
    api.listAuditSessions().then(setSessions).catch((e) => toast.error(String(e)));
  }, [refreshKey]);

  async function confirmClear() {
    setConfirm2Open(false);
    setClearing(true);
    try {
      await api.clearAuditLog();
      setRefreshKey((k) => k + 1);
      toast.success("Audit log cleared.");
    } catch (e) {
      toast.error(String(e));
    } finally {
      setClearing(false);
    }
  }

  async function doExport() {
    setExporting(true);
    try {
      const defaultName = `ckmanager-audit-${new Date().toISOString().slice(0, 10)}.zip`;
      const destination = await api.saveAuditZip(defaultName);
      if (!destination) return;
      await api.exportAudit(destination);
      toast.success(`Exported to ${destination}`);
    } catch (e) {
      toast.error(String(e));
    } finally {
      setExporting(false);
    }
  }

  return (
    <Card className="flex h-full min-h-0 w-full flex-col">
      <CardHeader className="flex flex-row items-center justify-between">
        <CardTitle>Audit log</CardTitle>
        <div className="flex gap-2">
          <Button variant="outline" size="sm" onClick={doExport} disabled={exporting}>
            {exporting ? "Exporting…" : "Export (CSV + db)"}
          </Button>
          <Button
            variant="destructive"
            size="sm"
            onClick={() => setConfirm1Open(true)}
            disabled={clearing}
          >
            {clearing ? "Clearing…" : "Clear log"}
          </Button>
          <Button variant="outline" size="sm" onClick={() => setRefreshKey((k) => k + 1)}>
            Refresh
          </Button>
        </div>
      </CardHeader>
      <CardContent className="flex min-h-0 flex-1 flex-col gap-3">
        <Tabs defaultValue="events" className="flex min-h-0 flex-1 flex-col gap-3">
          <TabsList>
            <TabsTrigger value="events">Events</TabsTrigger>
            <TabsTrigger value="commands">SSH commands</TabsTrigger>
            <TabsTrigger value="errors">Errors</TabsTrigger>
          </TabsList>
          <TabsContent value="events" className="flex min-h-0 flex-1 flex-col">
            <EventsView sessions={sessions} refreshKey={refreshKey} />
          </TabsContent>
          <TabsContent value="commands" className="flex min-h-0 flex-1 flex-col">
            <CommandsView sessions={sessions} refreshKey={refreshKey} />
          </TabsContent>
          <TabsContent value="errors" className="flex min-h-0 flex-1 flex-col">
            <ErrorsView sessions={sessions} refreshKey={refreshKey} />
          </TabsContent>
        </Tabs>
      </CardContent>

      {/* Confirm #1: plain */}
      <Dialog open={confirm1Open} onOpenChange={setConfirm1Open}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Clear the entire audit log?</DialogTitle>
            <DialogDescription>
              Deletes every session, event, SSH command, error, file operation, and
              backup record — all of it, not just what's currently filtered. This
              cannot be undone.
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setConfirm1Open(false)}>
              Cancel
            </Button>
            <Button
              variant="destructive"
              onClick={() => {
                setConfirm1Open(false);
                setConfirm2Open(true);
              }}
            >
              Yes, clear it
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* Confirm #2: the "no really" one */}
      <Dialog open={confirm2Open} onOpenChange={setConfirm2Open}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>No, like — are you REALLY sure?</DialogTitle>
            <DialogDescription>
              This is the digital equivalent of shredding the black box right before
              the NTSB shows up. Every "why did the server die at 3am" mystery you
              could've solved — gone. Every rollback you were quietly proud of — never
              happened. The audit log doesn't get better with age, it just dies, and
              you're the one holding the knife. Last chance to back away slowly.
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setConfirm2Open(false)}>
              No, I panicked
            </Button>
            <Button variant="destructive" onClick={confirmClear}>
              Yes. Do it. Let it go quietly.
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </Card>
  );
}

function SessionSelect({
  sessions,
  value,
  onChange,
}: {
  sessions: SessionRow[];
  value: string;
  onChange: (v: string) => void;
}) {
  return (
    <Select value={value} onValueChange={(v) => onChange(v ?? ALL)}>
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
  );
}

function EventsView({ sessions, refreshKey }: { sessions: SessionRow[]; refreshKey: number }) {
  const [kinds, setKinds] = useState<string[]>([]);
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
  }, [refreshKey]);

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
  }, [kind, sessionId, ok, refreshKey]);

  useEffect(() => {
    const id = setTimeout(runQuery, 300);
    return () => clearTimeout(id);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [search, since, until]);

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-3">
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

        <SessionSelect sessions={sessions} value={sessionId} onChange={setSessionId} />

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
      {loading && <p className="text-xs text-muted-foreground">Loading…</p>}
    </div>
  );
}

function CommandsView({ sessions, refreshKey }: { sessions: SessionRow[]; refreshKey: number }) {
  const [sessionId, setSessionId] = useState(ALL);
  const [failedOnly, setFailedOnly] = useState(ALL);
  const [search, setSearch] = useState("");
  const [rows, setRows] = useState<CommandRow[]>([]);
  const [expandedId, setExpandedId] = useState<number | null>(null);
  const [loading, setLoading] = useState(false);

  async function runQuery() {
    setLoading(true);
    try {
      const result = await api.queryAuditCommands({
        sessionId: sessionId === ALL ? undefined : Number(sessionId),
        failedOnly: failedOnly === ALL ? undefined : failedOnly === "failed",
        search: search || undefined,
        limit: 300,
      });
      setRows(result);
    } catch (e) {
      toast.error(String(e));
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    runQuery();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sessionId, failedOnly, refreshKey]);

  useEffect(() => {
    const id = setTimeout(runQuery, 300);
    return () => clearTimeout(id);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [search]);

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-3">
      <div className="flex flex-wrap gap-2">
        <SessionSelect sessions={sessions} value={sessionId} onChange={setSessionId} />

        <Select value={failedOnly} onValueChange={(v) => setFailedOnly(v ?? ALL)}>
          <SelectTrigger className="max-w-full min-w-28">
            <SelectValue placeholder="Exit status" />
          </SelectTrigger>
          <SelectContent className="w-auto min-w-40" alignItemWithTrigger={false}>
            <SelectItem value={ALL}>Any exit status</SelectItem>
            <SelectItem value="ok">Exit 0</SelectItem>
            <SelectItem value="failed">Nonzero exit</SelectItem>
          </SelectContent>
        </Select>

        <Input
          placeholder="Search command/stdout/stderr…"
          className="w-64"
          value={search}
          onChange={(e) => setSearch(e.target.value)}
        />
      </div>

      <div className="min-h-0 flex-1 overflow-auto rounded-md border">
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>Time</TableHead>
              <TableHead>Command</TableHead>
              <TableHead>Exit</TableHead>
              <TableHead>Duration</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {rows.map((c) => (
              <Fragment key={c.id}>
                <TableRow
                  className="cursor-pointer"
                  onClick={() => setExpandedId(expandedId === c.id ? null : c.id)}
                >
                  <TableCell className="font-mono text-xs whitespace-nowrap">{c.ts}</TableCell>
                  <TableCell className="max-w-160 truncate font-mono text-xs">{c.command}</TableCell>
                  <TableCell>
                    {c.exitCode === null ? (
                      <Badge variant="destructive">no response</Badge>
                    ) : (
                      <Badge
                        variant={c.exitCode === 0 ? "default" : "destructive"}
                        className={cn(c.exitCode === 0 && "border-signal bg-signal text-signal-foreground")}
                      >
                        {c.exitCode}
                      </Badge>
                    )}
                  </TableCell>
                  <TableCell className="font-mono text-xs text-muted-foreground">
                    {c.durationMs !== null ? `${c.durationMs} ms` : "—"}
                  </TableCell>
                </TableRow>
                {expandedId === c.id && (
                  <TableRow>
                    <TableCell colSpan={4} className="bg-muted/50">
                      <div className="flex flex-col gap-2">
                        <div>
                          <div className="text-xs font-medium text-muted-foreground">stdout</div>
                          <pre className="max-h-64 overflow-auto font-mono text-xs whitespace-pre-wrap">
                            {c.stdout || "(empty)"}
                          </pre>
                        </div>
                        <div>
                          <div className="text-xs font-medium text-muted-foreground">stderr</div>
                          <pre className="max-h-64 overflow-auto font-mono text-xs whitespace-pre-wrap">
                            {c.stderr || "(empty)"}
                          </pre>
                        </div>
                      </div>
                    </TableCell>
                  </TableRow>
                )}
              </Fragment>
            ))}
            {rows.length === 0 && (
              <TableRow>
                <TableCell colSpan={4} className="text-center text-sm text-muted-foreground">
                  No commands match these filters.
                </TableCell>
              </TableRow>
            )}
          </TableBody>
        </Table>
      </div>
      {loading && <p className="text-xs text-muted-foreground">Loading…</p>}
    </div>
  );
}

function ErrorsView({ sessions, refreshKey }: { sessions: SessionRow[]; refreshKey: number }) {
  const [contexts, setContexts] = useState<string[]>([]);
  const [context, setContext] = useState(ALL);
  const [sessionId, setSessionId] = useState(ALL);
  const [search, setSearch] = useState("");
  const [rows, setRows] = useState<ErrorRow[]>([]);
  const [expandedId, setExpandedId] = useState<number | null>(null);
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    api.listAuditErrorContexts().then(setContexts).catch((e) => toast.error(String(e)));
  }, [refreshKey]);

  async function runQuery() {
    setLoading(true);
    try {
      const result = await api.queryAuditErrors({
        context: context === ALL ? undefined : context,
        sessionId: sessionId === ALL ? undefined : Number(sessionId),
        search: search || undefined,
        limit: 300,
      });
      setRows(result);
    } catch (e) {
      toast.error(String(e));
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    runQuery();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [context, sessionId, refreshKey]);

  useEffect(() => {
    const id = setTimeout(runQuery, 300);
    return () => clearTimeout(id);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [search]);

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-3">
      <div className="flex flex-wrap gap-2">
        <Select value={context} onValueChange={(v) => setContext(v ?? ALL)}>
          <SelectTrigger className="max-w-full min-w-32">
            <SelectValue placeholder="Context" />
          </SelectTrigger>
          <SelectContent className="w-auto min-w-48" alignItemWithTrigger={false}>
            <SelectItem value={ALL}>All contexts</SelectItem>
            {contexts.map((c) => (
              <SelectItem key={c} value={c}>
                {c}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>

        <SessionSelect sessions={sessions} value={sessionId} onChange={setSessionId} />

        <Input
          placeholder="Search context/message/detail…"
          className="w-64"
          value={search}
          onChange={(e) => setSearch(e.target.value)}
        />
      </div>

      <div className="min-h-0 flex-1 overflow-auto rounded-md border">
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>Time</TableHead>
              <TableHead>Context</TableHead>
              <TableHead>Message</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {rows.map((e) => (
              <Fragment key={e.id}>
                <TableRow
                  className="cursor-pointer"
                  onClick={() => setExpandedId(expandedId === e.id ? null : e.id)}
                >
                  <TableCell className="font-mono text-xs whitespace-nowrap">{e.ts}</TableCell>
                  <TableCell>
                    <Badge variant="destructive">{e.context}</Badge>
                  </TableCell>
                  <TableCell className="max-w-160 truncate font-mono text-xs">{e.message}</TableCell>
                </TableRow>
                {expandedId === e.id && (
                  <TableRow>
                    <TableCell colSpan={3} className="bg-muted/50">
                      <pre className="max-h-64 overflow-auto font-mono text-xs whitespace-pre-wrap">
                        {e.message}
                        {e.detailJson ? `\n\n${e.detailJson}` : ""}
                      </pre>
                    </TableCell>
                  </TableRow>
                )}
              </Fragment>
            ))}
            {rows.length === 0 && (
              <TableRow>
                <TableCell colSpan={3} className="text-center text-sm text-muted-foreground">
                  No errors match these filters.
                </TableCell>
              </TableRow>
            )}
          </TableBody>
        </Table>
      </div>
      {loading && <p className="text-xs text-muted-foreground">Loading…</p>}
    </div>
  );
}
