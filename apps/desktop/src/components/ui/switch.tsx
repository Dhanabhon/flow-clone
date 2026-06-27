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
  // Track w-11 (44px) minus knob w-5 (20px) minus 2px left inset = 20px travel.
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      onClick={() => onCheckedChange(!checked)}
      className={cn(
        "relative inline-flex h-6 w-11 shrink-0 rounded-pill transition-colors duration-200 ease-out-soft",
        "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary focus-visible:ring-offset-2 focus-visible:ring-offset-background",
        checked ? "bg-primary" : "border border-border bg-elevated",
        className
      )}
      {...rest}
    >
      <motion.span
        animate={{ x: checked ? 20 : 0 }}
        transition={{ duration: 0.2, ease: "easeOut" }}
        className="pointer-events-none absolute left-0.5 top-0.5 h-5 w-5 rounded-full bg-white shadow-soft"
      />
    </button>
  );
}
