import { useEffect, useRef, useState } from "react";
import { toast } from "sonner";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";

import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import * as api from "@/lib/api";

/// SPEC §16: a real interactive shell over the same SSH connection, not another
/// `exec()` call. Manual start/stop — this is a genuine, unsandboxed root shell, not
/// something to spin up automatically just because the tab exists.
export function TerminalPanel() {
  const containerRef = useRef<HTMLDivElement>(null);
  const [open, setOpen] = useState(false);
  const [starting, setStarting] = useState(false);

  // Best-effort: close the remote session if the whole panel unmounts (e.g. switching
  // away while connected elsewhere doesn't unmount it — Tabs keep content mounted — but
  // disconnecting from the server does, via App's own teardown).
  useEffect(() => {
    return () => {
      api.terminalClose().catch(() => {});
    };
  }, []);

  useEffect(() => {
    if (!open || !containerRef.current) return;

    const term = new Terminal({
      fontFamily: "var(--font-mono), ui-monospace, monospace",
      fontSize: 13,
      cursorBlink: true,
      convertEol: true,
      theme: { background: "#00000000" },
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(containerRef.current);
    fit.fit();

    const dataDisposable = term.onData((data) => {
      api.terminalWrite(data).catch((e) => toast.error(String(e)));
    });

    let cancelled = false;
    const unlistenOutput = api.onTerminalOutput((chunk) => term.write(chunk));
    const unlistenClosed = api.onTerminalClosed(() => {
      if (cancelled) return;
      term.write("\r\n\x1b[31m[session closed]\x1b[0m\r\n");
      setOpen(false);
    });

    const resizeObserver = new ResizeObserver(() => {
      fit.fit();
      api.terminalResize(term.cols, term.rows).catch(() => {});
    });
    resizeObserver.observe(containerRef.current);
    api.terminalResize(term.cols, term.rows).catch(() => {});

    return () => {
      cancelled = true;
      resizeObserver.disconnect();
      dataDisposable.dispose();
      unlistenOutput.then((u) => u());
      unlistenClosed.then((u) => u());
      term.dispose();
    };
  }, [open]);

  async function start() {
    setStarting(true);
    try {
      await api.terminalOpen();
      setOpen(true);
    } catch (e) {
      toast.error(String(e));
    } finally {
      setStarting(false);
    }
  }

  async function stop() {
    try {
      await api.terminalClose();
    } catch (e) {
      toast.error(String(e));
    } finally {
      setOpen(false);
    }
  }

  return (
    <Card className="flex h-full min-h-0 w-full flex-col">
      <CardHeader className="flex flex-row items-center justify-between">
        <div>
          <CardTitle>Terminal</CardTitle>
          <CardDescription>
            A real shell over SSH — same trust level as <code>ssh root@host</code>, not
            sandboxed.
          </CardDescription>
        </div>
        {open ? (
          <Button variant="destructive" size="sm" onClick={stop}>
            Close session
          </Button>
        ) : (
          <Button size="sm" onClick={start} disabled={starting}>
            {starting ? "Starting…" : "Start terminal session"}
          </Button>
        )}
      </CardHeader>
      <CardContent className="min-h-0 flex-1">
        {open ? (
          <div ref={containerRef} className="h-full min-h-64 w-full overflow-hidden rounded-md bg-black p-1" />
        ) : (
          <div className="flex h-full min-h-64 items-center justify-center rounded-md border text-sm text-muted-foreground">
            No active session. Click "Start terminal session" to open a real shell.
          </div>
        )}
      </CardContent>
    </Card>
  );
}
