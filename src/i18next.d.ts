// Typed translation keys: augments react-i18next so `t("some.made.up.key")` is a
// compile error rather than a silent runtime fallback to the raw key string.
import "i18next";

import type en from "@/locales/en.json";

declare module "i18next" {
  interface CustomTypeOptions {
    defaultNS: "translation";
    resources: {
      translation: typeof en;
    };
    returnNull: false;
  }
}
