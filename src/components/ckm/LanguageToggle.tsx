import { useState } from "react";
import { LanguagesIcon } from "lucide-react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import {
  SUPPORTED_LANGUAGES,
  getStoredLanguageMode,
  nativeNameOf,
  resolveSystemLanguage,
  setLanguageMode,
  type LanguageMode,
} from "@/lib/i18n";

// Mirrors ThemeToggle's system → light → dark cycle-on-click pattern, just with a
// dynamically-discovered list of languages instead of a fixed three states.
const CYCLE: LanguageMode[] = ["system", ...SUPPORTED_LANGUAGES];

export function LanguageToggle() {
  const { t } = useTranslation();
  const [mode, setMode] = useState<LanguageMode>(() => getStoredLanguageMode());

  function cycle() {
    const next = CYCLE[(CYCLE.indexOf(mode) + 1) % CYCLE.length];
    setMode(next);
    setLanguageMode(next);
  }

  const effective = mode === "system" ? resolveSystemLanguage() : mode;
  const nativeName = nativeNameOf(effective);
  const title =
    mode === "system" ? `${t("language.toggleTitle")} — ${t("language.system")} (${nativeName})` : nativeName;

  return (
    <Button variant="outline" size="icon-sm" onClick={cycle} title={title}>
      {mode === "system" ? (
        <LanguagesIcon className="size-4" />
      ) : (
        <span className="text-xs font-medium">{mode.toUpperCase()}</span>
      )}
    </Button>
  );
}
