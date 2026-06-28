import { useEffect, useMemo, useState } from "react";
import type { ReactNode } from "react";
import { message, open, save } from "@tauri-apps/plugin-dialog";
import { motion } from "framer-motion";
import { AlertTriangle, FileArchive, HardDrive, RefreshCw, Upload } from "lucide-react";
import { Button } from "@/components/ui/button";
import { DiskCard } from "@/components/flowclone/DiskCard";
import { FlowArrow } from "@/components/flowclone/FlowArrow";
import { useDisks } from "@/hooks/use-disks";
import { useFlowStore } from "@/stores/flow-store";
import {
  createImageStub,
  discardPendingImage,
  dismissPendingImage,
  ejectDisk,
  isTauriRuntime,
  pendingImageJob,
  type PendingImage,
} from "@/lib/tauri";
import { useI18n } from "@/lib/i18n";
import { cn, fileNameFromPath, formatBytes, formatDuration } from "@/lib/utils";
import type { DiskInfo, Progress } from "@/lib/types";
import appLogo from "@/assets/app-logo.png";

const IMAGE_ESTIMATE_BYTES_PER_SEC = 300_000_000;

/**
 * Screen 1 — Home. Two equal disk cards (source → target), a flow arrow, a
 * warning banner after target selection, and the Start Clone button that stays
 * disabled until selection + size validation pass.
 */
