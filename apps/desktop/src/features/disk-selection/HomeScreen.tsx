import { useMemo } from "react";
import { motion } from "framer-motion";
import { Button } from "@/components/ui/button";
import { DiskCard } from "@/components/flowclone/DiskCard";
import { FlowArrow } from "@/components/flowclone/FlowArrow";
import { useDisks } from "@/hooks/use-disks";
import { useFlowStore } from "@/stores/flow-store";
import { formatBytes } from "@/lib/utils";

/**
 * Screen 1 — Home. Two equal disk cards (source → target), a flow arrow, a
 * warning banner after target selection, and the Start Clone button that stays
 * disabled until selection + size validation pass.
 */
export function HomeScreen() {
  const { data: disks, isLoading } = useDisks();
  const { source, target, setSource, setTarget, goTo } = useFlowStore();

  const canStart = useMemo(() => {
    if (!source || !target) return false;
    if (source.device_path === target.device_path) return false;
    return target.total_bytes >= source.total_bytes;
  }, [source, target]);

  const sameDisk =
    source && target && source.device_path === target.device_path;
  const tooSmall =
    source && target && target.total_bytes < source.total_bytes;

  return (
    <main className="mx-auto min-h-screen max-w-content px-8 py-12">
      <header className="mb-12 text-center">
        <h1 className="text-4xl font-semibold tracking-tight">FlowClone</h1>
        <p className="mt-2 text-lg text-muted">
          Safe SSD Cloning — clone your disk with confidence.
        </p>
      </header>

      {isLoading && (
        <p className="text-center text-muted">Scanning for disks…</p>
      )}

      {!isLoading && disks?.length === 0 && (
        <EmptyState />
      )}

      {disks && disks.length > 0 && (
        <div className="grid grid-cols-[1fr_auto_1fr] items-center gap-6">
          <Slot
            label="Source"
            disks={disks}
            selected={source}
            exclude={target?.device_path}
            onSelect={setSource}
          />

          <FlowArrow active={!!source && !!target} />

          <Slot
            label="Target"
            disks={disks}
            selected={target}
            exclude={source?.device_path}
            onSelect={setTarget}
          />
        </div>
      )}

      {target && !tooSmall && !sameDisk && (
        <motion.div
          initial={{ opacity: 0, y: 8 }}
          animate={{ opacity: 1, y: 0 }}
          className="mt-8 rounded-input border border-warning/30 bg-warning/10 p-4 text-center text-sm text-warning"
        >
          Target disk <strong>{target.device_path}</strong> will be completely
          erased.
        </motion.div>
      )}

      {sameDisk && (
        <p className="mt-8 text-center text-sm text-danger">
          Cannot clone to the same device.
        </p>
      )}
      {tooSmall && (
        <p className="mt-8 text-center text-sm text-danger">
          Target disk is smaller than the source disk.
        </p>
      )}

      <div className="mt-8 flex justify-center">
        <Button
          size="lg"
          className="w-full max-w-md"
          disabled={!canStart}
          onClick={() => goTo("confirmation")}
        >
          Start Clone
        </Button>
      </div>

      {source && target && (
        <p className="mt-3 text-center text-xs text-muted">
          {formatBytes(source.total_bytes)} → {formatBytes(target.total_bytes)}
        </p>
      )}
    </main>
  );
}

/** A source/target slot: either the selected card or a pickable list. */
function Slot({
  label,
  disks,
  selected,
  exclude,
  onSelect,
}: {
  label: string;
  disks: import("@/lib/types").DiskInfo[];
  selected: import("@/lib/types").DiskInfo | null;
  exclude?: string;
  onSelect: (d: import("@/lib/types").DiskInfo | null) => void;
}) {
  return (
    <section>
      <h2 className="mb-3 text-sm font-medium uppercase tracking-wide text-muted">
        {label}
      </h2>
      {selected ? (
        <DiskCard
          disk={selected}
          selected
          onSelect={() => onSelect(null)}
        />
      ) : (
        <div className="flex flex-col gap-3">
          {disks
            .filter((d) => d.device_path !== exclude)
            .map((d) => (
              <DiskCard
                key={d.device_path}
                disk={d}
                selected={false}
                disabled={d.is_boot || d.read_only}
                onSelect={() => onSelect(d)}
              />
            ))}
        </div>
      )}
    </section>
  );
}

function EmptyState() {
  return (
    <div className="rounded-card border border-dashed border-border bg-surface p-16 text-center">
      <p className="text-lg font-medium">No drives connected.</p>
      <p className="mt-1 text-muted">Connect your SSDs to begin.</p>
    </div>
  );
}
