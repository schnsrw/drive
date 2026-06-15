/**
 * File preview modal — Radix Dialog backed. Two-column layout (preview stage
 * + detail sidebar). Type-aware primary action (Open in Sheets / Editor /
 * Download). Keyboard: Esc closes, ←/→ navigates.
 *
 * v0 doesn't render inline previews for binary types — the stage shows the
 * file's procedural thumbnail at large size. Phase-2 wires real PDF.js /
 * image / video / text rendering.
 */
import { lazy, Suspense, useEffect, useState } from "react";
import * as Dialog from "@radix-ui/react-dialog";
import { ChevronLeft, ChevronRight, Download, Share2, Star, X } from "lucide-react";

import type { UseFileSourceAutoSaveReturn } from "@schnsrw/docx-js-editor";

import { downloadUrl, type FileDto } from "../api/client.ts";
import { useReportViewing } from "../state/PresenceContext.tsx";
import { FileThumb, inferKind } from "./FileThumb.tsx";
import { PreviewStage } from "./preview/PreviewStage.tsx";

// AutosaveStatus is lazy — the SDK's vendor bundle (which contains a
// React.Activity assignment that crashes module-init on React 19) must
// never load at app boot. Suspense fallback is null because the dot
// is decorative chrome only shown when a .docx is open.
const AutosaveStatus = lazy(() =>
  import("@schnsrw/docx-js-editor").then((m) => ({ default: m.AutosaveStatus })),
);