export function HomeScreen() {
  const { t } = useI18n();
  const { data: disks, error, isFetching, isLoading, refetch } = useDisks();
  const {
    mode,
    source,
    target,
    imagePath,
    setMode,
    setSource,
    setTarget,
    setImagePath,
    setProgress,
    beginClone,
    goTo,
  } = useFlowStore();
  const selectableDisks = useMemo(
    () => (disks ?? []).filter(isSelectableDisk),
    [disks]
  );

  // Surface an image job that was interrupted by a crash or power loss.
  const [pending, setPending] = useState<PendingImage | null>(null);
  useEffect(() => {
    let active = true;
    pendingImageJob()
      .then((job) => {
        if (active) setPending(job);
      })
      .catch(() => {});
    return () => {
      active = false;
    };
  }, []);

  async function discardPending() {
    await discardPendingImage();
    setPending(null);
  }

  async function dismissPending() {
    await dismissPendingImage();
    setPending(null);
  }

  async function eject(disk: DiskInfo) {
    try {
      await ejectDisk(disk.device_path);
      // The disk list refreshes automatically via the native watcher when the
      // device disappears.
    } catch (err) {
      const detail = err instanceof Error ? err.message : String(err);
      await message(t("ejectFailedBody", { name: disk.model, message: detail }), {
        title: t("ejectFailed"),
        kind: "error",
      });
    }
  }

  const canStart = useMemo(() => {
    if (!source || !target) return false;
    if (source.device_path === target.device_path) return false;
    return target.total_bytes >= source.total_bytes;
  }, [source, target]);

  const sameDisk =
    source && target && source.device_path === target.device_path;
  const tooSmall =
    source && target && target.total_bytes < source.total_bytes;
  const isImageMode = mode === "image";
  const isRestoreMode = mode === "restore";
  const isCloneMode = mode === "clone";
  const showTargetWarning =
    !!target && (isRestoreMode || (isCloneMode && !tooSmall && !sameDisk));
  const imageSource =
    source && isSelectableDisk(source)
      ? source
      : selectableDisks.length === 1
        ? selectableDisks[0]
        : null;

  async function chooseImageLocation() {
    const defaultImageName = defaultImageFileName();
    const path = isTauriRuntime()
      ? await save({
          defaultPath: defaultImageName,
          filters: [{ name: t("flowCloneImage"), extensions: ["flowimg"] }],
        })
      : window.prompt(t("saveImagePrompt"), defaultImageName);
    if (path) {
      setImagePath(path);
    }
  }

  async function chooseRestoreImage() {
    const selected = isTauriRuntime()
      ? await open({
          multiple: false,
          filters: [{ name: t("flowCloneImage"), extensions: ["flowimg"] }],
        })
      : window.prompt(t("openImagePrompt"), "");
    const path = Array.isArray(selected) ? selected[0] : selected;
    if (path) {
      setImagePath(path);
    }
  }

  async function startImageMigration() {
    if (!imageSource || !imagePath) return;

    try {
      setSource(imageSource);
      setTarget(null);
      const jobId = await createImageStub(imageSource.device_path, imagePath);
      beginClone(jobId, "image");
    } catch (err) {
      setProgress(failedImageProgress(imageSource, err));
      goTo("cloning");
    }
  }

  return (
    <main className="mx-auto min-h-screen max-w-content px-8 pb-12 pt-24 sm:py-12">
      <header className="mb-12 text-center">
        <img
          src={appLogo}
          alt="FlowClone"
          className="mx-auto mb-5 h-20 w-20 rounded-button border border-border object-cover shadow-soft"
        />
        <h1 className="text-4xl font-semibold tracking-tight">
          {t("homeTitle")}
        </h1>
        <p className="mx-auto mt-3 max-w-2xl text-lg text-muted">
          {t("homeSubtitle")}
        </p>
      </header>

      {pending && (
        <InterruptedJobBanner
          pending={pending}
          onDiscard={discardPending}
          onDismiss={dismissPending}
        />
      )}

      <div className="mb-6 flex flex-wrap items-center justify-center gap-3">
        <div className="inline-grid grid-cols-3 rounded-button border border-border bg-surface p-1 shadow-soft">
          <ModeButton
            active={mode === "image"}
            icon={<FileArchive className="h-4 w-4" />}
            label={t("imageMigrationMode")}
            onClick={() => setMode("image")}
          />
          <ModeButton
            active={mode === "restore"}
            icon={<Upload className="h-4 w-4" />}
            label={t("restoreImageMode")}
            onClick={() => setMode("restore")}
          />
          <ModeButton
            active={mode === "clone"}
            icon={<HardDrive className="h-4 w-4" />}
            label={t("directCloneMode")}
            onClick={() => setMode("clone")}
            disabled
            tooltip={t("comingSoon")}
          />
        </div>
        <button
          type="button"
          onClick={() => refetch()}
          title={t("refreshDisks")}
          aria-label={t("refreshDisks")}
          aria-busy={isFetching}
          className="inline-flex h-9 w-9 items-center justify-center rounded-button border border-border bg-surface text-muted shadow-soft transition hover:bg-elevated hover:text-primary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
        >
          <RefreshCw
            className={isFetching ? "h-4 w-4 animate-spin" : "h-4 w-4"}
            strokeWidth={2}
          />
        </button>
      </div>

      {isLoading && (
        <p className="text-center text-muted">{t("scanningDisks")}</p>
      )}

      {!isLoading && error && (
        <div className="rounded-card border border-danger/30 bg-danger/10 p-8 text-center">
          <p className="text-lg font-medium text-danger">{t("diskScanFailed")}</p>
          <p className="mt-2 text-sm text-muted">{error.message}</p>
          <Button className="mt-4" variant="secondary" onClick={() => refetch()}>
            {t("refreshDisks")}
          </Button>
        </div>
      )}

      {!isLoading && !error && selectableDisks.length === 0 && (
        <EmptyState />
      )}

      {!error && selectableDisks.length > 0 && isImageMode && (
        <div className="mx-auto max-w-xl">
          <Slot
            label={t("source")}
            disks={selectableDisks}
            selected={imageSource}
            onSelect={setSource}
            onEject={eject}
            emptyText={t("connectDrives")}
          />
        </div>
      )}

      {!error && selectableDisks.length > 0 && isRestoreMode && (
        <div className="mx-auto max-w-xl">
          <Slot
            label={t("target")}
            disks={selectableDisks}
            selected={target}
            onSelect={setTarget}
            onEject={eject}
            emptyText={t("connectTargetDrive")}
          />
        </div>
      )}

      {!error && selectableDisks.length > 0 && isCloneMode && (
        <div className="grid grid-cols-1 items-center gap-6 lg:grid-cols-[1fr_auto_1fr]">
          <Slot
            label={t("source")}
            disks={selectableDisks}
            selected={source}
            exclude={target?.device_path}
            onSelect={setSource}
            onEject={eject}
            emptyText={t("connectDrives")}
          />

          <FlowArrow active={!!source && !!target} />

          <Slot
            label={t("target")}
            disks={selectableDisks}
            selected={target}
            exclude={source?.device_path}
            onSelect={setTarget}
            onEject={eject}
            emptyText={t("connectTargetDrive")}
          />
        </div>
      )}

      {isImageMode && !error && selectableDisks.length > 0 && (
        <ImageMigrationPanel
          canCreate={!!imageSource && !!imagePath}
          imagePath={imagePath}
          source={imageSource}
          onChooseLocation={chooseImageLocation}
          onCreate={startImageMigration}
        />
      )}

      {isRestoreMode && !error && selectableDisks.length > 0 && (
        <RestoreImagePanel
          canRestore={!!target && !!imagePath}
          imagePath={imagePath}
          target={target}
          onChooseImage={chooseRestoreImage}
          onRestore={() => goTo("confirmation")}
        />
      )}

      {showTargetWarning && target && (
        <motion.div
          initial={{ opacity: 0, y: 8 }}
          animate={{ opacity: 1, y: 0 }}
          className="mt-8 rounded-input border border-warning/30 bg-warning/10 p-4 text-center text-sm text-warning"
        >
          {t("targetWillBeErased", { path: target.device_path })}
        </motion.div>
      )}

      {isCloneMode && sameDisk && (
        <p className="mt-8 text-center text-sm text-danger">
          {t("sameDiskError")}
        </p>
      )}
      {isCloneMode && tooSmall && (
        <p className="mt-8 text-center text-sm text-danger">
          {t("targetTooSmallError")}
        </p>
      )}

      {isCloneMode && (
        <div className="mt-8 flex justify-center">
          <Button
            size="lg"
            className="w-full max-w-md"
            disabled={!canStart}
            onClick={() => goTo("confirmation")}
          >
            {t("startClone")}
          </Button>
        </div>
      )}

      {isCloneMode && source && target && (
        <p className="mt-3 text-center text-xs text-muted">
          {formatBytes(source.total_bytes)} → {formatBytes(target.total_bytes)}
        </p>
      )}
    </main>
  );
}

