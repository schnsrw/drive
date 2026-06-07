/**
 * Settings → Storage → Workspace bucket card. Pipeline §8.9.
 * Spec: docs/ux/15-byo-storage-surface.md.
 *
 * Owner-only on Team workspaces; the component returns null otherwise so
 * Personal / Member surfaces never see it.
 */
import { useEffect, useState } from "react";
import { Database, RotateCw, Trash2 } from "lucide-react";
import { toast } from "sonner";

import {
  type ByoConfigInput,
  type ByoProvider,
  type ByoStatus,
  type Workspace,
  getWorkspaceStorage,
  listWorkspaces,
  removeWorkspaceStorage,
  replaceWorkspaceStorageCredentials,
  saveWorkspaceStorage,
  testWorkspaceStorage,
} from "../api/client.ts";
import { SettingsCard } from "../pages/settings/SettingsHeader.tsx";
import { useActiveWorkspaceId } from "../state/WorkspaceContext.tsx";

type TestState =
  | { kind: "idle" }
  | { kind: "running" }
  | { kind: "ok"; latencyMs: number }
  | { kind: "fail"; message: string };

const PROVIDER_LABELS: Record<ByoProvider, string> = {
  s3: "Amazon S3",
  minio: "MinIO",
  r2: "Cloudflare R2",
  b2: "Backblaze B2",
};

export function WorkspaceStorageCard() {
  const workspaceId = useActiveWorkspaceId();
  const [active, setActive] = useState<Workspace | null>(null);
  const [status, setStatus] = useState<ByoStatus | null>(null);
  const [loading, setLoading] = useState(true);
  const [editing, setEditing] = useState(false);
  const [replacingCreds, setReplacingCreds] = useState(false);

  // Load the active workspace's full row (we need role + kind) + current
  // storage status. Re-runs when the switcher fires.
  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    (async () => {
      try {
        const ws = await listWorkspaces();
        const me = ws.workspaces.find((w) => w.id === (workspaceId ?? ws.current_id)) ?? null;
        if (cancelled) return;
        setActive(me);
        if (!me || me.kind !== "team" || me.role !== "owner") {
          setStatus(null);
          return;
        }
        const s = await getWorkspaceStorage(me.id);
        if (!cancelled) setStatus(s);
      } catch {
        if (!cancelled) setStatus(null);
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [workspaceId]);

  // Card is owner-only on Team workspaces. Hide entirely otherwise so the
  // surface never even hints that BYO exists for users who can't use it.
  if (!active || active.kind !== "team" || active.role !== "owner") return null;

  return (
    <SettingsCard
      title="Workspace storage"
      subtitle={`Where uploads in “${active.name}” physically land. New uploads go to whichever storage is active at upload time; existing files stay where they are.`}
    >
      {loading ? (
        <Skeleton />
      ) : !status || status.kind === "default" ? (
        <DefaultState onConfigure={() => setEditing(true)} />
      ) : (
        <ActiveState
          status={status}
          onReplaceCreds={() => setReplacingCreds(true)}
          onRemove={async () => {
            if (!confirm(
              "Files already uploaded will continue to live on this bucket. " +
                "New uploads will go to the server default. Continue?",
            )) return;
            try {
              await removeWorkspaceStorage(active.id);
              const s = await getWorkspaceStorage(active.id);
              setStatus(s);
              toast.success("Removed custom storage");
            } catch {
              toast.error("Couldn't remove storage");
            }
          }}
        />
      )}

      {editing && (
        <ConfigureForm
          workspaceId={active.id}
          onCancel={() => setEditing(false)}
          onSaved={async () => {
            setEditing(false);
            const s = await getWorkspaceStorage(active.id);
            setStatus(s);
          }}
        />
      )}

      {replacingCreds && status?.kind === "byo" && (
        <ReplaceCredentialsForm
          workspaceId={active.id}
          provider={status.provider}
          onCancel={() => setReplacingCreds(false)}
          onSaved={async () => {
            setReplacingCreds(false);
            const s = await getWorkspaceStorage(active.id);
            setStatus(s);
          }}
        />
      )}
    </SettingsCard>
  );
}

// ── State views ──────────────────────────────────────────────────────

function DefaultState({ onConfigure }: { onConfigure: () => void }) {
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 14 }}>
      <Row
        icon={<Database size={16} strokeWidth={1.7} />}
        label="Backend"
        value="Server default"
        hint="Uses whichever backend the host has configured for everyone."
      />
      <Action onClick={onConfigure}>Configure custom bucket</Action>
    </div>
  );
}

