/**
 * Inline Casual Drive mark. Uses currentColor for the dark background so the
 * caller can flip light/dark via `color`. `--mark-fg` overrides the crescent
 * tint if needed (defaults to the brand cream).
 */
export function Logo({ size = 20, className }: { size?: number; className?: string }) {
  return (
    <svg
      viewBox="0 0 172 172"
      width={size}
      height={size}
      role="img"
      aria-label="Casual Drive"
      className={className}
      style={{ display: "block" }}
    >
      <defs>
        <clipPath id="cd-mark-clip">
          <rect x="0" y="0" width="172" height="172" rx="40" />
        </clipPath>
      </defs>
      <g clipPath="url(#cd-mark-clip)">
        <rect width="172" height="172" fill="currentColor" />
        <circle cx="78" cy="86" r="52" fill="var(--mark-fg, #F5F3EE)" />
        <circle cx="104" cy="72" r="52" fill="currentColor" />
        <circle cx="112" cy="64" r="6.5" fill="var(--mark-fg, #F5F3EE)" />
      </g>
    </svg>
  );
}
