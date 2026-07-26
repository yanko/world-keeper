import { useState } from "react";
import { toast } from "sonner";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { InfoIcon, FolderOpenIcon } from "lucide-react";

import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import type { AboutInfo } from "@/lib/api";
import * as api from "@/lib/api";

async function reveal(path: string) {
  try {
    await revealItemInDir(path);
  } catch (e) {
    toast.error(String(e));
  }
}

function PathRow({ label, path }: { label: string; path: string }) {
  return (
    <div className="flex items-center gap-2 rounded-md border p-2 text-sm">
      <div className="min-w-0 flex-1">
        <div className="text-xs text-muted-foreground">{label}</div>
        <div className="truncate font-mono text-xs">{path}</div>
      </div>
      <Button
        variant="outline"
        size="icon-sm"
        className="shrink-0"
        title="Show in folder"
        onClick={() => reveal(path)}
      >
        <FolderOpenIcon />
      </Button>
    </div>
  );
}

export function AboutDialog() {
  const [open, setOpen] = useState(false);
  const [info, setInfo] = useState<AboutInfo | null>(null);

  async function show() {
    setOpen(true);
    if (info) return;
    try {
      setInfo(await api.getAboutInfo());
    } catch (e) {
      toast.error(String(e));
    }
  }

  return (
    <>
      <Button variant="outline" size="icon-sm" title="About" onClick={show}>
        <InfoIcon />
      </Button>
      <Dialog open={open} onOpenChange={setOpen}>
        <DialogContent className="sm:max-w-xl">
          <DialogHeader>
            <DialogTitle>Core Keeper World Manager</DialogTitle>
            <DialogDescription>
              {info ? `Version ${info.version}` : "Loading…"} — MIT licensed. A
              homelab tool for managing a Core Keeper dedicated server's worlds over
              SSH; see the project's GitHub repo for source and docs.
            </DialogDescription>
          </DialogHeader>
          {info && (
            <div className="grid gap-2">
              <PathRow label="Connection profiles" path={info.configPath} />
              <PathRow label="Audit log (SQLite)" path={info.dbPath} />
              <PathRow label="Generated SSH keys" path={info.keysDir} />
            </div>
          )}
        </DialogContent>
      </Dialog>
    </>
  );
}
