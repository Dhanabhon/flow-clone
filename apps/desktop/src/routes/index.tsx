import { useEffect, useState } from "react";
import { AlertTriangle, ArrowRight, CheckCircle2, ShieldCheck } from "lucide-react";
import { Button } from "@/components/ui/button";
import { onProgress, generateReportStub, startCloneStub } from "@/lib/tauri";
import type { Progress } from "@/lib/types";
import { formatBytes, formatDuration, formatSpeed } from "@/lib/utils";
import { HomeScreen } from "@/features/disk-selection/HomeScreen";
import { useFlowStore } from "@/stores/flow-store";

export function Routes() {
  const phase = useFlowStore((s) => s.phase);

  switch (phase) {
    case "home":
      return <HomeScreen />;
    case "confirmation":
      return <ConfirmationScreen />;
    case "cloning":
      return <CloningScreen />;
    case "completed":
      return <CompletedScreen />;
  }
}

function ConfirmationScreen() {
  const { source, target, verify, setVerify, beginClone, goTo } = useFlowStore();
  const [typed, setTyped] = useState("");
  const [error, setError] = useState<string | null>(null);
  const ready = typed === "ERASE" && source && target;

  async function start() {
    if (!source || !target) return;
    setError(null);
    try {
      const jobId = await startCloneStub(source.device_path, target.device_path, verify);
      beginClone(jobId, "clone");
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }

  if (!source || !target) {
    return <MissingSelection />;
  }

  return (
    <main className="mx-auto flex min-h-screen max-w-content items-center justify-center p-8">
      <section className="w-full max-w-xl rounded-card border border-border bg-surface p-8 shadow-soft">
        <div className="mx-auto mb-5 flex h-14 w-14 items-center justify-center rounded-card bg-primary/15 text-primary">
          <ShieldCheck className="h-8 w-8" />
        </div>
        <h1 className="text-center text-3xl font-semibold">Ready to Clone</h1>
        <div className="mt-8 grid grid-cols-[1fr_auto_1fr] items-center gap-4 rounded-input border border-border bg-background p-4">
          <DiskSummary label="Source" name={source.model} size={source.total_bytes} />
          <ArrowRight className="h-5 w-5 text-primary" />
          <DiskSummary label="Target" name={target.model} size={target.total_bytes} />
        </div>
        <div className="mt-5 rounded-input border border-warning/30 bg-warning/10 p-4 text-warning">
          <div className="flex items-start gap-3">
            <AlertTriangle className="mt-0.5 h-5 w-5 shrink-0" />
            <p className="text-sm">
              All data on the target disk will be erased and cannot be recovered.
              Phase 1 is mocked and will not write to any disk.
            </p>
          </div>
        </div>
        <label className="mt-6 block text-sm font-medium">
          Type ERASE to continue.
          <input
            className="mt-2 h-11 w-full rounded-input border border-border bg-background px-3 text-base outline-none transition focus:border-primary"
            value={typed}
            onChange={(event) => setTyped(event.target.value)}
            placeholder="ERASE"
          />
        </label>
        <label className="mt-4 flex items-center gap-3 text-sm text-muted">
          <input
            type="checkbox"
            checked={verify}
            onChange={(event) => setVerify(event.target.checked)}
          />
          Verify after cloning
        </label>
        {error && <p className="mt-4 text-sm text-danger">{error}</p>}
        <div className="mt-7 grid grid-cols-2 gap-3">
          <Button variant="secondary" onClick={() => goTo("home")}>
            Cancel
          </Button>
          <Button disabled={!ready} onClick={start}>
            Clone
          </Button>
        </div>
      </section>
    </main>
  );
}

function CloningScreen() {
  const { source, target, progress, setProgress, goTo } = useFlowStore();

  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | undefined;
    onProgress((next) => {
      if (!active) return;
      setProgress(next);
      if (next.phase === "completed") goTo("completed");
    }).then((fn) => {
      if (active) {
        unlisten = fn;
      } else {
        fn();
      }
    });
    return () => {
      active = false;
      unlisten?.();
    };
  }, [goTo, setProgress]);

  const shown = progress ?? emptyProgress(source?.total_bytes ?? 0);
  const pct = Math.round(shown.fraction * 100);

  return (
    <main className="mx-auto flex min-h-screen max-w-content items-center justify-center p-8">
      <section className="w-full max-w-3xl rounded-card border border-border bg-surface p-8 text-center shadow-soft">
        <h1 className="text-3xl font-semibold">Cloning in progress...</h1>
        <p className="mt-2 text-sm text-muted">Please do not disconnect the drives.</p>
        <div
          className="mx-auto mt-8 grid h-40 w-40 place-items-center rounded-full"
          style={{
            background: `conic-gradient(var(--primary) ${pct}%, var(--elevated) ${pct}% 100%)`,
          }}
        >
          <div className="grid h-32 w-32 place-items-center rounded-full bg-background text-4xl font-semibold">
            {pct}%
          </div>
        </div>
        <div className="mt-8 grid grid-cols-[1fr_auto_1fr] items-center gap-4 rounded-input border border-border bg-background p-4">
          <DiskSummary label="Source" name={source?.model ?? "Source"} size={source?.total_bytes ?? 0} />
          <ArrowRight className="h-5 w-5 text-primary" />
          <DiskSummary label="Target" name={target?.model ?? "Target"} size={target?.total_bytes ?? 0} />
        </div>
        <dl className="mt-6 grid grid-cols-2 gap-4 text-sm md:grid-cols-4">
          <Metric label="Read Speed" value={formatSpeed(shown.read_speed)} />
          <Metric label="Write Speed" value={formatSpeed(shown.write_speed)} />
          <Metric label="Elapsed" value={formatDuration(shown.elapsed_secs)} />
          <Metric label="Remaining" value={shown.eta_secs == null ? "..." : formatDuration(shown.eta_secs)} />
        </dl>
        <p className="mt-6 text-sm text-muted">{shown.current_operation}</p>
      </section>
    </main>
  );
}

