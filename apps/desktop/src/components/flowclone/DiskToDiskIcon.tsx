/**
 * Direct Clone icon — two drives stacked, conveying a direct disk-to-disk clone.
 * Each bar is a drive (with an activity light), echoing a drive bay. Custom-drawn
 * but follows lucide's stroke conventions (24×24 viewBox, stroke width 2,
 * currentColor, round caps/joins) so it sits in the same visual family as the
 * HardDriveUpload / HardDriveDownload icons used by the other modes.
 */
export function DiskToDiskIcon({ className }: { className?: string }) {
  return (
    <svg
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={2}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      className={className}
    >
      {/* top drive */}
      <rect x="3" y="4" width="18" height="6.5" rx="1.5" />
      <line x1="6.5" y1="7.25" x2="6.51" y2="7.25" />
      {/* bottom drive */}
      <rect x="3" y="13.5" width="18" height="6.5" rx="1.5" />
      <line x1="6.5" y1="16.75" x2="6.51" y2="16.75" />
    </svg>
  );
}
