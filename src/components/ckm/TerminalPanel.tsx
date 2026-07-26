import { useEffect, useRef, useState } from "react";
import { toast } from "sonner";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import * as api from "@/lib/api";
import { useOperations } from "@/lib/operations";

/// SPEC §16: a real interactive shell over the same SSH connection, not another
/// `exec()` call. Manual start/stop — this is a genuine, unsandboxed root shell, not
/// something to spin up automatically just because the tab exists.
export function TerminalPanel() {
  const { t } = useTranslation();
  const containerRef = useRef<HTMLDivElement>(null);
  const [open, setOpen] = useState(false);
  const [starting, setStarting] = useState(false);
  const { activeOperation } = useOperations();

  // Best-effort: close the remote session if the whole panel unmounts. Base UI's Tabs
  // unmounts inactive panels by default, so switching to another tab tears this down —
  // that's fine for the terminal itself (reopening is cheap), unlike push/pull/restore
  // progress, which is why *that* state was moved up into a provider instead.
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
      term.write(`\r\n\x1b[31m${t("terminal.sessionClosed")}\x1b[0m\r\n`);
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
    // eslint-disable-next-line react-hooks/exhaustive-deps
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
          <CardTitle>{t("terminal.title")}</CardTitle>
          <CardDescription>{t("terminal.desc")}</CardDescription>
        </div>
        {open ? (
          <Button variant="destructive" size="sm" onClick={stop}>
            {t("terminal.closeSession")}
          </Button>
        ) : (
          <Button size="sm" onClick={start} disabled={starting}>
            {starting ? t("terminal.starting") : t("terminal.startSession")}
          </Button>
        )}
      </CardHeader>
      <CardContent className="flex min-h-0 flex-1 flex-col gap-2">
        {activeOperation && (
          <div className="rounded-md border border-destructive/40 bg-destructive/10 p-2 text-xs text-destructive">
            {t("terminal.otherOpBanner", { operation: t(`operations.${activeOperation}`) })}
          </div>
        )}
        {open ? (
          <div ref={containerRef} className="min-h-64 w-full flex-1 overflow-hidden rounded-md bg-black p-1" />
        ) : (
          <div className="flex min-h-64 flex-1 items-center justify-center rounded-md border text-sm text-muted-foreground">
            {t("terminal.noActiveSession")}
          </div>
        )}
      </CardContent>
    </Card>
  );
}
