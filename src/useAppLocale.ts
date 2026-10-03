import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { messages, resolveLocale, type Locale } from "./settingsI18n";

type UiLocale = { locale: Locale; systemLocale: Locale };

export function useAppLocale() {
  const [state, setState] = useState<UiLocale>(() => {
    const systemLocale = window.__OVERTEXT__?.systemLocale ?? resolveLocale("system", navigator.languages);
    return { locale: window.__OVERTEXT__?.uiLocale ?? systemLocale, systemLocale };
  });
  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    // Subscribe before fetching so a concurrent save cannot leave this window stale.
    void listen<UiLocale>("ui-language-changed", (event) => {
      if (!disposed) setState(event.payload);
    }).then(async (stop) => {
      if (disposed) { stop(); return; }
      unlisten = stop;
      const locale = await invoke<UiLocale>("get_ui_locale");
      if (!disposed) setState(locale);
    }).catch(() => {});
    return () => { disposed = true; unlisten?.(); };
  }, []);
  useEffect(() => { document.documentElement.lang = state.locale; }, [state.locale]);
  return { ...state, t: messages[state.locale] };
}
