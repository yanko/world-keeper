import { useState } from "react";
import { toast } from "sonner";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { InfoIcon, FolderOpenIcon } from "lucide-react";
import { useTranslation } from "react-i18next";

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
  const { t } = useTranslation();
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
        title={t("about.showInFolder")}
        onClick={() => reveal(path)}
      >
        <FolderOpenIcon />
      </Button>
    </div>
  );
}

export function AboutDialog() {
  const { t } = useTranslation();
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
      <Button variant="outline" size="icon-sm" title={t("about.buttonTitle")} onClick={show}>
        <InfoIcon />
      </Button>
      <Dialog open={open} onOpenChange={setOpen}>
        <DialogContent className="sm:max-w-xl">
          <DialogHeader>
            <DialogTitle>{t("about.title")}</DialogTitle>
            <DialogDescription>
              {info ? t("about.versionDesc", { version: info.version }) : t("about.loading")}
            </DialogDescription>
          </DialogHeader>
          {info && (
            <div className="grid gap-2">
              <PathRow label={t("about.connectionProfiles")} path={info.configPath} />
              <PathRow label={t("about.auditLog")} path={info.dbPath} />
              <PathRow label={t("about.generatedKeys")} path={info.keysDir} />
            </div>
          )}
        </DialogContent>
      </Dialog>
    </>
  );
}
