import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import * as api from "@/lib/api";

const POLL_MS = 2000;
const LINES = 300;

/// SPEC §10's "live log drawer, pop-open." Deliberately polling rather than a true
/// `journalctl -f`/`docker logs -f` stream — that would need a persistent SSH channel
/// piped through Tauri events with its own start/stop lifecycle, a meaningfully bigger
/// addition than this phase's scope. Polling every 2s over the existing `logs_tail`
/// gets 90% of the "watch it happen" feel for a fraction of the complexity.
export function LogDrawer() {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const [text, setText] = useState("");
  const [error, setError] = useState<string | null>(null);
  const scrollRef = useRef<HTMLPreElement>(null);

  useEffect(() => {
    if (!open) return;
    let cancelled = false;

    async function poll() {
      try {
        const tail = await api.getLogTail(LINES);
        if (!cancelled) {
          setText(tail);
          setError(null);
        }
      } catch (e) {
        if (!cancelled) setError(String(e));
      }
    }

    poll();
    const id = setInterval(poll, POLL_MS);
    return () => {
      cancelled = true;
      clearInterval(id);
    };
  }, [open]);

  useEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    // Only auto-scroll if already near the bottom — don't yank the view out from
    // under someone scrolling back through history.
    const nearBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 80;
    if (nearBottom) el.scrollTop = el.scrollHeight;
  }, [text]);

  return (
    <>
      <Button variant="outline" size="sm" onClick={() => setOpen(true)}>
        {t("logDrawer.trigger")}
      </Button>
      <Dialog open={open} onOpenChange={setOpen}>
        <DialogContent className="flex max-h-[85vh] flex-col overflow-hidden sm:max-w-3xl">
          <DialogHeader>
            <DialogTitle>{t("logDrawer.title")}</DialogTitle>
            <DialogDescription>
              {t("logDrawer.desc", { seconds: POLL_MS / 1000 })}
            </DialogDescription>
          </DialogHeader>
          {error && <div className="text-sm text-destructive">{error}</div>}
          <pre
            ref={scrollRef}
            className="min-h-0 flex-1 overflow-y-auto overflow-x-hidden rounded-md bg-muted p-3 font-mono text-xs whitespace-pre-wrap"
          >
            {text || t("logDrawer.empty")}
          </pre>
        </DialogContent>
      </Dialog>
    </>
  );
}
