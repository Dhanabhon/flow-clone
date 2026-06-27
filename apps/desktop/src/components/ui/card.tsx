import type { HTMLAttributes } from "react";
import { cn } from "@/lib/utils";

/** Surface card with the soft shadow and 20px radius from DESIGN.md. */
export function Card({ className, ...props }: HTMLAttributes<HTMLDivElement>) {
  return (
    <div
      className={cn(
        "rounded-card border border-border bg-surface shadow-soft transition-all duration-200 ease-out-soft",
        className
      )}
      {...props}
    />
  );
}
