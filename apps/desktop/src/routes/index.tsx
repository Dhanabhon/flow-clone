import { useEffect, useMemo, useState } from "react";
import {
  AlertTriangle,
  ArrowRight,
  CheckCircle2,
  Clipboard,
  ExternalLink,
  RefreshCw,
  ShieldCheck,
} from "lucide-react";
import { ask } from "@tauri-apps/plugin-dialog";
import { Button } from "@/components/ui/button";
import { useI18n } from "@/lib/i18n";
import {
  cancelClone,
  copyText,
  createImageStub,
  generateReportStub,
  isTauriRuntime,
  onProgress,
  openFullDiskAccessSettings,
  restoreImageStub,
  saveReportFile,
  startCloneStub,
  validateImageStub,
} from "@/lib/tauri";
import type { ImageValidation, Progress } from "@/lib/types";
import { fileNameFromPath, formatBytes, formatDuration, formatSpeed } from "@/lib/utils";
import { HomeScreen } from "@/features/disk-selection/HomeScreen";
import { useFlowStore, type WorkflowMode } from "@/stores/flow-store";

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
  const { t } = useI18n();
  const { mode, source, target, imagePath, verify, setVerify, beginClone, goTo } =
    useFlowStore();
  const [typed, setTyped] = useState("");
  const [error, setError] = useState<string | null>(null);
  const isRestoreMode = mode === "restore";
  const ready =
    typed === "ERASE" && target && (isRestoreMode ? imagePath : source);

  async function start() {
    if (!target) return;
    setError(null);
    try {
      if (isRestoreMode) {
        if (!imagePath) return;
        const jobId = await restoreImageStub(imagePath, target.device_path);
        beginClone(jobId, "restore");
        return;
      }

      if (!source) return;
      const jobId = await startCloneStub(source.device_path, target.device_path, verify);
      beginClone(jobId, "clone");
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }

  if (!target || (isRestoreMode ? !imagePath : !source)) {
    return <MissingSelection />;
  }

  return (
    <main className="mx-auto flex min-h-screen max-w-content items-center justify-center p-8">
      <section className="w-full max-w-xl rounded-card border border-border bg-surface p-8 shadow-soft">
        <div className="mx-auto mb-5 flex h-14 w-14 items-center justify-center rounded-card bg-primary/15 text-primary">
          <ShieldCheck className="h-8 w-8" />
        </div>
        <h1 className="text-center text-3xl font-semibold">
          {isRestoreMode ? t("readyToRestore") : t("readyToClone")}
        </h1>
        <div className="mt-8 grid grid-cols-[1fr_auto_1fr] items-center gap-4 rounded-input border border-border bg-background p-4">
          <DiskSummary
            label={isRestoreMode ? t("image") : t("source")}
            name={
              isRestoreMode
                ? fileNameFromPath(imagePath ?? "", "migration.flowimg")
                : source?.model ?? t("source")
            }
            size={isRestoreMode ? undefined : source?.total_bytes}
          />
          <ArrowRight className="h-5 w-5 text-primary" />
          <DiskSummary label={t("target")} name={target.model} size={target.total_bytes} />
        </div>
        <div className="mt-5 rounded-input border border-warning/30 bg-warning/10 p-4 text-warning">
          <div className="flex items-start gap-3">
            <AlertTriangle className="mt-0.5 h-5 w-5 shrink-0" />
            <p className="text-sm">
              {isRestoreMode ? t("restoreEraseWarning") : t("eraseWarning")}
            </p>
          </div>
        </div>
        <label className="mt-6 block text-sm font-medium">
          {t("typeErase")}
          <input
            className="mt-2 h-11 w-full rounded-input border border-border bg-background px-3 text-base outline-none transition focus:border-primary"
            value={typed}
            onChange={(event) => setTyped(event.target.value)}
            placeholder="ERASE"
          />
        </label>
        {!isRestoreMode && (
          <label className="mt-4 flex items-center gap-3 text-sm text-muted">
            <input
              type="checkbox"
              checked={verify}
              onChange={(event) => setVerify(event.target.checked)}
            />
            {t("verifyAfterCloning")}
          </label>
        )}
        {error && <p className="mt-4 text-sm text-danger">{error}</p>}
        <div className="mt-7 grid grid-cols-2 gap-3">
          <Button variant="secondary" onClick={() => goTo("home")}>
            {t("cancel")}
          </Button>
          <Button disabled={!ready} onClick={start}>
            {isRestoreMode ? t("restoreImageAction") : t("clone")}
          </Button>
        </div>
      </section>
    </main>
  );
}

