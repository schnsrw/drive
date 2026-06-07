/**
 * Storage section — read-only readout of the configured storage backend.
 * Quota math arrives once `/api/storage/usage` lands (see PIPELINE.md §6.4).
 */
import { useEffect, useState } from "react";
import { ArrowUpCircle, HardDrive, Server } from "lucide-react";
import { toast } from "sonner";

import { me as fetchMe, requestQuotaUpgrade, type Me } from "../../api/client.ts";
import { WorkspaceStorageCard } from "../../components/WorkspaceStorageCard.tsx";
import { SettingsCard, SettingsHeader } from "./SettingsHeader.tsx";

export function StorageSection() {
  const [me, setMe] = useState<Me | null>(null);
  const [err, setErr] = useState<string | null>(null);

  useEffect(() => {
    fetchMe().then(setMe).catch((e) => setErr(String(e?.message ?? e)));
  }, []);

  return (
    <>
      <SettingsHeader
        title="Storage"
        description="The storage backend Drive is using to keep your files, plus per-workspace quota when set."
      />

      <SettingsCard
        title="Backend"
        subtitle="Configured at boot via DRIVE_STORAGE_BACKEND. Switching backends requires a restart."
      >
        {err ? (
          <Inline danger>{err}</Inline>
        ) : !me ? (
          <Skeleton />
        ) : (
          <ReadoutRow
            icon={<Server size={16} strokeWidth={1.7} />}
            label="Backend in use"
            value={me.backend}
          />
        )}
      </SettingsCard>

      <SettingsCard
        title="Usage"
        subtitle="Live storage consumed by your non-trashed files."
      >
        {!me ? (
          <Skeleton />
        ) : (
          <>
            <ReadoutRow
              icon={<HardDrive size={16} strokeWidth={1.7} />}
              label="Used"
              value={typeof me.used_bytes === "number" ? formatBytes(me.used_bytes) : "—"}
            />
            <ReadoutRow
              icon={<HardDrive size={16} strokeWidth={1.7} />}
              label="Quota"
              value={
                me.quota_bytes && me.quota_bytes > 0
                  ? formatBytes(me.quota_bytes)
                  : "Unlimited"
              }
              hint={
                me.quota_bytes
                  ? `${pctUsed(me.used_bytes, me.quota_bytes)}% used`
                  : "An admin can allocate a cap via the Admin → Users surface."
              }
            />
            {me.quota_bytes && me.quota_bytes > 0 && (
              <RequestUpgradeRow currentQuota={me.quota_bytes} />
            )}
          </>
        )}
      </SettingsCard>

      <WorkspaceStorageCard />
    </>
  );
}

function RequestUpgradeRow({ currentQuota }: { currentQuota: number }) {
  const [busy, setBusy] = useState(false);
  const [sent, setSent] = useState(false);

  async function submit() {
    if (busy || sent) return;
    setBusy(true);
    try {
      // Suggest doubling the current cap as a reasonable default.
      await requestQuotaUpgrade(currentQuota * 2);
      setSent(true);
      toast.success("Request sent to your admin", {
        description: "It'll show up in their Activity feed.",
      });
    } catch {
      toast.error("Couldn't send the request");
    } finally {
      setBusy(false);
    }
  }

  return (
    <div
      style={{
        marginTop: 14,
        padding: "12px 14px",
        background: "var(--accent-muted)",
        border: "1px solid rgba(200,164,92,.32)",
        borderRadius: 12,
        display: "flex",
        alignItems: "center",
        gap: 12,
      }}
    >
      <ArrowUpCircle size={16} strokeWidth={1.8} style={{ color: "var(--accent)" }} />
      <div style={{ flex: 1 }}>
        <div style={{ fontSize: "var(--text-sm)", fontWeight: 500, color: "var(--ink)" }}>
          Need more storage?
        </div>
        <div style={{ fontSize: "var(--text-xs)", color: "var(--muted)" }}>
          Send a request — your admin sees it in the Activity feed and can
          raise your cap from Admin → Users.
        </div>
      </div>
      <button
        type="button"
        onClick={() => void submit()}
        disabled={busy || sent}
        style={{
          padding: "8px 14px",
          borderRadius: 9,
          border: "none",
          background: sent ? "var(--success)" : busy ? "var(--line-strong)" : "var(--ink)",
          color: "var(--paper)",
          fontFamily: "var(--font-sans)",
          fontSize: "var(--text-sm)",
          fontWeight: 500,
          cursor: busy || sent ? "default" : "pointer",
        }}
      >
        {sent ? "Sent" : busy ? "Sending…" : "Request upgrade"}
      </button>
    </div>
  );
}

function ReadoutRow({
  icon,
  label,
  value,
  hint,
}: {
  icon: React.ReactNode;
  label: string;
  value: string;
  hint?: string;
}) {
  return (
    <div
      style={{
        display: "flex",
        alignItems: "center",
        gap: 14,
        padding: "12px 4px",
        borderBottom: "1px solid var(--line)",
      }}
    >
      <span
        style={{
          width: 32,
          height: 32,
          borderRadius: 8,
          background: "var(--bg-subtle)",
          color: "var(--muted)",
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          flexShrink: 0,
        }}
      >
        {icon}
      </span>
      <div style={{ flex: 1, minWidth: 0 }}>
        <div style={{ fontSize: "var(--text-sm)", color: "var(--muted)" }}>{label}</div>
        <div className="tabular-nums" style={{ fontSize: "var(--text-md)", fontWeight: 500, color: "var(--ink)" }}>
          {value}
        </div>
        {hint && (
          <div style={{ marginTop: 2, fontSize: "var(--text-xs)", color: "var(--muted-2)" }}>{hint}</div>
        )}
      </div>
    </div>
  );
}

function Skeleton() {
  return (
    <div
      style={{
        height: 52,
        borderRadius: 10,
        background: "linear-gradient(90deg, var(--bg-subtle), var(--card) 40%, var(--bg-subtle))",
        backgroundSize: "200% 100%",
        animation: "cd-skeleton 1.4s linear infinite",
      }}
    />
  );
}

function formatBytes(b: number): string {
  if (b === 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let v = b;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${i === 0 ? v : v.toFixed(v < 10 ? 1 : 0)} ${units[i]}`;
}

function pctUsed(used: number | undefined, quota: number | null | undefined): number {
  if (!used || !quota || quota <= 0) return 0;
  return Math.round((used / quota) * 100);
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
