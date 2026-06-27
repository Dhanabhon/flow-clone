import { motion } from "framer-motion";
import { cn } from "@/lib/utils";

export interface SwitchProps {
  checked: boolean;
  onCheckedChange: (checked: boolean) => void;
  className?: string;
  "aria-label"?: string;
}

/**
 * Toggle switch. Pill track (radius 999px per DESIGN.md) with an animated
 * knob. Motion follows the design spec: ~200ms easeOut, no bounce.
 */
export function Switch({
  checked,
  onCheckedChange,
  className,
  ...rest
}: SwitchProps) {
  // Track 44px wide minus 20px knob and 2px inset on each side leaves 20px travel.
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      onClick={() => onCheckedChange(!checked)}
      className={cn(
        "relative inline-flex h-6 w-11 shrink-0 rounded-pill transition-colors duration-200 ease-out-soft",
        "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary focus-visible:ring-offset-2 focus-visible:ring-offset-background",
        checked
          ? "bg-primary"
          : "bg-[#e2e8f0] ring-1 ring-inset ring-[#cbd5e1] dark:bg-elevated dark:ring-border",
        className
      )}
      {...rest}
    >
      <motion.span
        animate={{ x: checked ? 20 : 0 }}
        transition={{ duration: 0.2, ease: "easeOut" }}
        className="pointer-events-none absolute left-[2px] top-[2px] h-5 w-5 rounded-full bg-white shadow-[0_1px_5px_rgba(15,23,42,0.22)] ring-1 ring-black/5 dark:ring-white/10"
      />
    </button>
  );
}
