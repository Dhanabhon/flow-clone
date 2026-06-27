import { HomeScreen } from "@/features/disk-selection/HomeScreen";
import { useFlowStore } from "@/stores/flow-store";

/**
 * Top-level router. FlowClone has no sidebar/tabs — just four screens whose
 * transitions are driven by the central flow store (see DESIGN.md):
 *
 *   home → confirmation → cloning → completed
 */
export function Routes() {
  const phase = useFlowStore((s) => s.phase);

  switch (phase) {
    case "home":
      return <HomeScreen />;
    case "confirmation":
      return <ConfirmationPlaceholder />;
    case "cloning":
      return <CloningPlaceholder />;
    case "completed":
      return <CompletedPlaceholder />;
  }
}

// Minimal placeholders so the screen graph compiles. Each feature owns its
// full implementation under src/features/<name>/.
function ConfirmationPlaceholder() {
  return <Screen title="Confirmation" />;
}
function CloningPlaceholder() {
  return <Screen title="Cloning" />;
}
function CompletedPlaceholder() {
  return <Screen title="Completed" />;
}

function Screen({ title }: { title: string }) {
  return (
    <main className="mx-auto flex min-h-screen max-w-content items-center justify-center p-8">
      <div className="rounded-card border border-border bg-surface p-12 shadow-soft">
        <h1 className="text-3xl font-semibold">{title}</h1>
        <p className="mt-2 text-muted">FlowClone · {title} screen</p>
      </div>
    </main>
  );
}
