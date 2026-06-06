import { Sparkles } from "lucide-react";

/**
 * Top-of-shell strip shown when VITE_DEMO_MODE=1 (the drive.schnsrw.live
 * Pages build). Tells the visitor up front that there is no server and
 * their changes won't survive a reload.
 */
export function DemoBanner() {
  return (
    <div
      role="status"
      style={{
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        gap: 10,
        padding: "9px 18px",
        background: "var(--ink)",
        color: "var(--paper)",
        fontFamily: "var(--font-sans)",
        fontSize: "var(--text-sm)",
        letterSpacing: ".01em",
        flexShrink: 0,
      }}
    >
      <Sparkles size={14} strokeWidth={1.8} style={{ color: "var(--accent)" }} />
      <span>
        <strong style={{ fontWeight: 600 }}>Demo</strong> · in-memory only · changes reset on reload ·{" "}
        <a
          href="https://github.com/schnsrw/drive"
          target="_blank"
          rel="noreferrer"
          style={{ color: "var(--accent)", textDecoration: "underline", textDecorationThickness: 1 }}
        >
          self-host the real thing
        </a>
      </span>
    </div>
  );
}