function ActiveState({
  status,
  onReplaceCreds,
  onRemove,
}: {
  status: Extract<ByoStatus, { kind: "byo" }>;
  onReplaceCreds: () => void;
  onRemove: () => void | Promise<void>;
}) {
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
      <Row
        icon={<Database size={16} strokeWidth={1.7} />}
        label="Backend"
        value={`${PROVIDER_LABELS[status.provider]} · ${status.bucket} · ${status.region}`}
        hint={
          status.tested_ok
            ? `Connected · last tested ${formatRelative(status.tested_at)}`
            : status.tested_error
              ? `Last test failed: ${status.tested_error}`
              : "Not tested yet"
        }
        hintOk={status.tested_ok}
      />
      <Row label="Endpoint" value={status.endpoint ?? "AWS default"} />
      <Row label="Access ID" value={status.access_key_id_masked} />
      <Row label="Secret" value={status.secret_masked + " (encrypted at rest)"} />
      <div style={{ display: "flex", gap: 8, marginTop: 4, flexWrap: "wrap" }}>
        <Secondary onClick={onReplaceCreds}>
          <RotateCw size={13} strokeWidth={1.8} /> Replace credentials
        </Secondary>
        <Secondary onClick={onRemove} danger>
          <Trash2 size={13} strokeWidth={1.8} /> Remove
        </Secondary>
      </div>
    </div>
  );
}

// ── Forms ────────────────────────────────────────────────────────────

function ConfigureForm({
  workspaceId,
  onCancel,
  onSaved,
}: {
  workspaceId: string;
  onCancel: () => void;
  onSaved: () => Promise<void>;
}) {
  const [cfg, setCfg] = useState<ByoConfigInput>({
    provider: "s3",
    bucket: "",
    region: "us-east-1",
    endpoint: "",
    access_key_id: "",
    secret_access_key: "",
  });
  const [test, setTest] = useState<TestState>({ kind: "idle" });
  const [saving, setSaving] = useState(false);

  const endpointRequired = cfg.provider !== "s3";

  function update<K extends keyof ByoConfigInput>(k: K, v: ByoConfigInput[K]) {
    setCfg((prev) => ({ ...prev, [k]: v }));
    setTest({ kind: "idle" });
  }

  const canTest =
    cfg.bucket.trim().length > 0 &&
    cfg.region.trim().length > 0 &&
    cfg.access_key_id.trim().length > 0 &&
    cfg.secret_access_key.length > 0 &&
    (!endpointRequired || (cfg.endpoint ?? "").trim().length > 0);

  async function runTest() {
    if (!canTest) return;
    setTest({ kind: "running" });
    try {
      const r = await testWorkspaceStorage(workspaceId, cfg);
      if (r.ok) setTest({ kind: "ok", latencyMs: r.latency_ms ?? 0 });
      else setTest({ kind: "fail", message: r.error ?? "Test failed" });
    } catch (e: unknown) {
      const msg =
        e && typeof e === "object" && "message" in e
          ? String((e as { message: unknown }).message)
          : "Test failed";
      setTest({ kind: "fail", message: msg });
    }
  }

  async function save() {
    if (test.kind !== "ok") return;
    setSaving(true);
    try {
      await saveWorkspaceStorage(workspaceId, cfg);
      toast.success("Custom storage configured");
      await onSaved();
    } catch (e: unknown) {
      const msg =
        e && typeof e === "object" && "message" in e
          ? String((e as { message: unknown }).message)
          : "Save failed";
      toast.error(msg);
    } finally {
      setSaving(false);
    }
  }

  return (
    <FormShell title="Configure custom bucket">
      <ProviderPicker
        value={cfg.provider}
        onChange={(p) => update("provider", p)}
      />
      <Field label="Bucket" value={cfg.bucket} onChange={(v) => update("bucket", v)} />
      <Field label="Region" value={cfg.region} onChange={(v) => update("region", v)} />
      {endpointRequired && (
        <Field
          label="Endpoint"
          value={cfg.endpoint ?? ""}
          onChange={(v) => update("endpoint", v)}
          placeholder="https://minio.internal:9000"
        />
      )}
      <Field
        label="Access key ID"
        value={cfg.access_key_id}
        onChange={(v) => update("access_key_id", v)}
      />
      <Field
        label="Secret access key"
        value={cfg.secret_access_key}
        onChange={(v) => update("secret_access_key", v)}
        type="password"
      />

      <div style={{ display: "flex", alignItems: "center", gap: 12, marginTop: 12 }}>
        <Secondary onClick={runTest} disabled={!canTest || test.kind === "running"}>
          {test.kind === "running" ? "Testing…" : "Test connection"}
        </Secondary>
        {test.kind === "ok" && (
          <span style={{ color: "var(--success)", fontSize: "var(--text-sm)" }}>
            ✓ Connected in {test.latencyMs} ms · ready to save
          </span>
        )}
        {test.kind === "fail" && (
          <span style={{ color: "var(--danger)", fontSize: "var(--text-sm)" }}>
            ✗ {test.message}
          </span>
        )}
      </div>

      <div style={{ display: "flex", justifyContent: "flex-end", gap: 8, marginTop: 16 }}>
        <Secondary onClick={onCancel}>Cancel</Secondary>
        <Primary onClick={save} disabled={test.kind !== "ok" || saving}>
          {saving ? "Saving…" : "Save"}
        </Primary>
      </div>
    </FormShell>
  );
}