function CloningScreen() {
  const { t } = useI18n();
  const { mode, source, target, imagePath, progress, setProgress, beginClone, goTo, reset } =
    useFlowStore();
  const [isCancelling, setIsCancelling] = useState(false);
  const [isRetrying, setIsRetrying] = useState(false);
  const [copyState, setCopyState] = useState<"idle" | "copied">("idle");
  const [recoveryError, setRecoveryError] = useState<string | null>(null);
  const isImageMode = mode === "image";
  const isRestoreMode = mode === "restore";

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

  const shown =
    progress ??
    emptyProgress(
      (isRestoreMode ? target?.total_bytes : source?.total_bytes) ?? 0,
      mode
    );
  const pct = Math.round(shown.fraction * 100);
  const imageName = fileNameFromPath(imagePath, "migration.flowimg");
  const canCancel =
    (isImageMode || isRestoreMode) &&
    shown.phase !== "completed" &&
    shown.phase !== "failed";
  const failed = shown.phase === "failed";
  const insufficientSpace = failed
    ? parseInsufficientSpace(shown.current_operation)
    : null;
  const permissionDenied =
    isImageMode &&
    failed &&
    // EACCES ("permission denied") is the raw 640 device-mode gate; EPERM
    // ("operation not permitted" / os error 1) is the macOS Full Disk Access
    // gate that applies even to root. Both route to the same recovery card.
    /denied access|permission denied|not permitted|os error 1/i.test(
      shown.current_operation
    );
  // The source dropped off the bus and didn't recover (disconnect / power loss).
  const interrupted =
    isImageMode &&
    failed &&
    !insufficientSpace &&
    !permissionDenied &&
    /dropping off the bus|did not come back|Device not configured|ended early|os error 6|os error 5/i.test(
      shown.current_operation
    );
  const cliCommand = useMemo(
    () =>
      source && imagePath
        ? `sudo ./target/debug/flowclone create-image --source ${shellQuote(source.device_path)} --output ${shellQuote(imagePath)}`
        : "",
    [imagePath, source]
  );
  const title = failed
    ? insufficientSpace
      ? t("notEnoughSpaceTitle")
      : permissionDenied
      ? t("diskAccessRequiredTitle")
      : interrupted
      ? t("migrationInterruptedTitle")
      : isImageMode
        ? t("imageFailedTitle")
        : isRestoreMode
          ? t("restoreFailedTitle")
        : t("cloneFailedTitle")
    : isImageMode
      ? t("creatingImageTitle")
      : isRestoreMode
        ? t("restoringImageTitle")
      : t("cloningTitle");
  const help = failed
    ? insufficientSpace
      ? t("notEnoughSpaceBody")
      : permissionDenied
      ? t("diskAccessRequiredBody")
      : interrupted
      ? t("migrationInterruptedBody")
      : t("workflowFailedHelp")
    : isImageMode
      ? t("creatingImageHelp")
      : isRestoreMode
        ? t("restoringImageHelp")
      : t("cloningHelp");

  async function cancelImage() {
    const confirmed = isTauriRuntime()
      ? await ask(t("cancelImageConfirmBody"), {
          title: t("cancelImageConfirmTitle"),
          kind: "warning",
          okLabel: t("cancelImageConfirmOk"),
          cancelLabel: t("cancelImageConfirmKeep"),
        })
      : window.confirm(t("cancelImageConfirmBody"));
    if (!confirmed) return;
    setIsCancelling(true);
    await cancelClone();
    reset();
  }

  async function retryImage() {
    if (!source || !imagePath) return;
    setIsRetrying(true);
    setRecoveryError(null);
    try {
      const jobId = await createImageStub(source.device_path, imagePath);
      beginClone(jobId, "image");
    } catch (err) {
      setProgress(failedProgress(source.total_bytes, err));
    } finally {
      setIsRetrying(false);
    }
  }

  async function openSettings() {
    setRecoveryError(null);
    try {
      await openFullDiskAccessSettings();
    } catch (err) {
      setRecoveryError(err instanceof Error ? err.message : String(err));
    }
  }

  async function copyCliCommand() {
    setRecoveryError(null);
    try {
      await copyText(cliCommand);
      setCopyState("copied");
    } catch (err) {
      setRecoveryError(err instanceof Error ? err.message : String(err));
    }
  }

  return (
    <main className="mx-auto flex min-h-screen max-w-content items-center justify-center p-8">
      <section className="w-full max-w-3xl rounded-card border border-border bg-surface p-8 text-center shadow-soft">
        <h1 className="text-3xl font-semibold">{title}</h1>
        <p className="mt-2 text-sm text-muted">
          {help}
        </p>
        {failed ? (
          <div className="mx-auto mt-8 grid h-20 w-20 place-items-center rounded-full bg-warning/15 text-warning">
            <AlertTriangle className="h-10 w-10" />
          </div>
        ) : (
          <div
            className="mx-auto mt-8 grid h-40 w-40 place-items-center rounded-full"
            style={{
              background: `conic-gradient(rgb(var(--primary)) ${pct}%, rgb(var(--elevated)) ${pct}% 100%)`,
            }}
          >
            <div className="grid h-32 w-32 place-items-center rounded-full bg-background text-4xl font-semibold">
              {pct}%
            </div>
          </div>
        )}
        <div className="mt-8 grid grid-cols-[1fr_auto_1fr] items-center gap-4 rounded-input border border-border bg-background p-4">
          <DiskSummary
            label={isRestoreMode ? t("image") : t("source")}
            name={isRestoreMode ? imageName : source?.model ?? t("source")}
            size={isRestoreMode ? undefined : source?.total_bytes ?? 0}
          />
          <ArrowRight className="h-5 w-5 text-primary" />
          {isImageMode ? (
            <DiskSummary label={t("image")} name={imageName} size={source?.total_bytes ?? 0} />
          ) : (
            <DiskSummary label={t("target")} name={target?.model ?? t("target")} size={target?.total_bytes ?? 0} />
          )}
        </div>
        <dl className="mt-6 grid grid-cols-2 gap-4 text-sm md:grid-cols-4">
          <Metric label={t("readSpeed")} value={formatSpeed(shown.read_speed)} />
          <Metric label={t("writeSpeed")} value={formatSpeed(shown.write_speed)} />
          <Metric label={t("elapsed")} value={formatDuration(shown.elapsed_secs)} />
          <Metric label={t("remaining")} value={shown.eta_secs == null ? "..." : formatDuration(shown.eta_secs)} />
        </dl>
        {!insufficientSpace && (
          <p className={failed ? "mt-6 text-sm text-danger" : "mt-6 text-sm text-muted"}>
            {progressOperationText(shown.current_operation, t)}
          </p>
        )}

        {insufficientSpace && (
          <InsufficientSpaceCard details={insufficientSpace} />
        )}

        {permissionDenied && (
          <div className="mx-auto mt-6 max-w-xl rounded-input border border-warning/30 bg-warning/10 p-4 text-left">
            <p className="text-sm text-warning">{t("diskAccessDevHelp")}</p>
            {cliCommand && (
              <code className="mt-3 block overflow-hidden text-ellipsis whitespace-nowrap rounded-input border border-border bg-background px-3 py-2 text-xs text-muted">
                {cliCommand}
              </code>
            )}
            <div className="mt-4 grid gap-3 sm:grid-cols-2">
              <Button className="gap-2" size="sm" variant="secondary" onClick={openSettings}>
                <ExternalLink className="h-4 w-4" />
                {t("openFullDiskAccess")}
              </Button>
              <Button className="gap-2" size="sm" variant="secondary" disabled={isRetrying} onClick={retryImage}>
                <RefreshCw className={isRetrying ? "h-4 w-4 animate-spin" : "h-4 w-4"} />
                {t("checkAgain")}
              </Button>
              <Button className="gap-2" size="sm" variant="secondary" disabled={!cliCommand} onClick={copyCliCommand}>
                <Clipboard className="h-4 w-4" />
                {copyState === "copied" ? t("cliCommandCopied") : t("copyCliCommand")}
              </Button>
            </div>
            {recoveryError && (
              <p className="mt-3 text-sm text-danger">{recoveryError}</p>
            )}
          </div>
        )}

        {interrupted && (
          <div className="mx-auto mt-6 max-w-xl rounded-input border border-warning/30 bg-warning/10 p-4 text-left">
            <p className="text-sm text-warning">{t("migrationInterruptedHelp")}</p>
            <Button
              className="mt-4 w-full gap-2"
              size="sm"
              variant="secondary"
              disabled={isRetrying}
              onClick={retryImage}
            >
              <RefreshCw className={isRetrying ? "h-4 w-4 animate-spin" : "h-4 w-4"} />
              {t("tryAgain")}
            </Button>
            {recoveryError && (
              <p className="mt-3 text-sm text-danger">{recoveryError}</p>
            )}
          </div>
        )}

        {failed && (
          <Button className="mt-6 w-full max-w-xs" variant="secondary" onClick={reset}>
            {t("back")}
          </Button>
        )}
        {canCancel && (
          <Button
            className="mt-6 w-full max-w-xs"
            variant="secondary"
            disabled={isCancelling}
            onClick={cancelImage}
          >
            {t("cancel")}
          </Button>
        )}
      </section>
    </main>
  );
}

