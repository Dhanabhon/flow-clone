import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { useEffect, useRef } from "react";
import { ask } from "@tauri-apps/plugin-dialog";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Routes } from "@/routes";
import { ShellControls } from "@/components/flowclone/ShellControls";
import { isTauriRuntime } from "@/lib/tauri";
import { useI18n } from "@/lib/i18n";
import { useFlowStore } from "@/stores/flow-store";
import { useLocaleStore } from "@/stores/locale-store";
import { useThemeStore } from "@/stores/theme-store";

const queryClient = new QueryClient({
  defaultOptions: { queries: { refetchOnWindowFocus: false } },
});

/**
 * App shell. The UI is a presenter only — all cloning happens in the Rust
 * core via Tauri commands. See `src/lib/tauri.ts`.
 */
export default function App() {
  // Apply the persisted theme on mount. The store's setter keeps the DOM
  // in sync thereafter; this effect handles the first paint and any later
  // OS-prefers change only when the user hasn't made an explicit choice.
  const theme = useThemeStore((s) => s.theme);
  const locale = useLocaleStore((s) => s.locale);
  useEffect(() => {
    document.documentElement.classList.toggle("dark", theme === "dark");
  }, [theme]);
  useEffect(() => {
    document.documentElement.lang = locale;
  }, [locale]);

  // Suppress the WebView's right-click menu (Reload / Back / Forward) so the
  // desktop app doesn't expose browser chrome. Editable fields keep their menu
  // so Cut/Copy/Paste still works (e.g. the ERASE confirmation input).
  useEffect(() => {
    const onContextMenu = (event: MouseEvent) => {
      const target = event.target as HTMLElement | null;
      if (target?.closest('input, textarea, [contenteditable="true"]')) return;
      event.preventDefault();
    };
    document.addEventListener("contextmenu", onContextMenu);
    return () => document.removeEventListener("contextmenu", onContextMenu);
  }, []);

  // Warn before closing the window while a migration/restore is running, so a
  // stray Cmd-Q doesn't silently interrupt a long job. `t` is read through a ref
  // so the listener is set up once but always speaks the current language.
  const { t } = useI18n();
  const tRef = useRef(t);
  tRef.current = t;
  useEffect(() => {
    if (!isTauriRuntime()) return;
    let active = true;
    let unlisten: (() => void) | undefined;
    getCurrentWindow()
      .onCloseRequested(async (event) => {
        if (useFlowStore.getState().phase !== "cloning") return;
        const tt = tRef.current;
        event.preventDefault();
        const confirmed = await ask(tt("closeDuringJobBody"), {
          title: tt("closeDuringJobTitle"),
          kind: "warning",
          okLabel: tt("closeAnyway"),
          cancelLabel: tt("keepRunning"),
        });
        if (confirmed) await getCurrentWindow().destroy();
      })
      .then((fn) => {
        if (active) unlisten = fn;
        else fn();
      });
    return () => {
      active = false;
      unlisten?.();
    };
  }, []);

  return (
    <QueryClientProvider client={queryClient}>
      <ShellControls />
      <Routes />
    </QueryClientProvider>
  );
}