function ReplaceCredentialsForm({
  workspaceId,
  provider,
  onCancel,
  onSaved,
}: {
  workspaceId: string;
  provider: ByoProvider;
  onCancel: () => void;
  onSaved: () => Promise<void>;
}) {
  const [accessKeyId, setAccessKeyId] = useState("");
  const [secret, setSecret] = useState("");
  const [saving, setSaving] = useState(false);

  async function submit() {
    if (saving || !accessKeyId.trim() || !secret) return;
    setSaving(true);
    try {
      await replaceWorkspaceStorageCredentials(workspaceId, accessKeyId.trim(), secret);
      toast.success("Credentials replaced");
      await onSaved();
    } catch (e: unknown) {
      const msg =
        e && typeof e === "object" && "message" in e
          ? String((e as { message: unknown }).message)
          : "Update failed";
      toast.error(msg);
    } finally {
      setSaving(false);
    }
  }

  return (
    <FormShell title={`Replace credentials (${PROVIDER_LABELS[provider]})`}>
      <Field
        label="Access key ID"
        value={accessKeyId}
        onChange={setAccessKeyId}
      />
      <Field
        label="Secret access key"
        value={secret}
        onChange={setSecret}
        type="password"
      />
      <div style={{ display: "flex", justifyContent: "flex-end", gap: 8, marginTop: 16 }}>
        <Secondary onClick={onCancel}>Cancel</Secondary>
        <Primary onClick={submit} disabled={saving || !accessKeyId.trim() || !secret}>
          {saving ? "Testing + saving…" : "Save"}
        </Primary>
      </div>
    </FormShell>
  );
}

// ── Atoms ────────────────────────────────────────────────────────────

function ProviderPicker({
  value,
  onChange,
}: {
  value: ByoProvider;
  onChange: (v: ByoProvider) => void;
}) {
  const opts: ByoProvider[] = ["s3", "minio", "r2", "b2"];
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
      <Label>Provider</Label>
      <div style={{ display: "flex", gap: 6, flexWrap: "wrap" }}>
        {opts.map((o) => (
          <button
            key={o}
            type="button"
            onClick={() => onChange(o)}
            style={{
              padding: "8px 12px",
              borderRadius: 8,
              border: `1px solid ${
                value === o ? "var(--accent)" : "var(--line)"
              }`,
              background: value === o ? "var(--accent-muted)" : "transparent",
              color: value === o ? "var(--ink)" : "var(--ink-soft)",
              fontFamily: "var(--font-sans)",
              fontSize: "var(--text-sm)",
              fontWeight: 500,
              cursor: "pointer",
              transition: "background 120ms, border-color 120ms",
            }}
          >
            {PROVIDER_LABELS[o]}
          </button>
        ))}
      </div>
    </div>
  );
}

function Field({
  label,
  value,
  onChange,
  type = "text",
  placeholder,
}: {
  label: string;
  value: string;
  onChange: (v: string) => void;
  type?: "text" | "password";
  placeholder?: string;
}) {
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
      <Label>{label}</Label>
      <input
        type={type}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        placeholder={placeholder}
        autoComplete="off"
        spellCheck={false}
        style={{
          padding: "10px 12px",
          fontFamily: type === "password" ? "var(--font-mono)" : "var(--font-sans)",
          fontSize: "var(--text-sm)",
          color: "var(--ink)",
          background: "var(--paper)",
          border: "1px solid var(--line-strong)",
          borderRadius: 8,
          outline: "none",
        }}
      />
    </div>
  );
}

