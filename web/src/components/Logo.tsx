/**
 * Casual Drive mark — black rounded square with a cloud silhouette.
 * `currentColor` paints the square so callers can flip light/dark via `color`.
 * The cloud fill (`--mark-fg`) defaults to the brand paper cream.
 *
 * Built from primitives (three bumps + flat baseline) rather than a single
 * path so the geometry stays editable and renders cleanly at favicon size.
 */
export function Logo({ size = 38, className }: { size?: number; className?: string }) {
  return (
    <svg
      viewBox="0 0 38 38"
      width={size}
      height={size}
      role="img"
      aria-label="Casual Drive"
      className={className}
      style={{ display: "block" }}
    >
      <defs>
        <clipPath id="cd-mark-clip">
          <rect width="38" height="38" rx="10" />
        </clipPath>
      </defs>
      <g clipPath="url(#cd-mark-clip)">
        <rect width="38" height="38" fill="currentColor" />
        {/* Cloud paints in the "paper" colour so it stays opposite the
            square's currentColor whichever theme is active: light mode →
            dark square, cream cloud; dark mode → cream square, dark cloud. */}
        <g fill="var(--mark-fg, var(--paper, #F2F0EA))">
          <circle cx="12" cy="22" r="5" />
          <circle cx="26" cy="22" r="5" />
          <circle cx="19" cy="15" r="7.5" />
          <rect x="12" y="22" width="14" height="5" />
        </g>
      </g>
    </svg>
  );
}

/** The wordmark — Fraunces "Casual" over uppercase letter-spaced "DRIVE". */
export function Wordmark() {
  return (
    <span style={{ display: "inline-block", lineHeight: 1 }}>
      <span
        style={{
          fontFamily: "var(--font-display)",
          fontWeight: 500,
          fontSize: 18,
          letterSpacing: "0.5px",
          display: "block",
          color: "var(--ink)",
        }}
      >
        Casual
      </span>
      <span
        style={{
          fontFamily: "var(--font-sans)",
          fontSize: 10,
          letterSpacing: "4px",
          textTransform: "uppercase",
          color: "var(--muted)",
          fontWeight: 500,
          marginTop: 3,
          display: "block",
        }}
      >
        Drive
      </span>
    </span>
  );
}
