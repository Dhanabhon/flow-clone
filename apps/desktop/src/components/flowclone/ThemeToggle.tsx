import { Moon, Sun } from "lucide-react";
import { Switch } from "@/components/ui/switch";
import { cn } from "@/lib/utils";
import { useThemeStore } from "@/stores/theme-store";

/**
 * Dark/light theme toggle. Sits in the top-right corner as a glassy pill so
 * it stays available across all four screens (Home, Confirmation, Cloning,
 * Completed). The active side's icon lights up in the primary color.
 */
export function ThemeToggle() {
  const theme = useThemeStore((s) => s.theme);
  const setTheme = useThemeStore((s) => s.setTheme);
  const isDark = theme === "dark";

  return (
    <div className="fixed right-5 top-5 z-50 flex items-center gap-2 rounded-pill border border-border bg-surface/80 px-3 py-1.5 shadow-soft backdrop-blur">
      <Sun
        className={cn(
          "h-4 w-4 transition-colors",
          isDark ? "text-muted" : "text-primary"
        )}
        strokeWidth={2}
      />
      <Switch
        checked={isDark}
        onCheckedChange={(c) => setTheme(c ? "dark" : "light")}
        aria-label="Toggle dark mode"
      />
      <Moon
        className={cn(
          "h-4 w-4 transition-colors",
          isDark ? "text-primary" : "text-muted"
        )}
        strokeWidth={2}
      />
    </div>
  );
}