function isSelectableDisk(disk: DiskInfo) {
  return !disk.is_boot && !disk.read_only && disk.connection !== "internal";
}

function defaultImageFileName() {
  const now = new Date();
  const pad = (value: number) => String(value).padStart(2, "0");
  const date = [
    now.getFullYear(),
    pad(now.getMonth() + 1),
    pad(now.getDate()),
  ].join("-");
  const time = [
    pad(now.getHours()),
    pad(now.getMinutes()),
    pad(now.getSeconds()),
  ].join("-");
  return `FlowClone-${date}_${time}.flowimg`;
}

function ModeButton({
  active,
  icon,
  label,
  onClick,
  disabled,
  tooltip,
}: {
  active: boolean;
  icon: ReactNode;
  label: string;
  onClick: () => void;
  disabled?: boolean;
  tooltip?: string;
}) {
  return (
    <div className="group relative">
      <button
        type="button"
        aria-pressed={active}
        disabled={disabled}
        title={disabled ? tooltip : undefined}
        onClick={disabled ? undefined : onClick}
        className={cn(
          "inline-flex h-9 w-full items-center justify-center gap-2 rounded-input px-4 text-sm font-medium transition",
          disabled
            ? "cursor-not-allowed text-muted/50"
            : active
              ? "bg-primary text-white shadow-soft"
              : "text-muted hover:bg-elevated hover:text-text"
        )}
      >
        {icon}
        <span>{label}</span>
      </button>
      {disabled && tooltip && (
        <span
          role="tooltip"
          className="pointer-events-none absolute left-1/2 top-full z-20 mt-2 -translate-x-1/2 whitespace-nowrap rounded-input bg-text px-2 py-1 text-xs font-medium text-background opacity-0 shadow-soft transition-opacity duration-150 group-hover:opacity-100"
        >
          {tooltip}
        </span>
      )}
    </div>
  );
}

