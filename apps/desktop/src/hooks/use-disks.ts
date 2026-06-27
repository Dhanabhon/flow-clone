import { useQuery } from "@tanstack/react-query";
import { listDisks } from "@/lib/tauri";

/**
 * Poll the disk catalog. DESIGN.md says the empty state should auto-refresh
 * every second; we refresh once a second regardless so newly-plugged drives
 * show up without a manual reload.
 */
export function useDisks() {
  return useQuery({
    queryKey: ["disks"],
    queryFn: listDisks,
    refetchInterval: 1000,
  });
}
