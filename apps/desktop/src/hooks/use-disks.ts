import { useEffect } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { listDisks, onDisksChanged } from "@/lib/tauri";

/** Long fallback refetch in case a native disk event is ever missed. */
const FALLBACK_REFETCH_MS = 30_000;
/** Coalesce the burst of events from plugging in one drive (disk + partitions). */
const REFRESH_DEBOUNCE_MS = 400;

/**
 * Disk catalog, refreshed when storage actually changes.
 *
 * The native DiskArbitration watcher emits `disks://changed` on attach/detach;
 * we debounce those and invalidate the query so newly-plugged drives appear
 * instantly without polling `diskutil` every second. A slow fallback poll keeps
 * the list correct even if an event is ever missed.
 */
export function useDisks() {
  const queryClient = useQueryClient();

  useEffect(() => {
    let active = true;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let unlisten: (() => void) | undefined;

    onDisksChanged(() => {
      clearTimeout(timer);
      timer = setTimeout(() => {
        queryClient.invalidateQueries({ queryKey: ["disks"] });
      }, REFRESH_DEBOUNCE_MS);
    }).then((fn) => {
      if (active) unlisten = fn;
      else fn();
    });

    return () => {
      active = false;
      clearTimeout(timer);
      unlisten?.();
    };
  }, [queryClient]);

  return useQuery({
    queryKey: ["disks"],
    queryFn: listDisks,
    refetchInterval: FALLBACK_REFETCH_MS,
  });
}