function ImageMigrationPanel({
  canCreate,
  imagePath,
  source,
  onChooseLocation,
  onCreate,
}: {
  canCreate: boolean;
  imagePath: string | null;
  source: DiskInfo | null;
  onChooseLocation: () => void;
  onCreate: () => void;
}) {
  const { t } = useI18n();

  return (
    <motion.section
      initial={{ opacity: 0, y: 8 }}
      animate={{ opacity: 1, y: 0 }}
      className="mx-auto mt-8 max-w-3xl rounded-card border border-border bg-surface p-6 shadow-soft"
    >
      <h2 className="text-center text-lg font-semibold">
        {t("imageMigrationTitle")}
      </h2>
      <p className="mx-auto mt-2 max-w-2xl text-center text-sm text-muted">
        {t("imageMigrationBody")}
      </p>

      <div className="mt-6 grid gap-3 md:grid-cols-2">
        <StepBlock
          index="1"
          title={t("imageSourceStep")}
          body={t("imageSourceStepBody")}
        />
        <StepBlock
          index="2"
          title={t("imageDestinationStep")}
          body={
            imagePath
              ? fileNameFromPath(imagePath, "migration.flowimg")
              : t("imageDestinationEmpty")
          }
          emphasized={!!imagePath}
        />
      </div>
      {source && (
        <p className="mt-4 text-center text-xs text-muted">
          {t("imageEstimatedTime", {
            duration: estimateImageDuration(source.total_bytes),
          })}
        </p>
      )}

      <div className="mt-6 flex flex-col justify-center gap-3 sm:flex-row">
        <Button variant="secondary" onClick={onChooseLocation}>
          {imagePath ? t("chooseDifferentLocation") : t("chooseImageLocation")}
        </Button>
        <Button disabled={!canCreate} onClick={onCreate}>
          {t("createMigrationImage")}
        </Button>
      </div>
    </motion.section>
  );
}

function RestoreImagePanel({
  canRestore,
  imagePath,
  target,
  onChooseImage,
  onRestore,
}: {
  canRestore: boolean;
  imagePath: string | null;
  target: DiskInfo | null;
  onChooseImage: () => void;
  onRestore: () => void;
}) {
  const { t } = useI18n();

  return (
    <motion.section
      initial={{ opacity: 0, y: 8 }}
      animate={{ opacity: 1, y: 0 }}
      className="mx-auto mt-8 max-w-3xl rounded-card border border-border bg-surface p-6 shadow-soft"
    >
      <h2 className="text-center text-lg font-semibold">
        {t("restoreImageTitle")}
      </h2>
      <p className="mx-auto mt-2 max-w-2xl text-center text-sm text-muted">
        {t("restoreImageBody")}
      </p>

      <div className="mt-6 grid gap-3 md:grid-cols-2">
        <StepBlock
          index="1"
          title={t("restoreImageStep")}
          body={
            imagePath
              ? fileNameFromPath(imagePath, "migration.flowimg")
              : t("restoreImageEmpty")
          }
          emphasized={!!imagePath}
        />
        <StepBlock
          index="2"
          title={t("restoreTargetStep")}
          body={
            target
              ? `${target.model} · ${formatBytes(target.total_bytes)}`
              : t("restoreTargetEmpty")
          }
          emphasized={!!target}
        />
      </div>

      <div className="mt-6 flex flex-col justify-center gap-3 sm:flex-row">
        <Button variant="secondary" onClick={onChooseImage}>
          {imagePath ? t("chooseDifferentImage") : t("chooseRestoreImage")}
        </Button>
        <Button disabled={!canRestore} onClick={onRestore}>
          {t("restoreImageAction")}
        </Button>
      </div>
    </motion.section>
  );
}

function estimateImageDuration(bytes: number) {
  return formatDuration(bytes / IMAGE_ESTIMATE_BYTES_PER_SEC);
}

