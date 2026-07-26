import { useState } from "react";
import { toast } from "sonner";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { InfoIcon, FolderOpenIcon, CopyIcon, CopyCheckIcon } from "lucide-react";
import { useTranslation } from "react-i18next";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import type { AboutInfo, CommandRefGroup } from "@/lib/api";
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

function CommandRow({ label, command, targetKind }: { label: string; command: string; targetKind: string | null }) {
  const { t } = useTranslation();

  async function copy() {
    try {
      await writeText(command);
      toast.success(t("about.commandCopied"));
    } catch (e) {
      toast.error(String(e));
    }
  }

  return (
    <div className="flex items-start gap-2 rounded-md border p-2 text-sm">
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-1.5 text-xs text-muted-foreground">
          <span>{label}</span>
          {targetKind && (
            <Badge variant="secondary" className="px-1 py-0 text-[10px] font-normal">
              {targetKind}
            </Badge>
          )}
        </div>
        <div className="break-all font-mono text-xs">{command}</div>
      </div>
      <Button
        variant="outline"
        size="icon-sm"
        className="shrink-0"
        title={t("about.copyCommand")}
        onClick={copy}
      >
        <CopyIcon />
      </Button>
    </div>
  );
}

/// Plain-text rendering of the whole reference — one line per command, grouped under a
/// `== Group ==` header — for the single "copy everything" button.
function formatAllCommands(groups: CommandRefGroup[]): string {
  return groups
    .map((group) => {
      const lines = group.entries.map((e) => {
        const kind = e.targetKind ? ` [${e.targetKind}]` : "";
        return `${e.label}${kind}: ${e.command}`;
      });
      return `== ${group.group} ==\n${lines.join("\n")}`;
    })
    .join("\n\n");
}

export function AboutDialog() {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const [info, setInfo] = useState<AboutInfo | null>(null);
  const [commandRef, setCommandRef] = useState<CommandRefGroup[] | null>(null);

  async function copyAllCommands() {
    if (!commandRef) return;
    try {
      await writeText(formatAllCommands(commandRef));
      toast.success(t("about.commandReference.allCopied"));
    } catch (e) {
      toast.error(String(e));
    }
  }

  async function show() {
    setOpen(true);
    if (!info) {
      try {
        setInfo(await api.getAboutInfo());
      } catch (e) {
        toast.error(String(e));
      }
    }
    if (!commandRef) {
      try {
        setCommandRef(await api.getCommandReference());
      } catch (e) {
        toast.error(String(e));
      }
    }
  }

  return (
    <>
      <Button variant="outline" size="icon-sm" title={t("about.buttonTitle")} onClick={show}>
        <InfoIcon />
      </Button>
      <Dialog open={open} onOpenChange={setOpen}>
        <DialogContent className="flex max-h-[85vh] flex-col sm:max-w-2xl">
          <DialogHeader>
            <DialogTitle>{t("about.title")}</DialogTitle>
            <DialogDescription>
              {info ? t("about.versionDesc", { version: info.version }) : t("about.loading")}
            </DialogDescription>
          </DialogHeader>
          <div className="flex min-h-0 flex-1 flex-col gap-4">
            {info && (
              <div className="grid gap-2">
                <PathRow label={t("about.connectionProfiles")} path={info.configPath} />
                <PathRow label={t("about.auditLog")} path={info.dbPath} />
                <PathRow label={t("about.generatedKeys")} path={info.keysDir} />
              </div>
            )}
            {commandRef && (
              <div className="flex min-h-0 flex-1 flex-col gap-3">
                <div className="flex items-start justify-between gap-2">
                  <div>
                    <div className="text-sm font-medium">{t("about.commandReference.title")}</div>
                    <p className="text-xs text-muted-foreground">{t("about.commandReference.desc")}</p>
                  </div>
                  <Button
                    variant="outline"
                    size="sm"
                    className="shrink-0"
                    title={t("about.commandReference.copyAll")}
                    onClick={copyAllCommands}
                  >
                    <CopyCheckIcon />
                    {t("about.commandReference.copyAll")}
                  </Button>
                </div>
                <div className="min-h-0 flex-1 overflow-y-auto pr-1">
                  <div className="grid gap-3">
                    {commandRef.map((group) => (
                      <div key={group.group} className="grid gap-1.5">
                        <div className="text-xs font-semibold text-muted-foreground uppercase tracking-wide">
                          {group.group}
                        </div>
                        <div className="grid gap-1.5">
                          {group.entries.map((entry, i) => (
                            <CommandRow
                              key={i}
                              label={entry.label}
                              command={entry.command}
                              targetKind={entry.targetKind}
                            />
                          ))}
                        </div>
                      </div>
                    ))}
                  </div>
                </div>
              </div>
            )}
          </div>
        </DialogContent>
      </Dialog>
    </>
  );
}