function CompletedScreen() {
  const { t } = useI18n();
  const { mode, source, target, imagePath, progress, report, setReport, reset } =
    useFlowStore();
  const [exportMessage, setExportMessage] = useState<string | null>(null);
  const [exportError, setExportError] = useState<string | null>(null);
  const [imageValidation, setImageValidation] =
    useState<ImageValidation | null>(null);
  const [imageValidationError, setImageValidationError] = useState<
    string | null
  >(null);
  const imageName = fileNameFromPath(imagePath, "migration.flowimg");
  const isImageMode = mode === "image";
  const isRestoreMode = mode === "restore";

  useEffect(() => {
    if (mode !== "image" || !imagePath) return;
    let active = true;
    setImageValidation(null);
    setImageValidationError(null);
    validateImageStub(imagePath)
      .then((validation) => {
        if (active) setImageValidation(validation);
      })
      .catch((err) => {
        if (active) {
          setImageValidationError(err instanceof Error ? err.message : String(err));
        }
      });
    return () => {
      active = false;
    };
  }, [imagePath, mode]);

  async function exportReport() {
    setExportMessage(null);
    setExportError(null);
    try {
      let text: string;
      if (isRestoreMode) {
        if (!imagePath || !target) return;
        text = restoreReportText(imagePath, target.device_path);
      } else {
        if (!source) return;
        text = await generateReportStub(
          source.device_path,
          target?.device_path,
          imagePath ?? undefined
        );
      }
      const path = await saveReportFile(text);
      if (!path) return;
      setReport(text);
      setExportMessage(t("reportSaved", { path }));
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      setExportError(t("reportExportFailed", { message }));
    }
  }

  return (
    <main className="mx-auto flex min-h-screen max-w-content items-center justify-center p-8">
      <section className="w-full max-w-2xl rounded-card border border-border bg-surface p-8 text-center shadow-soft">
        <div className="mx-auto grid h-20 w-20 place-items-center rounded-full bg-success/20 text-success">
          <CheckCircle2 className="h-12 w-12" />
        </div>
        <h1 className="mt-5 text-3xl font-semibold">
          {isImageMode
            ? t("imageMigrationReady")
            : isRestoreMode
              ? t("restoreCompleted")
              : t("cloneCompleted")}
        </h1>
        <p className="mt-2 text-sm text-muted">
          {isImageMode
            ? t("imageCompletedBody")
            : isRestoreMode
              ? t("restoreCompletedBody")
              : t("cloneCompletedBody")}
        </p>

        <div className="mt-8 rounded-input border border-border bg-background p-4 text-left">
          <div className="grid gap-4 md:grid-cols-2">
            <DiskSummary
              label={isRestoreMode ? t("image") : t("source")}
              name={isRestoreMode ? imageName : source?.model ?? t("source")}
              size={isRestoreMode ? undefined : source?.total_bytes ?? 0}
            />
            {!isImageMode ? (
              <DiskSummary label={t("target")} name={target?.model ?? t("target")} size={target?.total_bytes ?? 0} />
            ) : (
              <DiskSummary label={t("image")} name={imageName} size={source?.total_bytes ?? 0} />
            )}
          </div>
          <dl className="mt-5 grid grid-cols-3 gap-3 border-t border-border pt-5 text-sm">
            <Metric label={t("averageSpeed")} value={formatSpeed(progress?.write_speed ?? 0)} />
            <Metric label={t("totalTime")} value={formatDuration(progress?.elapsed_secs ?? 0)} />
            <Metric
              label={mode === "clone" ? t("verification") : t("restore")}
              value={mode === "clone" ? t("passed") : t("ready")}
            />
          </dl>
        </div>

        {isImageMode && (
          <p
            className={
              imageValidationError
                ? "mt-4 text-sm text-danger"
                : "mt-4 text-sm text-success"
            }
          >
            {imageValidationError
              ? t("imageVerifyFailed", { message: imageValidationError })
              : imageValidation
                ? t("imageVerified", {
                    model: imageValidation.source.model,
                    size: formatBytes(
                      imageValidation.payload_bytes ||
                        imageValidation.source.total_bytes
                    ),
                  })
                : t("imageVerifying")}
          </p>
        )}

        {report && (
          <pre className="mt-5 max-h-40 overflow-auto rounded-input border border-border bg-background p-4 text-left text-xs text-muted">
            {report}
          </pre>
        )}
        {exportMessage && (
          <p className="mt-4 text-sm text-success">{exportMessage}</p>
        )}
        {exportError && (
          <p className="mt-4 text-sm text-danger">{exportError}</p>
        )}

        <div className="mt-7 grid grid-cols-2 gap-3">
          <Button variant="secondary" onClick={exportReport}>
            {t("exportReport")}
          </Button>
          <Button onClick={reset}>{t("done")}</Button>
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
  size?: number;
}) {
  return (
    <div className="min-w-0">
      <p className="text-xs uppercase tracking-wide text-muted">{label}</p>
      <p className="mt-1 truncate font-semibold" title={name}>{name}</p>
      {size != null && <p className="text-sm text-muted">{formatBytes(size)}</p>}
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

type InsufficientSpaceDetails = {
  location: string;
  required: string;
  reserve: string;
  available: string;
};

function InsufficientSpaceCard({
  details,
}: {
  details: InsufficientSpaceDetails;
}) {
  const { t } = useI18n();

  return (
    <div className="mx-auto mt-6 max-w-xl rounded-input border border-warning/30 bg-warning/10 p-4 text-left">
      <p className="text-sm text-warning">{t("notEnoughSpaceSuggestion")}</p>
      <p
        className="mt-3 truncate rounded-input border border-border bg-background px-3 py-2 text-sm font-medium"
        title={details.location}
      >
        {details.location}
      </p>
      <dl className="mt-4 grid gap-3 sm:grid-cols-3">
        <StorageMetric label={t("imageSize")} value={details.required} />
        <StorageMetric label={t("safetyReserve")} value={details.reserve} />
        <StorageMetric label={t("availableSpace")} value={details.available} />
      </dl>
    </div>
  );
}

function StorageMetric({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-input border border-border bg-background p-3">
      <dt className="text-xs uppercase tracking-wide text-muted">{label}</dt>
      <dd className="mt-1 font-semibold tabular-nums">{value}</dd>
    </div>
  );
}

function MissingSelection() {
  const { t } = useI18n();
  const goTo = useFlowStore((s) => s.goTo);
  return (
    <main className="mx-auto flex min-h-screen max-w-content items-center justify-center p-8">
      <section className="rounded-card border border-border bg-surface p-8 text-center shadow-soft">
        <h1 className="text-2xl font-semibold">{t("chooseDisksFirst")}</h1>
        <Button className="mt-5" onClick={() => goTo("home")}>
          {t("back")}
        </Button>
      </section>
    </main>
  );
}

function progressOperationText(
  text: string,
  t: ReturnType<typeof useI18n>["t"]
): string {
  if (text === "Preparing clone") return t("preparingClone");
  if (text === "Preparing image") return t("preparingImage");
  if (text === "Preparing restore") return t("preparingRestore");
  if (text === "Waiting for administrator authorization")
    return t("waitingForAuthorization");
  if (text === "Interrupted; reconnecting to disk")
    return t("reconnectingToDisk");
  if (text === "Restoring to disk") return t("restoringToDisk");
  if (text === "Completed") return t("cloneCompleted");
  if (text === "Restore workflow ready") return t("restoreWorkflowReady");
  if (text === "Mock migration image created") return t("mockImageCreated");
  const imageReady = text.match(/^Image workflow ready at (.+)$/);
  if (imageReady) {
    return t("imageReadyAt", {
      path: fileNameFromPath(imageReady[1], imageReady[1]),
    });
  }

  const match = text.match(/^Copying mock block (\d+) to (.+)$/);
  if (match) {
    return t("copyingMockBlock", { step: match[1], targetPath: match[2] });
  }
  const imageMatch = text.match(/^Creating image block (\d+) to (.+)$/);
  if (imageMatch) {
    return t("creatingImageBlock", {
      step: imageMatch[1],
      imagePath: fileNameFromPath(imageMatch[2], imageMatch[2]),
    });
  }

  return text;
}

function emptyProgress(bytesTotal: number, mode: WorkflowMode): Progress {
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
    current_operation:
      mode === "image"
        ? "Preparing image"
        : mode === "restore"
          ? "Preparing restore"
          : "Preparing clone",
  };
}

function failedProgress(bytesTotal: number, err: unknown): Progress {
  return {
    job_id: `failed-${Date.now()}`,
    phase: "failed",
    fraction: 0,
    bytes_done: 0,
    bytes_total: bytesTotal,
    read_speed: 0,
    write_speed: 0,
    elapsed_secs: 0,
    eta_secs: null,
    current_operation: err instanceof Error ? err.message : String(err),
  };
}

function restoreReportText(imagePath: string, targetPath: string): string {
  return [
    "# FlowClone restore report",
    "",
    `- Image: ${imagePath}`,
    `- Target: ${targetPath}`,
    "- Mode: Restore Image preview",
    "- Result: completed",
    "- Disk writes: stubbed",
    "",
  ].join("\n");
}

function parseInsufficientSpace(text: string): InsufficientSpaceDetails | null {
  const match = text.match(
    /^not enough space for image in (.+): need (.+) plus (.+) reserve, available (.+)$/
  );
  if (!match) return null;
  return {
    location: match[1],
    required: match[2],
    reserve: match[3],
    available: match[4],
  };
}

function shellQuote(value: string): string {
  return `'${value.replaceAll("'", "'\\''")}'`;
}