function failedImageProgress(source: DiskInfo, err: unknown): Progress {
  return {
    job_id: `image-preflight-${Date.now()}`,
    phase: "failed",
    fraction: 0,
    bytes_done: 0,
    bytes_total: source.total_bytes,
    read_speed: 0,
    write_speed: 0,
    elapsed_secs: 0,
    eta_secs: null,
    current_operation: err instanceof Error ? err.message : String(err),
  };
}

function StepBlock({
  index,
  title,
  body,
  emphasized,
}: {
  index: string;
  title: string;
  body: string;
  emphasized?: boolean;
}) {
  return (
    <div className="rounded-input border border-border bg-background p-4">
      <div className="flex items-start gap-3">
        <span className="grid h-7 w-7 shrink-0 place-items-center rounded-full bg-primary text-sm font-semibold text-white">
          {index}
        </span>
        <div className="min-w-0">
          <h3 className="text-sm font-semibold">{title}</h3>
          <p
            className={cn(
              "mt-1 break-words text-sm",
              emphasized ? "font-medium text-text" : "text-muted"
            )}
          >
            {body}
          </p>
        </div>
      </div>
    </div>
  );
}

/** A source/target slot: either the selected card or a pickable list. */
function Slot({
  label,
  disks,
  selected,
  exclude,
  onSelect,
  onEject,
  emptyText,
}: {
  label: string;
  disks: DiskInfo[];
  selected: DiskInfo | null;
  exclude?: string;
  onSelect: (d: DiskInfo | null) => void;
  onEject?: (d: DiskInfo) => void;
  emptyText: string;
}) {
  const available = disks.filter((d) => d.device_path !== exclude);

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
          onEject={onEject ? () => onEject(selected) : undefined}
        />
      ) : (
        <div className="flex flex-col gap-3">
          {available.length === 0 ? (
            <div className="rounded-card border border-dashed border-border bg-surface p-8 text-center text-sm text-muted">
              {emptyText}
            </div>
          ) : (
            available.map((d) => (
              <DiskCard
                key={d.device_path}
                disk={d}
                selected={false}
                disabled={d.is_boot || d.read_only}
                onSelect={() => onSelect(d)}
                onEject={onEject ? () => onEject(d) : undefined}
              />
            ))
          )}
        </div>
      )}
    </section>
  );
}

function InterruptedJobBanner({
  pending,
  onDiscard,
  onDismiss,
}: {
  pending: PendingImage;
  onDiscard: () => void;
  onDismiss: () => void;
}) {
  const { t } = useI18n();
  const name = fileNameFromPath(pending.image_path, "migration.flowimg");
  const progress =
    pending.total_bytes > 0
      ? `${formatBytes(pending.bytes_done)} / ${formatBytes(pending.total_bytes)}`
      : formatBytes(pending.bytes_done);

  return (
    <motion.section
      initial={{ opacity: 0, y: 8 }}
      animate={{ opacity: 1, y: 0 }}
      className="mx-auto mb-8 max-w-3xl rounded-card border border-warning/30 bg-warning/10 p-5 text-left"
    >
      <div className="flex items-start gap-3">
        <AlertTriangle className="mt-0.5 h-5 w-5 shrink-0 text-warning" />
        <div className="min-w-0 flex-1">
          <h2 className="text-sm font-semibold text-warning">
            {t("interruptedJobTitle")}
          </h2>
          <p className="mt-1 text-sm text-muted">
            {t("interruptedJobBody", { name, progress })}
          </p>
          <div className="mt-4 flex flex-col gap-3 sm:flex-row">
            <Button size="sm" variant="secondary" onClick={onDiscard}>
              {t("discardPartialFile")}
            </Button>
            <Button size="sm" variant="secondary" onClick={onDismiss}>
              {t("dismiss")}
            </Button>
          </div>
        </div>
      </div>
    </motion.section>
  );
}

function EmptyState() {
  const { t } = useI18n();

  return (
    <div className="rounded-card border border-dashed border-border bg-surface p-16 text-center">
      <p className="text-lg font-medium">{t("noDrives")}</p>
      <p className="mt-1 text-muted">{t("connectDrives")}</p>
    </div>
  );
}
