import { useEffect, useState } from "react";
import { Trans, useTranslation } from "react-i18next";

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
  const { t } = useTranslation();
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
          <DialogTitle>{t("disclaimer.title")}</DialogTitle>
          <DialogDescription className="space-y-2 text-foreground">
            <p>
              <Trans i18nKey="disclaimer.body1" components={{ 1: <strong /> }} />
            </p>
            <p>{t("disclaimer.body2")}</p>
          </DialogDescription>
        </DialogHeader>
        <DialogFooter>
          <Button onClick={acknowledge}>{t("disclaimer.understand")}</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