function CompletedScreen() {
  const { mode, source, target, imagePath, progress, report, setReport, reset } =
    useFlowStore();

  async function exportReport() {
    if (!source) return;
    const text = await generateReportStub(
      source.device_path,
      target?.device_path,
      imagePath ?? undefined
    );
    setReport(text);
  }

  return (
    <main className="mx-auto flex min-h-screen max-w-content items-center justify-center p-8">
      <section className="w-full max-w-2xl rounded-card border border-border bg-surface p-8 text-center shadow-soft">
        <div className="mx-auto grid h-20 w-20 place-items-center rounded-full bg-success/20 text-success">
          <CheckCircle2 className="h-12 w-12" />
        </div>
        <h1 className="mt-5 text-3xl font-semibold">
          {mode === "image" ? "Image Migration Ready" : "Clone Completed"}
        </h1>
        <p className="mt-2 text-sm text-muted">
          {mode === "image"
            ? "Your stub migration image workflow completed."
            : "Your stub clone completed and verification passed."}
        </p>

        <div className="mt-8 rounded-input border border-border bg-background p-4 text-left">
          <div className="grid gap-4 md:grid-cols-2">
            <DiskSummary label="Source" name={source?.model ?? "Source"} size={source?.total_bytes ?? 0} />
            {mode === "clone" ? (
              <DiskSummary label="Target" name={target?.model ?? "Target"} size={target?.total_bytes ?? 0} />
            ) : (
              <DiskSummary label="Image" name={imagePath ?? "migration.flowimg"} size={source?.total_bytes ?? 0} />
            )}
          </div>
          <dl className="mt-5 grid grid-cols-3 gap-3 border-t border-border pt-5 text-sm">
            <Metric label="Average Speed" value={formatSpeed(progress?.write_speed ?? 0)} />
            <Metric label="Total Time" value={formatDuration(progress?.elapsed_secs ?? 0)} />
            <Metric label="Verification" value={mode === "clone" ? "Passed" : "Stubbed"} />
          </dl>
        </div>

        {report && (
          <pre className="mt-5 max-h-40 overflow-auto rounded-input border border-border bg-background p-4 text-left text-xs text-muted">
            {report}
          </pre>
        )}

        <div className="mt-7 grid grid-cols-2 gap-3">
          <Button variant="secondary" onClick={exportReport}>
            Export Report
          </Button>
          <Button onClick={reset}>Done</Button>
        </div>
      </section>
    </main>
  );
}

function DiskSummary({
  label,
  name,
  size,
}: {
  label: string;
  name: string;
  size: number;
}) {
  return (
    <div>
      <p className="text-xs uppercase tracking-wide text-muted">{label}</p>
      <p className="mt-1 font-semibold">{name}</p>
      <p className="text-sm text-muted">{formatBytes(size)}</p>
    </div>
  );
}

function Metric({ label, value }: { label: string; value: string }) {
  return (
    <div>
      <dt className="text-xs uppercase tracking-wide text-muted">{label}</dt>
      <dd className="mt-1 font-semibold tabular-nums">{value}</dd>
    </div>
  );
}

function MissingSelection() {
  const goTo = useFlowStore((s) => s.goTo);
  return (
    <main className="mx-auto flex min-h-screen max-w-content items-center justify-center p-8">
      <section className="rounded-card border border-border bg-surface p-8 text-center shadow-soft">
        <h1 className="text-2xl font-semibold">Choose source and target disks first.</h1>
        <Button className="mt-5" onClick={() => goTo("home")}>
          Back
        </Button>
      </section>
    </main>
  );
}

function emptyProgress(bytesTotal: number): Progress {
  return {
    job_id: "",
    phase: "preparing",
    fraction: 0,
    bytes_done: 0,
    bytes_total: bytesTotal,
    read_speed: 0,
    write_speed: 0,
    elapsed_secs: 0,
    eta_secs: null,
    current_operation: "Preparing clone",
  };
}