export function PreviewModal({
  files,
  index,
  open,
  onClose,
  onChangeIndex,
}: {
  files: FileDto[];
  index: number;
  open: boolean;
  onClose: () => void;
  onChangeIndex: (i: number) => void;
}) {
  const file = files[index];
  const hasNav = files.length > 1;

  /** Navigate to `/file/<id>` for the in-Drive fullscreen editor.
   *  ED1 gap (a). Pushes the FileDto into `history.state` so
   *  FileFullscreen can mount without an extra metadata round trip
   *  (Drive has no `GET /api/files/{id}` endpoint yet). Closes the
   *  modal first so the back-stack reads cleanly. */
  const openInFullscreen = (target: FileDto) => {
    onClose();
    const url = `/file/${encodeURIComponent(target.id)}`;
    window.history.pushState({ file: target }, "", url);
    window.dispatchEvent(new PopStateEvent("popstate"));
  };
  // Autosave state bubbled up from CasualDocEditor when a .docx is in
  // view. Stays null for every other stage; the indicator collapses to
  // nothing in that case (AutosaveStatus already renders null on the
  // idle/never-saved state).
  const [autosaveState, setAutosaveState] = useState<UseFileSourceAutoSaveReturn | null>(
    null,
  );
  // Reset when the focused file changes — peer files might not be docs,
  // and stale state from the previous file would lie to the user.
  useEffect(() => {
    setAutosaveState(null);
  }, [files[index]?.id]);

  // RT3 — announce the focused file to peers' presence streams so
  // their file rows light up with the viewing dot. Pin updates on
  // ←/→ navigation between peer files; clears when the modal closes.
  const reportViewing = useReportViewing();
  const focusedId = open ? files[index]?.id ?? null : null;
  useEffect(() => {
    reportViewing(focusedId);
    return () => {
      if (focusedId) reportViewing(null);
    };
  }, [focusedId, reportViewing]);

  // ←/→ keyboard nav while open
  useEffect(() => {
    if (!open) return;
    function onKey(e: KeyboardEvent) {
      if (e.key === "ArrowLeft" && hasNav) {
        onChangeIndex((index - 1 + files.length) % files.length);
      } else if (e.key === "ArrowRight" && hasNav) {
        onChangeIndex((index + 1) % files.length);
      }
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, hasNav, index, files.length, onChangeIndex]);

  if (!file) return null;

  const kind = inferKind(file.name, file.content_type);
  const typeLabel = labelForKind(kind);
  const primary = primaryAction(kind, file, openInFullscreen);

  return (
    <Dialog.Root open={open} onOpenChange={(o) => !o && onClose()}>
      <Dialog.Portal>
        <Dialog.Overlay
          style={{
            position: "fixed",
            inset: 0,
            background: "var(--bg-overlay)",
            backdropFilter: "blur(6px)",
            WebkitBackdropFilter: "blur(6px)",
            zIndex: "var(--z-modal)" as unknown as number,
            animation: "cd-fade-in 280ms var(--ease)",
          }}
        />
        <Dialog.Content
          aria-describedby={undefined}
          style={{
            position: "fixed",
            top: "50%",
            left: "50%",
            transform: "translate(-50%, -50%)",
            width: "min(1000px, calc(100% - 60px))",
            height: "min(640px, 90vh)",
            background: "var(--card)",
            borderRadius: 24,
            overflow: "hidden",
            display: "grid",
            gridTemplateColumns: "1fr 320px",
            boxShadow: "var(--shadow-xl)",
            zIndex: "var(--z-modal)" as unknown as number,
            animation: "cd-modal-in 320ms var(--ease)",
          }}
        >
          <Dialog.Title style={{ position: "absolute", left: -9999 }}>
            {file.name}
          </Dialog.Title>

          {/* Stage */}
          <div
            style={{
              position: "relative",
              overflow: "hidden",
              background: "var(--bg-subtle)",
            }}
          >
            <PreviewStage file={file} kind={kind} onAutosaveState={setAutosaveState} />

            {autosaveState && (
              <div
                style={{
                  position: "absolute",
                  top: 10,
                  right: 12,
                  zIndex: 2,
                  fontSize: "var(--text-xs)",
                  color: "var(--ink-soft)",
                  background: "rgba(255,255,255,0.85)",
                  padding: "3px 9px",
                  borderRadius: 8,
                  pointerEvents: "none",
                  backdropFilter: "blur(4px)",
                }}
              >
                <Suspense fallback={null}>
                  <AutosaveStatus state={autosaveState} />
                </Suspense>
              </div>
            )}

            {hasNav && (
              <>
                <NavArrow
                  side="prev"
                  onClick={() => onChangeIndex((index - 1 + files.length) % files.length)}
                />
                <NavArrow
                  side="next"
                  onClick={() => onChangeIndex((index + 1) % files.length)}
                />
              </>
            )}
          </div>

          {/* Side */}
          <aside
            style={{
              padding: "24px 26px 22px",
              display: "flex",
              flexDirection: "column",
              borderLeft: "1px solid var(--line)",
              overflowY: "auto",
            }}
          >
            <Dialog.Close asChild>
              <button
                type="button"
                aria-label="Close"
                style={{
                  alignSelf: "flex-end",
                  background: "transparent",
                  border: "none",
                  cursor: "pointer",
                  color: "var(--muted)",
                  padding: 4,
                  borderRadius: 8,
                }}
                onMouseOver={(e) => (e.currentTarget.style.background = "var(--bg-hover)")}
                onMouseOut={(e) => (e.currentTarget.style.background = "transparent")}
              >
                <X size={18} />
              </button>
            </Dialog.Close>

            <div style={{ display: "flex", alignItems: "center", gap: 11, margin: "6px 0 4px" }}>
              <span
                style={{
                  width: 22,
                  height: 22,
                  borderRadius: 6,
                  overflow: "hidden",
                  flexShrink: 0,
                }}
              >
                <FileThumb name={file.name} kind={kind} size="small" thumbnail={file.thumbnail} />
              </span>
              <h3
                style={{
                  fontFamily: "var(--font-display)",
                  fontSize: "var(--text-xl)",
                  fontWeight: 500,
                  letterSpacing: "var(--tracking-tight)",
                  wordBreak: "break-word",
                  margin: 0,
                }}
              >
                {file.name}
              </h3>
            </div>
            <div style={{ fontSize: "var(--text-xs)", color: "var(--muted)", marginBottom: 20 }}>
              {typeLabel}
              {file.size > 0 && ` · ${formatBytes(file.size)}`}
            </div>

            {/* Actions */}
            <div style={{ display: "flex", gap: 8, marginBottom: 24 }}>
              <ActionButton primary onClick={primary.onClick}>
                <primary.Icon size={15} strokeWidth={2} />
                {primary.label}
              </ActionButton>
              <ActionButton onClick={() => navigator.clipboard?.writeText(window.location.href)}>
                <Share2 size={15} strokeWidth={2} />
                Share
              </ActionButton>
              <ActionButton icon onClick={() => {}}>
                <Star size={15} strokeWidth={1.8} />
              </ActionButton>
            </div>

            <SectionLabel>Details</SectionLabel>
            <Detail k="Type" v={typeLabel} />
            <Detail k="Size" v={file.size > 0 ? formatBytes(file.size) : "—"} />
            <Detail k="Modified" v={new Date(file.modified_at).toLocaleString()} />
            <Detail k="Location" v="My Drive" />
          </aside>
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

function NavArrow({ side, onClick }: { side: "prev" | "next"; onClick: () => void }) {
  return (
    <button
      type="button"
      aria-label={side === "prev" ? "Previous file" : "Next file"}
      onClick={onClick}
      style={{
        position: "absolute",
        top: "50%",
        transform: "translateY(-50%)",
        [side === "prev" ? "left" : "right"]: 16,
        width: 38,
        height: 38,
        borderRadius: "50%",
        background: "var(--card)",
        border: "1px solid var(--line-strong)",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        cursor: "pointer",
        color: "var(--ink)",
        transition: "transform 200ms var(--ease), box-shadow 200ms",
      } as React.CSSProperties}
      onMouseOver={(e) => {
        e.currentTarget.style.transform = "translateY(-50%) scale(1.07)";
        e.currentTarget.style.boxShadow = "var(--shadow)";
      }}
      onMouseOut={(e) => {
        e.currentTarget.style.transform = "translateY(-50%)";
        e.currentTarget.style.boxShadow = "";
      }}
    >
      {side === "prev" ? <ChevronLeft size={16} /> : <ChevronRight size={16} />}
    </button>
  );
}

function ActionButton({
  children,
  onClick,
  primary,
  icon,
}: {
  children: React.ReactNode;
  onClick?: () => void;
  primary?: boolean;
  icon?: boolean;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      style={{
        flex: icon ? "0 0 42px" : primary ? 1.4 : 1,
        border: `1px solid ${primary ? "var(--ink)" : "var(--line)"}`,
        background: primary ? "var(--ink)" : "var(--paper)",
        color: primary ? "var(--paper)" : "var(--ink)",
        cursor: "pointer",
        padding: 10,
        borderRadius: 11,
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        gap: 7,
        fontFamily: "var(--font-sans)",
        fontSize: "var(--text-sm)",
        fontWeight: 500,
        transition: "background 150ms, transform 150ms, border-color 150ms",
      }}
      onMouseOver={(e) => {
        if (primary) {
          e.currentTarget.style.background = "#000";
          e.currentTarget.style.transform = "translateY(-1px)";
        } else {
          e.currentTarget.style.background = "var(--bg-hover)";
          e.currentTarget.style.borderColor = "var(--line-strong)";
        }
      }}
      onMouseOut={(e) => {
        e.currentTarget.style.background = primary ? "var(--ink)" : "var(--paper)";
        e.currentTarget.style.transform = "";
        e.currentTarget.style.borderColor = primary ? "var(--ink)" : "var(--line)";
      }}
    >
      {children}
    </button>
  );
}

function SectionLabel({ children }: { children: React.ReactNode }) {
  return (
    <div
      style={{
        fontSize: 10,
        letterSpacing: "2px",
        textTransform: "uppercase",
        color: "var(--muted-2)",
        fontWeight: 600,
        marginBottom: 12,
      }}
    >
      {children}
    </div>
  );
}

function Detail({ k, v }: { k: string; v: string }) {
  return (
    <div
      style={{
        display: "flex",
        justifyContent: "space-between",
        alignItems: "center",
        padding: "10px 0",
        borderBottom: "1px solid var(--line)",
        fontSize: "var(--text-sm)",
      }}
    >
      <span style={{ color: "var(--muted)" }}>{k}</span>
      <span style={{ fontWeight: 500, textAlign: "right" }}>{v}</span>
    </div>
  );
}

function primaryAction(
  kind: ReturnType<typeof inferKind>,
  file: FileDto,
  openInFullscreen: (file: FileDto) => void,
) {
  switch (kind) {
    case "sheet":
      // ED1 gap (a) — primary "open" now lands on Drive's in-app
      // `/file/<id>` route with the full Casual Sheets chrome
      // (toolbar / header / footer). The previous WOPI new-tab
      // handoff (`handoffToEditor`) survives as a fallback when an
      // operator wants the third-party path — kept in this file
      // for that wiring; not surfaced by default.
      return {
        label: "Open in editor",
        Icon: Download,
        onClick: () => openInFullscreen(file),
      };
    case "doc":
      return {
        label: "Open in editor",
        Icon: Download,
        onClick: () => openInFullscreen(file),
      };
    default:
      return {
        label: "Download",
        Icon: Download,
        onClick: () => window.location.assign(downloadUrl(file.id)),
      };
  }
}

// `handoffToEditor` (WOPI new-tab path via `openInEditor`) used to be
// the primary action for `.docx` / `.xlsx`. Replaced by the in-Drive
// fullscreen route in ED1 gap (a) — `openInFullscreen` above. The
// WOPI path stays in `crates/drive-wopi` for third-party / cross-
// origin clients; Drive's own SPA no longer reaches for it.

function labelForKind(k: ReturnType<typeof inferKind>): string {
  switch (k) {
    case "fold":
      return "Folder";
    case "doc":
      return "Document";
    case "sheet":
      return "Spreadsheet";
    case "pdf":
      return "PDF";
    case "img":
      return "Image";
    case "vid":
      return "Video";
    case "aud":
      return "Audio";
    case "md":
      return "Markdown";
    case "text":
      return "Text";
    default:
      return "File";
  }
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
