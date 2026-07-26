import { Fragment, useEffect, useState } from "react";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";

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
  const { t } = useTranslation();
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
      toast.success(t("audit.toasts.cleared"));
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
      toast.success(t("audit.toasts.exported", { path: destination }));
    } catch (e) {
      toast.error(String(e));
    } finally {
      setExporting(false);
    }
  }

  return (
    <Card className="flex h-full min-h-0 w-full flex-col">
      <CardHeader className="flex flex-row items-center justify-between">
        <CardTitle>{t("audit.title")}</CardTitle>
        <div className="flex gap-2">
          <Button variant="outline" size="sm" onClick={doExport} disabled={exporting}>
            {exporting ? t("audit.exporting") : t("audit.export")}
          </Button>
          <Button
            variant="destructive"
            size="sm"
            onClick={() => setConfirm1Open(true)}
            disabled={clearing}
          >
            {clearing ? t("audit.clearing") : t("audit.clearLog")}
          </Button>
          <Button variant="outline" size="sm" onClick={() => setRefreshKey((k) => k + 1)}>
            {t("audit.refresh")}
          </Button>
        </div>
      </CardHeader>
      <CardContent className="flex min-h-0 flex-1 flex-col gap-3">
        <Tabs defaultValue="events" className="flex min-h-0 flex-1 flex-col gap-3">
          <TabsList>
            <TabsTrigger value="events">{t("audit.tabs.events")}</TabsTrigger>
            <TabsTrigger value="commands">{t("audit.tabs.commands")}</TabsTrigger>
            <TabsTrigger value="errors">{t("audit.tabs.errors")}</TabsTrigger>
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
            <DialogTitle>{t("audit.confirm1.title")}</DialogTitle>
            <DialogDescription>{t("audit.confirm1.desc")}</DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setConfirm1Open(false)}>
              {t("audit.confirm1.cancel")}
            </Button>
            <Button
              variant="destructive"
              onClick={() => {
                setConfirm1Open(false);
                setConfirm2Open(true);
              }}
            >
              {t("audit.confirm1.confirm")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* Confirm #2: the "no really" one */}
      <Dialog open={confirm2Open} onOpenChange={setConfirm2Open}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t("audit.confirm2.title")}</DialogTitle>
            <DialogDescription>{t("audit.confirm2.desc")}</DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setConfirm2Open(false)}>
              {t("audit.confirm2.cancel")}
            </Button>
            <Button variant="destructive" onClick={confirmClear}>
              {t("audit.confirm2.confirm")}
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
  const { t } = useTranslation();
  return (
    <Select value={value} onValueChange={(v) => onChange(v ?? ALL)}>
      <SelectTrigger className="max-w-full min-w-32">
        <SelectValue placeholder={t("audit.filters.sessionPlaceholder")} />
      </SelectTrigger>
      <SelectContent className="w-auto min-w-72" alignItemWithTrigger={false}>
        <SelectItem value={ALL}>{t("audit.filters.allSessions")}</SelectItem>
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
  const { t } = useTranslation();
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
            <SelectValue placeholder={t("audit.filters.kindPlaceholder")} />
          </SelectTrigger>
          <SelectContent className="w-auto min-w-48" alignItemWithTrigger={false}>
            <SelectItem value={ALL}>{t("audit.filters.allKinds")}</SelectItem>
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
            <SelectValue placeholder={t("audit.filters.statusPlaceholder")} />
          </SelectTrigger>
          <SelectContent className="w-auto min-w-40" alignItemWithTrigger={false}>
            <SelectItem value={ALL}>{t("audit.filters.anyStatus")}</SelectItem>
            <SelectItem value="ok">{t("audit.filters.ok")}</SelectItem>
            <SelectItem value="failed">{t("audit.filters.failed")}</SelectItem>
          </SelectContent>
        </Select>

        <Input
          placeholder={t("audit.filters.searchEventsPlaceholder")}
          className="w-48"
          value={search}
          onChange={(e) => setSearch(e.target.value)}
        />
        <Input
          placeholder={t("audit.filters.sincePlaceholder")}
          className="w-44"
          value={since}
          onChange={(e) => setSince(e.target.value)}
        />
        <Input
          placeholder={t("audit.filters.untilPlaceholder")}
          className="w-44"
          value={until}
          onChange={(e) => setUntil(e.target.value)}
        />
      </div>

      <div className="min-h-0 flex-1 overflow-auto rounded-md border">
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>{t("audit.table.time")}</TableHead>
              <TableHead>{t("audit.table.kind")}</TableHead>
              <TableHead>{t("audit.table.state")}</TableHead>
              <TableHead>{t("audit.table.status")}</TableHead>
              <TableHead>{t("audit.table.detail")}</TableHead>
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
                        {e.ok ? t("audit.filters.ok") : t("audit.filters.failed")}
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
                  {t("audit.noEventsMatch")}
                </TableCell>
              </TableRow>
            )}
          </TableBody>
        </Table>
      </div>
      {loading && <p className="text-xs text-muted-foreground">{t("audit.loading")}</p>}
    </div>
  );
}

