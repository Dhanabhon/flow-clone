import { AlertTriangle, CheckCircle2, HelpCircle, Loader2 } from "lucide-react";
import { useI18n } from "@/lib/i18n";
import { cn } from "@/lib/utils";
import type { VerifyOutcome } from "@/lib/types";

/** The four possible states for the banner. */
export type VerifyState =
  | "idle"
  | "running"
  | VerifyOutcome
  | { error: string };

function isVerifyOutcome(state: VerifyState): state is VerifyOutcome {
  return (
    typeof state === "object" &&
    state !== null &&
    "verifiable" in state &&
    !("error" in state)
  );
}

function isErrorState(state: VerifyState): state is { error: string } {
  return (
    typeof state === "object" && state !== null && "error" in state
  );
}

/**
 * Pure presentational banner for verification results.
 * Callers own the async `verifyImage` call and pass the current state down.
 */
export function VerifyResultBanner({ state }: { state: VerifyState }) {
  const { t } = useI18n();

  if (state === "idle") return null;

  // Running / spinner state
  if (state === "running") {
    return (
      <div className="mt-4 flex items-center gap-3 rounded-input border border-border bg-elevated px-4 py-3">
        <Loader2 className="h-4 w-4 shrink-0 animate-spin text-primary" />
        <p className="text-sm text-muted">{t("verifying")}</p>
      </div>
    );
  }

  // Generic error (thrown rejection from verifyImage)
  if (isErrorState(state)) {
    return (
      <div className="mt-4 rounded-input border border-danger/30 bg-danger/10 px-4 py-3">
        <div className="flex items-start gap-3">
          <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0 text-danger" />
          <p className="text-sm text-danger">{state.error}</p>
        </div>
      </div>
    );
  }

  // VerifyOutcome — three sub-states
  if (isVerifyOutcome(state)) {
    const { verifiable, matched, expected, actual } = state;

    // Unverifiable: created before checksums were added
    if (!verifiable) {
      return (
        <div className="mt-4 rounded-input border border-warning/30 bg-warning/10 px-4 py-3">
          <div className="flex items-start gap-3">
            <HelpCircle className="mt-0.5 h-4 w-4 shrink-0 text-warning" />
            <p className="text-sm text-warning">{t("verifyUnverifiable")}</p>
          </div>
        </div>
      );
    }

    // Corrupt: verifiable but checksum mismatch
    if (!matched) {
      return (
        <div className="mt-4 rounded-input border border-danger/30 bg-danger/10 px-4 py-3">
          <div className="flex items-start gap-3">
            <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0 text-danger" />
            <div className="min-w-0 flex-1">
              <p className="text-sm font-medium text-danger">{t("verifyCorrupt")}</p>
              {(expected != null || actual != null) && (
                <div className="mt-2 space-y-1">
                  {expected != null && (
                    <p className="text-xs text-muted">
                      <span className="font-medium">{t("verifyExpected")}:</span>{" "}
                      <code
                        className={cn(
                          "inline-block max-w-[28ch] truncate align-bottom font-mono",
                          "rounded bg-background px-1"
                        )}
                        title={expected}
                      >
                        {expected}
                      </code>
                    </p>
                  )}
                  {actual != null && (
                    <p className="text-xs text-muted">
                      <span className="font-medium">{t("verifyActual")}:</span>{" "}
                      <code
                        className={cn(
                          "inline-block max-w-[28ch] truncate align-bottom font-mono",
                          "rounded bg-background px-1"
                        )}
                        title={actual}
                      >
                        {actual}
                      </code>
                    </p>
                  )}
                </div>
              )}
            </div>
          </div>
        </div>
      );
    }

    // Verified: checksum matches
    return (
      <div className="mt-4 rounded-input border border-success/30 bg-success/10 px-4 py-3">
        <div className="flex items-center gap-3">
          <CheckCircle2 className="h-4 w-4 shrink-0 text-success" />
          <p className="text-sm font-medium text-success">{t("verifyVerified")}</p>
        </div>
      </div>
    );
  }

  return null;
}
