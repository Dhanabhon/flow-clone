import { Moon, Sun } from "lucide-react";
import { Switch } from "@/components/ui/switch";
import { useI18n } from "@/lib/i18n";
import { cn } from "@/lib/utils";
import { useThemeStore } from "@/stores/theme-store";

/**
 * Dark/light theme toggle. Lives inside the shell controls toolbar.
 */
export function ThemeToggle() {
  const theme = useThemeStore((s) => s.theme);
  const setTheme = useThemeStore((s) => s.setTheme);
  const { t } = useI18n();
  const isDark = theme === "dark";

  return (
    <div className="relative h-8 w-[118px] shrink-0 rounded-pill">
      <Sun
        className={cn(
          "absolute left-[18px] top-1/2 h-[18px] w-[18px] -translate-x-1/2 -translate-y-1/2 transition-colors",
          isDark ? "text-muted" : "text-primary"
        )}
        strokeWidth={2}
      />
      <div className="absolute left-1/2 top-1/2 grid h-6 w-11 -translate-x-1/2 -translate-y-1/2 place-items-center">
        <Switch
          checked={isDark}
          onCheckedChange={(c) => setTheme(c ? "dark" : "light")}
          aria-label={t("themeToggle")}
        />
      </div>
      <Moon
        className={cn(
          "absolute left-[100px] top-1/2 h-[18px] w-[18px] -translate-x-1/2 -translate-y-1/2 transition-colors",
          isDark ? "text-primary" : "text-muted"
        )}
        strokeWidth={2}
      />
    </div>
  );
}