function CommandsView({ sessions, refreshKey }: { sessions: SessionRow[]; refreshKey: number }) {
  const { t } = useTranslation();
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
            <SelectValue placeholder={t("audit.filters.exitStatusPlaceholder")} />
          </SelectTrigger>
          <SelectContent className="w-auto min-w-40" alignItemWithTrigger={false}>
            <SelectItem value={ALL}>{t("audit.filters.anyExitStatus")}</SelectItem>
            <SelectItem value="ok">{t("audit.filters.exit0")}</SelectItem>
            <SelectItem value="failed">{t("audit.filters.nonzeroExit")}</SelectItem>
          </SelectContent>
        </Select>

        <Input
          placeholder={t("audit.filters.searchCommandsPlaceholder")}
          className="w-64"
          value={search}
          onChange={(e) => setSearch(e.target.value)}
        />
      </div>

      <div className="min-h-0 flex-1 overflow-auto rounded-md border">
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>{t("audit.table.time")}</TableHead>
              <TableHead>{t("audit.table.command")}</TableHead>
              <TableHead>{t("audit.table.exit")}</TableHead>
              <TableHead>{t("audit.table.duration")}</TableHead>
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
                      <Badge variant="destructive">{t("audit.noResponse")}</Badge>
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
                          <div className="text-xs font-medium text-muted-foreground">{t("audit.stdout")}</div>
                          <pre className="max-h-64 overflow-auto font-mono text-xs whitespace-pre-wrap">
                            {c.stdout || t("audit.empty")}
                          </pre>
                        </div>
                        <div>
                          <div className="text-xs font-medium text-muted-foreground">{t("audit.stderr")}</div>
                          <pre className="max-h-64 overflow-auto font-mono text-xs whitespace-pre-wrap">
                            {c.stderr || t("audit.empty")}
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
                  {t("audit.noCommandsMatch")}
                </TableCell>
              </TableRow>
            )}
          </TableBody>
        </Table>
      </div>
      {loading && <p className="text-xs text-muted-foreground">{t("audit.loading")}</p>}
    </div>
  );
}

function ErrorsView({ sessions, refreshKey }: { sessions: SessionRow[]; refreshKey: number }) {
  const { t } = useTranslation();
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
            <SelectValue placeholder={t("audit.filters.contextPlaceholder")} />
          </SelectTrigger>
          <SelectContent className="w-auto min-w-48" alignItemWithTrigger={false}>
            <SelectItem value={ALL}>{t("audit.filters.allContexts")}</SelectItem>
            {contexts.map((c) => (
              <SelectItem key={c} value={c}>
                {c}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>

        <SessionSelect sessions={sessions} value={sessionId} onChange={setSessionId} />

        <Input
          placeholder={t("audit.filters.searchErrorsPlaceholder")}
          className="w-64"
          value={search}
          onChange={(e) => setSearch(e.target.value)}
        />
      </div>

      <div className="min-h-0 flex-1 overflow-auto rounded-md border">
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>{t("audit.table.time")}</TableHead>
              <TableHead>{t("audit.table.context")}</TableHead>
              <TableHead>{t("audit.table.message")}</TableHead>
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
                  {t("audit.noErrorsMatch")}
                </TableCell>
              </TableRow>
            )}
          </TableBody>
        </Table>
      </div>
      {loading && <p className="text-xs text-muted-foreground">{t("audit.loading")}</p>}
    </div>
  );
}
