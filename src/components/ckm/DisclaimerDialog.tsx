import { useEffect, useState } from "react";

import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";

// Bump the suffix if the warning's substance ever changes materially — that re-shows it
// once, rather than relying on everyone who already dismissed v1 to notice a new risk.
const STORAGE_KEY = "ckm.disclaimer-acknowledged.v1";

export function DisclaimerDialog() {
  const [open, setOpen] = useState(false);

  useEffect(() => {
    if (localStorage.getItem(STORAGE_KEY) !== "1") {
      setOpen(true);
    }
  }, []);

  function acknowledge() {
    localStorage.setItem(STORAGE_KEY, "1");
    setOpen(false);
  }

  return (
    <Dialog open={open} onOpenChange={(v) => !v && acknowledge()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Before you start</DialogTitle>
          <DialogDescription className="space-y-2 text-foreground">
            <p>
              This tool stops your Core Keeper server and overwrites world files on a
              remote host you control. It backs up before every destructive step and
              rolls back on failure — but it's a homelab tool, not a certified backup
              product, and it comes with{" "}
              <strong>no guarantee against data loss or corruption</strong>.
            </p>
            <p>Keep your own independent backups of anything you can't afford to lose.</p>
          </DialogDescription>
        </DialogHeader>
        <DialogFooter>
          <Button onClick={acknowledge}>I understand</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
