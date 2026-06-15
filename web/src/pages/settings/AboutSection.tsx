/**
 * About section — version, build, license, and the brand mark.
 * Pulls from GET /api/about (build-stamped, no DB read).
 */
import { useEffect, useState } from "react";
import { ExternalLink } from "lucide-react";

import { getAbout, type About } from "../../api/client.ts";
import { Logo } from "../../components/Logo.tsx";
import { SettingsCard, SettingsHeader } from "./SettingsHeader.tsx";

export function AboutSection() {
  const [about, setAbout] = useState<About | null>(null);
  const [err, setErr] = useState<string | null>(null);

  useEffect(() => {
    getAbout().then(setAbout).catch((e) => setErr(String(e?.message ?? e)));
  }, []);

  return (
    <>
      <SettingsHeader
        title="About"
        description="The version of Casual Drive currently running on this instance."
      />

      <SettingsCard title="Build">
        {err ? (
          <Inline danger>{err}</Inline>
        ) : !about ? (
          <Skeleton />
        ) : (
          <div
            style={{
              display: "grid",
              gridTemplateColumns: "auto 1fr",
              gap: "12px 24px",
              alignItems: "center",
            }}
          >
            <Brand />
            <div style={{ display: "flex", flexDirection: "column", gap: 4 }}>
              <span
                style={{
                  fontFamily: "var(--font-display)",
                  fontSize: "var(--text-xl)",
                  fontWeight: 500,
                  letterSpacing: "var(--tracking-tight)",
                }}
              >
                Casual Drive
              </span>
              <span className="tabular-nums" style={{ fontSize: "var(--text-sm)", color: "var(--muted)" }}>
                v{about.version}
                {about.git_sha !== "unknown" && about.git_sha !== "demo" && (
                  <>
                    {" · "}
                    <code style={{ fontFamily: "var(--font-mono, ui-monospace, monospace)" }}>
                      {about.git_sha}
                    </code>
                  </>
                )}
              </span>
            </div>

            <Cell label="Built at" />
            <Cell value={fmtBuilt(about.built_at)} />
            <Cell label="License" />
            <Cell value={about.license} />
            <Cell label="Storage backend" />
            <Cell value={about.storage_backend} />
            <Cell label="Database" />
            <Cell value={about.db_backend} />
            <Cell label="Repository" />
            <Cell
              value={
                <a
                  href={about.repository}
                  target="_blank"
                  rel="noreferrer"
                  style={{
                    color: "var(--ink)",
                    textDecoration: "underline",
                    textDecorationThickness: 1,
                    display: "inline-flex",
                    alignItems: "center",
                    gap: 5,
                  }}
                >
                  {short(about.repository)}
                  <ExternalLink size={12} />
                </a>
              }
            />
          </div>
        )}
      </SettingsCard>

      <SettingsCard title="Acknowledgements" subtitle="Casual Drive is open source. Bug reports and contributions welcome.">
        <p
          style={{
            margin: 0,
            fontSize: "var(--text-sm)",
            color: "var(--muted)",
            lineHeight: "var(--leading-normal)",
          }}
        >
          Built on Rust, Axum, OpenDAL, sqlx, React, Vite, Radix Primitives, and the WOPI protocol.
          Typography by IBM Plex Sans and IBM Plex Mono. Icons by Lucide.
        </p>
      </SettingsCard>
    </>
  );
}

function Brand() {
  return (
    <span
      style={{
        gridRow: "1 / span 1",
        display: "inline-block",
        color: "var(--ink)",
      }}
    >
      <Logo size={56} />
    </span>
  );
}

function Cell({ label, value }: { label?: string; value?: React.ReactNode }) {
  if (label) {
    return (
      <span style={{ fontSize: "var(--text-sm)", color: "var(--muted)" }}>{label}</span>
    );
  }
  return (
    <span
      className="tabular-nums"
      style={{ fontSize: "var(--text-sm)", color: "var(--ink)", fontWeight: 500 }}
    >
      {value}
    </span>
  );
}

function Skeleton() {
  return (
    <div
      style={{
        height: 140,
        borderRadius: 10,
        background: "linear-gradient(90deg, var(--bg-subtle), var(--card) 40%, var(--bg-subtle))",
        backgroundSize: "200% 100%",
        animation: "cd-skeleton 1.4s linear infinite",
      }}
    />
  );
}

function Inline({ children, danger }: { children: React.ReactNode; danger?: boolean }) {
  return (
    <div
      style={{
        padding: "10px 12px",
        background: danger ? "rgba(178,36,36,.06)" : "var(--bg-subtle)",
        border: `1px solid ${danger ? "rgba(178,36,36,.25)" : "var(--line)"}`,
        borderRadius: 10,
        fontSize: "var(--text-sm)",
        color: danger ? "var(--danger, #B22424)" : "var(--muted)",
      }}
    >
      {children}
    </div>
  );
}

function short(url: string) {
  return url.replace(/^https?:\/\//, "").replace(/\/$/, "");
}

function fmtBuilt(iso: string) {
  if (!iso || iso === "unknown") return "—";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  // Honour the user's locale + system timezone — never raw UTC.
  return d.toLocaleString(undefined, {
    year: "numeric",
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
    timeZoneName: "short",
  });
}