function Label({ children }: { children: React.ReactNode }) {
  return (
    <span
      style={{
        fontSize: "var(--text-xs)",
        textTransform: "uppercase",
        letterSpacing: "0.08em",
        color: "var(--muted)",
        fontWeight: 600,
      }}
    >
      {children}
    </span>
  );
}

function FormShell({
  title,
  children,
}: {
  title: string;
  children: React.ReactNode;
}) {
  return (
    <div
      style={{
        marginTop: 14,
        padding: "14px 16px 16px",
        background: "var(--bg-subtle)",
        border: "1px solid var(--line)",
        borderRadius: 10,
        display: "flex",
        flexDirection: "column",
        gap: 10,
      }}
    >
      <div style={{ fontSize: "var(--text-sm)", fontWeight: 600, color: "var(--ink)" }}>
        {title}
      </div>
      {children}
    </div>
  );
}

function Row({
  icon,
  label,
  value,
  hint,
  hintOk,
}: {
  icon?: React.ReactNode;
  label: string;
  value: string;
  hint?: string;
  hintOk?: boolean;
}) {
  return (
    <div
      style={{
        display: "flex",
        alignItems: "flex-start",
        gap: 14,
        padding: "10px 4px",
        borderBottom: "1px solid var(--line)",
      }}
    >
      {icon && (
        <span
          style={{
            width: 28,
            height: 28,
            borderRadius: 7,
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
      )}
      <div style={{ flex: 1, minWidth: 0 }}>
        <div style={{ fontSize: "var(--text-xs)", color: "var(--muted)" }}>{label}</div>
        <div
          className="tabular-nums"
          style={{
            fontSize: "var(--text-sm)",
            fontWeight: 500,
            color: "var(--ink)",
            wordBreak: "break-all",
          }}
        >
          {value}
        </div>
        {hint && (
          <div
            style={{
              marginTop: 2,
              fontSize: "var(--text-xs)",
              color: hintOk === false ? "var(--danger)" : "var(--muted-2)",
            }}
          >
            {hint}
          </div>
        )}
      </div>
    </div>
  );
}

function Action({
  onClick,
  children,
}: {
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      style={{
        alignSelf: "flex-start",
        padding: "8px 14px",
        background: "var(--ink)",
        color: "var(--paper)",
        border: "none",
        borderRadius: 8,
        fontFamily: "var(--font-sans)",
        fontSize: "var(--text-sm)",
        fontWeight: 500,
        cursor: "pointer",
      }}
    >
      {children}
    </button>
  );
}

function Primary({
  onClick,
  disabled,
  children,
}: {
  onClick: () => void;
  disabled?: boolean;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      style={{
        padding: "8px 14px",
        background: disabled ? "var(--line-strong)" : "var(--ink)",
        color: "var(--paper)",
        border: "none",
        borderRadius: 8,
        fontFamily: "var(--font-sans)",
        fontSize: "var(--text-sm)",
        fontWeight: 500,
        cursor: disabled ? "default" : "pointer",
      }}
    >
      {children}
    </button>
  );
}

function Secondary({
  onClick,
  disabled,
  danger,
  children,
}: {
  onClick: () => void;
  disabled?: boolean;
  danger?: boolean;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      style={{
        padding: "7px 12px",
        background: "transparent",
        color: danger ? "var(--danger)" : "var(--ink-soft)",
        border: `1px solid ${danger ? "rgba(178,36,36,.32)" : "var(--line-strong)"}`,
        borderRadius: 7,
        fontFamily: "var(--font-sans)",
        fontSize: "var(--text-sm)",
        fontWeight: 500,
        display: "inline-flex",
        alignItems: "center",
        gap: 6,
        cursor: disabled ? "default" : "pointer",
        opacity: disabled ? 0.5 : 1,
      }}
    >
      {children}
    </button>
  );
}

function Skeleton() {
  return (
    <div
      style={{
        height: 64,
        borderRadius: 10,
        background:
          "linear-gradient(90deg, var(--bg-subtle), var(--card) 40%, var(--bg-subtle))",
        backgroundSize: "200% 100%",
        animation: "cd-skeleton 1.4s linear infinite",
      }}
    />
  );
}

function formatRelative(iso: string | null): string {
  if (!iso) return "never";
  const t = new Date(iso).getTime();
  if (Number.isNaN(t)) return iso;
  const diffSec = Math.max(0, Math.round((Date.now() - t) / 1000));
  if (diffSec < 60) return `${diffSec}s ago`;
  if (diffSec < 3600) return `${Math.round(diffSec / 60)} min ago`;
  if (diffSec < 86400) return `${Math.round(diffSec / 3600)} h ago`;
  return new Date(iso).toLocaleString();
}
