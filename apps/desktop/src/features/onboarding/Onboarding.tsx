import { useEffect, useState } from "react";
import type { ReactNode } from "react";
import { HardDriveUpload, Lock, Rocket } from "lucide-react";
import appLogo from "@/assets/app-logo.png";
import { Button } from "@/components/ui/button";
import { useI18n } from "@/lib/i18n";
import { openFullDiskAccessSettings } from "@/lib/tauri";
import { useOnboardingStore } from "@/stores/onboarding-store";

const STEP_COUNT = 4;

/** True when running on Windows — only changes the permissions-step copy. */
function isWindows(): boolean {
  try {
    return /win/i.test(navigator.userAgent);
  } catch {
    return false;
  }
}

/**
 * First-run onboarding overlay: a self-contained 4-step tour ending with the
 * permission the app needs. Independent of the workflow phase machine; shown
 * whenever the onboarding store's `open` is true.
 */
export function Onboarding() {
  const { t } = useI18n();
  const finish = useOnboardingStore((s) => s.finish);
  const [step, setStep] = useState(0);
  const [accessError, setAccessError] = useState<string | null>(null);
  const windows = isWindows();
  const isLast = step === STEP_COUNT - 1;

  // Clear any permission error when leaving the permissions step.
  useEffect(() => {
    if (step !== 2) setAccessError(null);
  }, [step]);

  async function openAccess() {
    setAccessError(null);
    try {
      await openFullDiskAccessSettings();
    } catch (err) {
      setAccessError(err instanceof Error ? err.message : String(err));
    }
  }

  return (
    <div className="fixed inset-0 z-[60] grid place-items-center bg-background p-6">
      <section className="relative w-full max-w-lg rounded-card border border-border bg-surface p-8 text-center shadow-soft">
        <button
          type="button"
          onClick={finish}
          className="absolute right-5 top-5 rounded-input px-1 text-sm font-medium text-muted outline-none transition hover:text-text focus-visible:ring-2 focus-visible:ring-primary"
        >
          {t("onboardingSkip")}
        </button>

        <p className="text-xs uppercase tracking-wide text-muted">
          {t("onboardingStepLabel", { current: step + 1, total: STEP_COUNT })}
        </p>

        {step === 0 && (
          <div className="mt-5">
            <img
              src={appLogo}
              alt="FlowClone"
              className="mx-auto h-16 w-16 rounded-2xl"
            />
            <h2 className="mt-5 text-2xl font-semibold">
              {t("onboardingWelcomeTitle")}
            </h2>
            <p className="mx-auto mt-3 max-w-sm text-sm text-muted">
              {t("onboardingWelcomeBody")}
            </p>
          </div>
        )}

        {step === 1 && (
          <StepBody
            icon={<HardDriveUpload className="h-8 w-8" />}
            title={t("onboardingWhatTitle")}
            body={t("onboardingWhatBody")}
          />
        )}

        {step === 2 && (
          <div className="mt-5">
            <Badge icon={<Lock className="h-8 w-8" />} />
            <h2 className="mt-5 text-2xl font-semibold">
              {t(windows ? "onboardingPermTitleWin" : "onboardingPermTitleMac")}
            </h2>
            <p className="mx-auto mt-3 max-w-sm text-sm text-muted">
              {t(windows ? "onboardingPermBodyWin" : "onboardingPermBodyMac")}
            </p>
            {!windows && (
              <Button className="mt-5" variant="secondary" onClick={openAccess}>
                {t("onboardingPermOpen")}
              </Button>
            )}
            {accessError && (
              <p className="mt-3 text-sm text-danger">{accessError}</p>
            )}
          </div>
        )}

        {step === 3 && (
          <StepBody
            icon={<Rocket className="h-8 w-8" />}
            title={t("onboardingReadyTitle")}
            body={t("onboardingReadyBody")}
          />
        )}

        <div className="mt-8 flex items-center justify-between">
          <Button
            variant="secondary"
            className={step === 0 ? "invisible" : ""}
            onClick={() => setStep((s) => Math.max(0, s - 1))}
          >
            {t("onboardingBack")}
          </Button>

          <div className="flex items-center gap-1.5">
            {Array.from({ length: STEP_COUNT }).map((_, i) => (
              <span
                key={i}
                className={
                  i === step
                    ? "h-2 w-4 rounded-full bg-primary"
                    : "h-2 w-2 rounded-full bg-border"
                }
              />
            ))}
          </div>

          <Button onClick={() => (isLast ? finish() : setStep((s) => s + 1))}>
            {isLast ? t("onboardingGetStarted") : t("onboardingContinue")}
          </Button>
        </div>
      </section>
    </div>
  );
}

function Badge({ icon }: { icon: ReactNode }) {
  return (
    <div className="mx-auto grid h-16 w-16 place-items-center rounded-full bg-primary/15 text-primary">
      {icon}
    </div>
  );
}

function StepBody({
  icon,
  title,
  body,
}: {
  icon: ReactNode;
  title: string;
  body: string;
}) {
  return (
    <div className="mt-5">
      <Badge icon={icon} />
      <h2 className="mt-5 text-2xl font-semibold">{title}</h2>
      <p className="mx-auto mt-3 max-w-sm text-sm text-muted">{body}</p>
    </div>
  );
}
