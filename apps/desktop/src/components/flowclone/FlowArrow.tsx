import { motion } from "framer-motion";
import { ArrowRight } from "lucide-react";
import { cn } from "@/lib/utils";

/**
 * The animated flow arrow shown between source and target cards. Stays static
 * until both disks are selected (DESIGN.md), then a subtle particle drifts
 * across to suggest movement.
 */
export function FlowArrow({ active }: { active: boolean }) {
  return (
    <div className="flex items-center justify-center px-2">
      <ArrowRight
        className={cn(
          "h-6 w-6 transition-colors duration-200",
          active ? "text-primary" : "text-muted"
        )}
        strokeWidth={2}
      />
      {active && (
        <motion.span
          className="ml-1 h-1.5 w-1.5 rounded-full bg-primary"
          animate={{ opacity: [0.2, 1, 0.2] }}
          transition={{ duration: 1.4, repeat: Infinity, ease: "easeOut" }}
        />
      )}
    </div>
  );
}
