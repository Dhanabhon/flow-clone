import { motion } from "framer-motion";
import { HardDrive } from "lucide-react";
import { Card } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { cn, formatBytes } from "@/lib/utils";
import type { DiskInfo } from "@/lib/types";

/**
 * Physical-feeling disk card. Per DESIGN.md: hover lifts slightly, selected
 * shows a blue outline + glow, badges encode health / flags.
 */
export function DiskCard({
  disk,
  selected,
  disabled,
  onSelect,
}: {
  disk: DiskInfo;
  selected: boolean;
  disabled?: boolean;
  onSelect: () => void;
}) {
  const usedPct = disk.used_bytes
    ? Math.min(100, (disk.used_bytes / disk.total_bytes) * 100)
    : null;

  return (
    <motion.button
      type="button"
      whileHover={disabled ? undefined : { y: -2 }}
      onClick={onSelect}
      disabled={disabled}
      aria-pressed={selected}
      className={cn(
        "w-full rounded-card text-left transition-all duration-200 ease-out-soft",
        "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary",
        disabled && "cursor-not-allowed opacity-50"
      )}
    >
      <Card
        className={cn(
          "p-6",
          selected && "border-primary shadow-glow"
        )}
      >
        <div className="flex items-start justify-between">
          <div className="flex items-center gap-4">
            <div className="rounded-button bg-elevated p-3">
              <HardDrive className="h-6 w-6 text-primary" strokeWidth={2} />
            </div>
            <div>
              <h3 className="text-lg font-semibold">{disk.model}</h3>
              <p className="text-sm text-muted">{disk.device_path}</p>
            </div>
          </div>
          <div className="flex flex-col items-end gap-1">
            {disk.health === "healthy" && (
              <Badge tone="success">Healthy</Badge>
            )}
            {disk.read_only && <Badge tone="primary">Read Only</Badge>}
            {disk.encrypted && <Badge tone="purple">Encrypted</Badge>}
            {disk.health === "unknown" && <Badge tone="neutral">Unknown</Badge>}
          </div>
        </div>

        <dl className="mt-6 grid grid-cols-2 gap-x-6 gap-y-2 text-sm">
          <Field label="Capacity" value={formatBytes(disk.total_bytes)} />
          <Field
            label="Filesystem"
            value={disk.filesystem ?? "—"}
          />
          <Field
            label="Connection"
            value={prettyConnection(disk.connection)}
          />
          <Field label="Serial" value={disk.serial ?? "—"} />
        </dl>

        {usedPct !== null && (
          <div className="mt-4">
            <div className="mb-1 flex justify-between text-xs text-muted">
              <span>{formatBytes(disk.used_bytes!)} used</span>
              <span>{usedPct.toFixed(0)}%</span>
            </div>
            <div className="h-1.5 w-full overflow-hidden rounded-pill bg-elevated">
              <div
                className="h-full rounded-pill bg-primary"
                style={{ width: `${usedPct}%` }}
              />
            </div>
          </div>
        )}
      </Card>
    </motion.button>
  );
}

function Field({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex justify-between gap-2">
      <dt className="text-muted">{label}</dt>
      <dd className="font-medium tabular-nums">{value}</dd>
    </div>
  );
}

function prettyConnection(c: DiskInfo["connection"]): string {
  switch (c) {
    case "usb":
      return "USB";
    case "thunderbolt":
      return "Thunderbolt";
    case "internal":
      return "Internal";
    case "firewire":
      return "FireWire";
    case "network":
      return "Network";
    default:
      return "—";
  }
}
