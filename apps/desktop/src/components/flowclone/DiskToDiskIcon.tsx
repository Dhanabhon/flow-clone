/**
 * Direct Clone icon — a small drive and a larger drive with an arrow between
 * them. The size asymmetry conveys the common case: cloning onto a bigger SSD
 * (e.g. a 256 GB → 512 GB upgrade). Custom-drawn because lucide has no
 * two-drive glyph, but it follows lucide's stroke conventions (24-tall viewBox,
 * stroke width 2, currentColor, round caps/joins) so it sits in the same visual
 * family as the HardDriveUpload / HardDriveDownload icons used by the other
 * modes.
 *
 * The viewBox is wider than tall (42×24) so both drives and the arrow stay
 * legible; render it with a matching aspect, e.g. `h-4 w-7`.
 */
export function DiskToDiskIcon({ className }: { className?: string }) {
  return (
    <svg
      viewBox="0 0 42 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={2}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      className={className}
    >
      {/* source drive (smaller) */}
      <rect x="2.5" y="7.5" width="9" height="9" rx="1.5" />
      <line x1="5.5" y1="14" x2="5.51" y2="14" />
      {/* transfer arrow */}
      <line x1="14" y1="12" x2="22" y2="12" />
      <polyline points="19.5 9.5 22 12 19.5 14.5" />
      {/* target drive (larger) */}
      <rect x="25" y="4.5" width="14" height="15" rx="2" />
      <line x1="29" y1="16.5" x2="29.01" y2="16.5" />
    </svg>
  );
}
