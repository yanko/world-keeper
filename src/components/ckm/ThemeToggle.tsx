import { useEffect, useState } from "react";
import { useTheme } from "next-themes";
import { Monitor, Moon, Sun } from "lucide-react";

import { Button } from "@/components/ui/button";

const ORDER = ["system", "light", "dark"] as const;
const ICON = { system: Monitor, light: Sun, dark: Moon } as const;

export function ThemeToggle() {
  const { theme, setTheme } = useTheme();
  // Avoid a light/dark flash on first paint: `theme` is undefined until next-themes
  // reads localStorage/the media query on mount.
  const [mounted, setMounted] = useState(false);
  useEffect(() => setMounted(true), []);

  const current = (mounted ? theme : "system") as (typeof ORDER)[number] | undefined;
  const Icon = ICON[current ?? "system"];

  function cycle() {
    const idx = ORDER.indexOf(current ?? "system");
    setTheme(ORDER[(idx + 1) % ORDER.length]);
  }

  return (
    <Button variant="ghost" size="icon" onClick={cycle} title={`Theme: ${current ?? "system"}`}>
      <Icon />
    </Button>
  );
}
