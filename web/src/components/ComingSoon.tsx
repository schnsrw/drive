import { Sparkles } from "lucide-react";

/**
 * "Coming in v0.2" empty state. First-class component — every Phase-2/3 surface
 * gets a polished ComingSoon page rather than a 404 or blank.
 */
export function ComingSoon({
  title,
  description,
  shipping = "v0.2",
  bullets,
}: {
  title: string;
  description: string;
  shipping?: string;
  bullets?: string[];
}) {
  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        alignItems: "center",
        justifyContent: "center",
        textAlign: "center",
        padding: "80px 40px",
        maxWidth: 640,
        margin: "0 auto",
      }}
    >
      <div
        style={{
          width: 96,
          height: 96,
          borderRadius: 24,
          background: "var(--accent-muted)",
          border: "1px solid var(--line)",
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          marginBottom: 22,
          color: "var(--accent)",
        }}
      >
        <Sparkles size={42} strokeWidth={1.4} />
      </div>

      <div
        style={{
          fontSize: "var(--text-xs)",
          letterSpacing: "var(--tracking-wider)",
          textTransform: "uppercase",
          color: "var(--accent)",
          fontWeight: 600,
          marginBottom: 8,
        }}
      >
        Coming in {shipping}
      </div>

      <h2
        style={{
          margin: 0,
          fontFamily: "var(--font-display)",
          fontWeight: 500,
          fontSize: "var(--text-2xl)",
          color: "var(--ink)",
          letterSpacing: "var(--tracking-tight)",
        }}
      >
        {title}
      </h2>

      <p
        style={{
          marginTop: 10,
          fontSize: "var(--text-md)",
          color: "var(--muted)",
          lineHeight: "var(--leading-normal)",
        }}
      >
        {description}
      </p>

      {bullets && bullets.length > 0 && (
        <ul
          style={{
            marginTop: 26,
            padding: 0,
            listStyle: "none",
            display: "flex",
            flexDirection: "column",
            gap: 8,
            textAlign: "left",
            maxWidth: 460,
            width: "100%",
          }}
        >
          {bullets.map((b) => (
            <li
              key={b}
              style={{
                display: "flex",
                alignItems: "flex-start",
                gap: 10,
                fontSize: "var(--text-sm)",
                color: "var(--ink-soft)",
                padding: "10px 14px",
                background: "var(--card)",
                border: "1px solid var(--line)",
                borderRadius: 12,
              }}
            >
              <span
                style={{
                  color: "var(--accent)",
                  marginTop: 2,
                  flexShrink: 0,
                  fontWeight: 600,
                }}
              >
                →
              </span>
              <span>{b}</span>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
