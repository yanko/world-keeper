// react-i18next setup. Resources are bundled at build time (imported directly, not
// fetched) since this is a desktop app with no server to host locale files on — the
// whole point of `i18next-http-backend` (fetch-on-demand) doesn't apply here.
import i18n from "i18next";
import { initReactI18next } from "react-i18next";

// Auto-discovers every locale file in this folder at build time — adding a new
// language (e.g. Spanish) is just "drop `es.json` in `src/locales/`," nothing else to
// touch: no array to update, no component to edit. `eager: true` bundles them all
// upfront rather than lazy-loading, which is fine at this app's size.
const localeModules = import.meta.glob<{ default: Record<string, unknown> }>(
  "../locales/*.json",
  { eager: true }
);

const resources: Record<string, { translation: Record<string, unknown> }> = {};
for (const path in localeModules) {
  const code = path.match(/([^/\\]+)\.json$/)?.[1];
  if (code) resources[code] = { translation: localeModules[path].default };
}

export type Language = string;
/** `"system"` means "follow the OS/browser locale" — not itself a language, so it's
 * kept out of `SUPPORTED_LANGUAGES` (the list of languages that actually have a
 * translation bundle) and handled as a separate mode by the toggle. */
export type LanguageMode = "system" | Language;

export const SUPPORTED_LANGUAGES: Language[] = Object.keys(resources).sort();

const STORAGE_KEY = "ckm.language";

function isSupported(code: string): boolean {
  return SUPPORTED_LANGUAGES.includes(code);
}

/** Matches the OS/browser locale against installed languages (comparing just the
 * primary subtag, e.g. `fr` out of `fr-FR`), falling back to English — or whatever's
 * first alphabetically, if this build somehow doesn't include English — when nothing
 * installed matches. */
export function resolveSystemLanguage(): Language {
  for (const tag of navigator.languages ?? [navigator.language]) {
    const primary = tag.split("-")[0].toLowerCase();
    if (isSupported(primary)) return primary;
  }
  return isSupported("en") ? "en" : SUPPORTED_LANGUAGES[0];
}

export function getStoredLanguageMode(): LanguageMode {
  const stored = localStorage.getItem(STORAGE_KEY);
  if (stored === "system") return "system";
  if (stored && isSupported(stored)) return stored;
  return "system";
}

function effectiveLanguage(mode: LanguageMode): Language {
  return mode === "system" ? resolveSystemLanguage() : mode;
}

export function setLanguageMode(mode: LanguageMode) {
  localStorage.setItem(STORAGE_KEY, mode);
  i18n.changeLanguage(effectiveLanguage(mode));
}

/** A language's own name for itself (e.g. `"Français"`), read directly out of that
 * language's own resource bundle — never translated into whatever's currently active,
 * which is the standard convention for language switchers. Falls back to the language
 * code itself if a bundle is somehow missing its `meta.nativeName`. */
export function nativeNameOf(lang: Language): string {
  const name = i18n.getResource(lang, "translation", "meta.nativeName") as string | undefined;
  return name ?? lang.toUpperCase();
}

i18n.use(initReactI18next).init({
  resources,
  lng: effectiveLanguage(getStoredLanguageMode()),
  fallbackLng: "en",
  interpolation: { escapeValue: false }, // React already escapes rendered output
  returnNull: false,
});

export default i18n;
