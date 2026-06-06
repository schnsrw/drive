/**
 * Share modal — owner-side. Spec: docs/ux/05-sharing-surface.md.
 *
 * One modal handles: mint, copy, list existing, revoke. Options (expiry +
 * password) live in a collapsible panel inside the modal so the default
 * 90% case (mint + copy) stays one click.
 */
import { useEffect, useState } from "react";
import * as Dialog from "@radix-ui/react-dialog";
import {
  Check,
  Clock,
  Copy as CopyIcon,
  Eye,
  EyeOff,
  Link2,
  Sliders,
  Trash2,
  X,
} from "lucide-react";
import { toast } from "sonner";

import {
  ApiError,
  createShare,
  listShares,
  revokeShare,
  type FileDto,
  type ShareDto,
} from "../api/client.ts";
import { FileThumb, inferKind } from "./FileThumb.tsx";

type Expiry = "never" | "7d" | "30d";

const EXPIRY_SECONDS: Record<Expiry, number | null> = {
  never: null,
  "7d": 7 * 24 * 60 * 60,
  "30d": 30 * 24 * 60 * 60,
};

export function ShareDialog({
  open,
  file,
  onClose,
}: {
  open: boolean;
  file: FileDto | null;
  onClose: () => void;
}) {
  const [shares, setShares] = useState<ShareDto[] | null>(null);
  const [loadErr, setLoadErr] = useState<string | null>(null);
  const [optionsOpen, setOptionsOpen] = useState(false);
  const [creating, setCreating] = useState(false);
  const [createErr, setCreateErr] = useState<string | null>(null);
  const [pwdReveal, setPwdReveal] = useState(false);
  const [password, setPassword] = useState("");
  const [expiry, setExpiry] = useState<Expiry>("7d");

  useEffect(() => {
    if (!open || !file) return;
    setShares(null);
    setLoadErr(null);
    setOptionsOpen(false);
    setPassword("");
    setExpiry("7d");
    void listShares(file.id)
      .then((r) => setShares(r.shares))
      .catch((e) => setLoadErr((e as ApiError).message ?? "Couldn't load existing links."));
  }, [open, file]);

  async function mint() {
    if (!file) return;
    setCreating(true);
    setCreateErr(null);
    try {
      const link = await createShare(file.id, {
        permissions: "view",
        password: password.trim() ? password : null,
        expires_in_seconds: EXPIRY_SECONDS[expiry],
      });
      setShares((prev) => (prev ? [link, ...prev] : [link]));
      setPassword("");
      setOptionsOpen(false);
      // Auto-copy newly-minted link to clipboard — the modal still stays
      // open so the user can hand-tweak options or revoke prior links.
      void navigator.clipboard?.writeText(link.url);
      toast.success("Link copied to clipboard");
    } catch (err) {
      const e = err as ApiError;
      const body = e.body as { error?: string } | null;
      setCreateErr(body?.error ?? e.message ?? "Couldn't create the link.");
    } finally {
      setCreating(false);
    }
  }

  async function revoke(shareId: string) {
    try {
      await revokeShare(shareId);
      setShares((prev) => (prev ? prev.filter((s) => s.id !== shareId) : prev));
      toast.success("Link revoked");
    } catch {
      toast.error("Couldn't revoke the link.");
    }
  }

  if (!file) return null;
  const kind = inferKind(file.name, file.content_type);

  return (
    <Dialog.Root open={open} onOpenChange={(o) => !o && onClose()}>
      <Dialog.Portal>
        <Dialog.Overlay
          style={{
            position: "fixed",
            inset: 0,
            background: "var(--bg-overlay)",
            backdropFilter: "blur(5px)",
            WebkitBackdropFilter: "blur(5px)",
            zIndex: 90,
            animation: "cd-fade-in 240ms var(--ease)",
          }}
        />
        <Dialog.Content
          style={{
            position: "fixed",
            top: "50%",
            left: "50%",
            transform: "translate(-50%, -50%)",
            width: "min(540px, 92vw)",
            maxHeight: "min(80vh, 720px)",
            overflow: "auto",
            background: "var(--card)",
            border: "1px solid var(--line)",
            borderRadius: 20,
            padding: 24,
            boxShadow: "var(--shadow-xl)",
            zIndex: 91,
            animation: "cd-modal-in 280ms var(--ease)",
          }}
        >
          <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between" }}>
            <Dialog.Title
              style={{
                margin: 0,
                fontFamily: "var(--font-display)",
                fontSize: "var(--text-xl)",
                fontWeight: 500,
                letterSpacing: "var(--tracking-tight)",
                color: "var(--ink)",
              }}
            >
              Share file
            </Dialog.Title>
            <Dialog.Close asChild>
              <button type="button" aria-label="Close" style={iconBtn()}>
                <X size={16} />
              </button>
            </Dialog.Close>
          </div>

          <div style={{ display: "flex", alignItems: "center", gap: 12, marginTop: 14 }}>
            <span style={{ width: 36, height: 36, borderRadius: 8, overflow: "hidden", flexShrink: 0 }}>
              <FileThumb name={file.name} kind={kind} size="small" thumbnail={file.thumbnail} />
            </span>
            <div style={{ minWidth: 0 }}>
              <div style={{ fontWeight: 500, fontSize: "var(--text-md)", whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>
                {file.name}
              </div>
              <div style={{ fontSize: "var(--text-xs)", color: "var(--muted)" }}>
                Anyone with the link can view this file.
              </div>
            </div>
          </div>

          {/* Generate / latest-link card */}
          <div
            style={{
              marginTop: 18,
              padding: 14,
              background: "var(--accent-muted)",
              border: "1px solid rgba(200,164,92,.32)",
              borderRadius: 14,
            }}
          >
            <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
              <Link2 size={16} strokeWidth={1.8} style={{ color: "var(--accent)" }} />
              <span style={{ fontWeight: 500, fontSize: "var(--text-sm)" }}>
                {shares && shares.length > 0 ? "Latest link" : "No links yet — generate one"}
              </span>
            </div>
            <button
              type="button"
              onClick={mint}
              disabled={creating}
              style={{
                marginTop: 12,
                width: "100%",
                padding: "11px 14px",
                background: creating ? "var(--line-strong)" : "var(--ink)",
                color: "var(--paper)",
                border: "none",
                borderRadius: 11,
                fontFamily: "var(--font-sans)",
                fontSize: "var(--text-sm)",
                fontWeight: 500,
                cursor: creating ? "not-allowed" : "pointer",
                display: "flex",
                alignItems: "center",
                justifyContent: "center",
                gap: 8,
                transition: "background 150ms",
              }}
            >
              {creating ? (
                "Creating…"
              ) : (
                <>
                  <CopyIcon size={14} strokeWidth={2} />
                  {shares && shares.length > 0 ? "Generate another link" : "Generate link"}
                </>
              )}
            </button>
            {createErr && (
              <div role="alert" aria-live="polite" style={inlineErr()}>
                {createErr}
              </div>
            )}
          </div>

          {/* Options (collapsible) */}
          <div style={{ marginTop: 14 }}>
            <button
              type="button"
              onClick={() => setOptionsOpen((v) => !v)}
              style={{
                display: "inline-flex",
                alignItems: "center",
                gap: 7,
                background: "transparent",
                border: "none",
                cursor: "pointer",
                color: "var(--muted)",
                padding: 4,
                fontSize: "var(--text-sm)",
              }}
            >
              <Sliders size={14} strokeWidth={1.8} />
              {optionsOpen ? "Hide options" : "Link options"}
            </button>

            {optionsOpen && (
              <div
                style={{
                  marginTop: 10,
                  padding: 16,
                  background: "var(--bg-subtle)",
                  borderRadius: 12,
                  border: "1px solid var(--line)",
                }}
              >
                <Label>Expires</Label>
                <div style={{ display: "flex", gap: 8, marginTop: 8 }}>
                  {(Object.keys(EXPIRY_SECONDS) as Expiry[]).map((opt) => (
                    <Chip key={opt} active={expiry === opt} onClick={() => setExpiry(opt)}>
                      <Clock size={12} strokeWidth={1.8} />
                      {opt === "never" ? "Never" : opt === "7d" ? "7 days" : "30 days"}
                    </Chip>
                  ))}
                </div>

                <Label style={{ marginTop: 16 }}>Password (optional)</Label>
                <div style={{ position: "relative", marginTop: 8 }}>
                  <input
                    type={pwdReveal ? "text" : "password"}
                    value={password}
                    onChange={(e) => setPassword(e.target.value)}
                    placeholder="Leave blank for no password"
                    autoComplete="new-password"
                    style={inputStyle()}
                  />
                  <button
                    type="button"
                    aria-label={pwdReveal ? "Hide password" : "Show password"}
                    onClick={() => setPwdReveal((v) => !v)}
                    style={{
                      position: "absolute",
                      right: 8,
                      top: "50%",
                      transform: "translateY(-50%)",
                      background: "transparent",
                      border: "none",
                      cursor: "pointer",
                      color: "var(--muted)",
                      padding: 6,
                      borderRadius: 6,
                    }}
                  >
                    {pwdReveal ? <EyeOff size={14} /> : <Eye size={14} />}
                  </button>
                </div>
              </div>
            )}
          </div>

          {/* Existing links */}
          <div style={{ marginTop: 20 }}>
            <Label>Active links</Label>
            <div style={{ marginTop: 8 }}>
              {loadErr ? (
                <div style={inlineErr()}>{loadErr}</div>
              ) : shares === null ? (
                <Skeleton />
              ) : shares.length === 0 ? (
                <div style={{ fontSize: "var(--text-sm)", color: "var(--muted)", padding: "10px 0" }}>
                  No active links. Generate one above to share this file.
                </div>
              ) : (
                <ul style={{ listStyle: "none", margin: 0, padding: 0, display: "flex", flexDirection: "column", gap: 8 }}>
                  {shares.map((s) => (
                    <ShareRow key={s.id} share={s} onRevoke={() => void revoke(s.id)} />
                  ))}
                </ul>
              )}
            </div>
          </div>
        </Dialog.Content>
      </Dialog.Portal>

      <style>
        {`
          @keyframes cd-fade-in   { from { opacity: 0; } to { opacity: 1; } }
          @keyframes cd-modal-in {
            from { opacity: 0; transform: translate(-50%, calc(-50% + 14px)) scale(.98); }
            to   { opacity: 1; transform: translate(-50%, -50%) scale(1); }
          }
        `}
      </style>
    </Dialog.Root>
  );
}

function ShareRow({ share, onRevoke }: { share: ShareDto; onRevoke: () => void }) {
  const [copied, setCopied] = useState(false);
  return (
    <li
      style={{
        display: "flex",
        alignItems: "center",
        gap: 10,
        padding: "11px 12px",
        background: "var(--card)",
        border: "1px solid var(--line)",
        borderRadius: 11,
      }}
    >
      <Link2 size={14} strokeWidth={1.8} style={{ color: "var(--muted)", flexShrink: 0 }} />
      <div style={{ flex: 1, minWidth: 0 }}>
        <code
          style={{
            display: "block",
            fontFamily: "var(--font-mono, ui-monospace, monospace)",
            fontSize: "var(--text-xs)",
            color: "var(--ink)",
            whiteSpace: "nowrap",
            overflow: "hidden",
            textOverflow: "ellipsis",
          }}
          title={share.url}
        >
          {prettyUrl(share.url)}
        </code>
        <div style={{ marginTop: 2, fontSize: 11, color: "var(--muted)", display: "flex", gap: 9 }}>
          <span>{share.permissions === "view" ? "View" : share.permissions}</span>
          {share.has_password && <span>· password</span>}
          {share.expires_at && <span>· expires {fmtDate(share.expires_at)}</span>}
          <span>· {share.access_count} {share.access_count === 1 ? "open" : "opens"}</span>
        </div>
      </div>
      <button
        type="button"
        aria-label="Copy link"
        onClick={() => {
          void navigator.clipboard?.writeText(share.url);
          setCopied(true);
          setTimeout(() => setCopied(false), 1400);
        }}
        style={ghostIcon()}
      >
        {copied ? <Check size={14} strokeWidth={2.2} style={{ color: "var(--success)" }} /> : <CopyIcon size={14} />}
      </button>
      <button type="button" aria-label="Revoke link" onClick={onRevoke} style={ghostIcon(true)}>
        <Trash2 size={14} />
      </button>
    </li>
  );
}

// ── tiny primitives ────────────────────────────────────────────────────

function Label({ children, style }: { children: React.ReactNode; style?: React.CSSProperties }) {
  return (
    <div
      style={{
        fontSize: 10,
        letterSpacing: "2px",
        textTransform: "uppercase",
        color: "var(--muted-2)",
        fontWeight: 600,
        ...style,
      }}
    >
      {children}
    </div>
  );
}

function Chip({ active, onClick, children }: { active: boolean; onClick: () => void; children: React.ReactNode }) {
  return (
    <button
      type="button"
      onClick={onClick}
      style={{
        display: "inline-flex",
        alignItems: "center",
        gap: 5,
        padding: "7px 11px",
        borderRadius: 9,
        border: `1px solid ${active ? "var(--ink)" : "var(--line)"}`,
        background: active ? "var(--ink)" : "var(--paper)",
        color: active ? "var(--paper)" : "var(--ink)",
        fontFamily: "var(--font-sans)",
        fontSize: "var(--text-sm)",
        fontWeight: 500,
        cursor: "pointer",
        transition: "background 150ms, border-color 150ms",
      }}
    >
      {children}
    </button>
  );
}

function inputStyle(): React.CSSProperties {
  return {
    width: "100%",
    padding: "10px 36px 10px 12px",
    fontFamily: "var(--font-sans)",
    fontSize: "var(--text-md)",
    color: "var(--ink)",
    background: "var(--paper)",
    border: "1px solid var(--line)",
    borderRadius: 10,
    outline: "none",
  };
}

function iconBtn(): React.CSSProperties {
  return {
    background: "transparent",
    border: "none",
    cursor: "pointer",
    color: "var(--muted)",
    padding: 6,
    borderRadius: 8,
  };
}

function ghostIcon(danger?: boolean): React.CSSProperties {
  return {
    background: "transparent",
    border: "none",
    cursor: "pointer",
    color: danger ? "var(--danger)" : "var(--muted)",
    padding: 6,
    borderRadius: 8,
  };
}

function inlineErr(): React.CSSProperties {
  return {
    marginTop: 10,
    padding: "8px 10px",
    background: "rgba(176,69,69,.06)",
    border: "1px solid rgba(176,69,69,.25)",
    borderRadius: 9,
    fontSize: "var(--text-xs)",
    color: "var(--danger)",
  };
}

function Skeleton() {
  return (
    <div
      style={{
        height: 56,
        borderRadius: 11,
        background: "linear-gradient(90deg, var(--bg-subtle), var(--card) 40%, var(--bg-subtle))",
        backgroundSize: "200% 100%",
        animation: "cd-skeleton 1.4s linear infinite",
      }}
    />
  );
}

function prettyUrl(u: string): string {
  return u.replace(/^https?:\/\//, "");
}

function fmtDate(iso: string): string {
  const d = new Date(iso);
  return Number.isNaN(d.getTime())
    ? iso
    : d.toLocaleDateString(undefined, { month: "short", day: "numeric" });
}
