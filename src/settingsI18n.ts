import catalog from "./locales/messages.json";
export type UiLanguage = "system" | "vi" | "en" | "ja";
export type Locale = Exclude<UiLanguage, "system">;
export type MessageKey = keyof typeof catalog.en;
export const messages: Record<Locale, Record<MessageKey, string>> = catalog;

export function resolveLocale(choice: string, systemLanguages: readonly string[]): Locale {
  if (choice === "vi" || choice === "en" || choice === "ja") return choice;
  for (const tag of systemLanguages) {
    const language = tag.toLowerCase().split("-")[0];
    if (language === "vi" || language === "en" || language === "ja") return language;
  }
  return "en";
}

export function languageName(code: string, fallback: string, locale: Locale): string {
  try {
    return new Intl.DisplayNames([locale], { type: "language" }).of(code) ?? fallback;
  } catch {
    return fallback;
  }
}

// Windows may report "ja" for a language selected as "ja-JP".
export function nativeOcrAvailable(code: string, installed: readonly string[]): boolean {
  const requested = code.toLowerCase();
  return installed.some((tag) => requested === tag.toLowerCase() || requested.startsWith(`${tag.toLowerCase()}-`));
}
