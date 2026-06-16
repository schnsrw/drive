/**
 * Per-type stage renderer for the Preview Modal. Spec:
 * docs/ux/07-preview-surface.md.
 *
 * Picks the right primitive for the file kind:
 *   - img  → <img>
 *   - pdf  → <iframe> (browser-native viewer)
 *   - vid  → <video>
 *   - aud  → <audio>
 *   - text → <pre> after a capped text fetch
 *   - md   → marked + DOMPurify-sanitised HTML
 *   - doc / sheet / fold / generic → procedural thumbnail (handoff target)
 *
 * All bytes come from the file's existing downloadUrl which 302s to the
 * signed URL on the user-content origin.
 */
import { lazy, Suspense, useEffect, useRef, useState } from "react";
import DOMPurify from "dompurify";
import { marked } from "marked";

import type { UseFileSourceAutoSaveReturn } from "@schnsrw/docx-js-editor";

import { downloadUrl, type FileDto } from "../../api/client.ts";
import { FileThumb, inferKind, type FileKind } from "../FileThumb.tsx";

// CasualDocEditor + CasualSheetWorkspace pull the editor SDK + the Univer
// peer set (collectively ~2.5 MB minified). Defer them behind React.lazy so
// the vendor chunk only downloads when a user actually clicks into a
// .docx / .xlsx preview — Drive's cold-load stays small.
const CasualDocEditor = lazy(() =>
  import("../editor/CasualDocEditor.tsx").then((m) => ({ default: m.CasualDocEditor })),
);
const CasualSheetWorkspace = lazy(() =>
  import("../editor/CasualSheetWorkspace.tsx").then((m) => ({
    default: m.CasualSheetWorkspace,
  })),
);
// vidstack ships ~120 KB of player chrome + ~30 KB of default-layout
// CSS. Only video / audio previews pay this cost.
const DrivenMediaPlayer = lazy(() =>
  import("./MediaPlayer.tsx").then((m) => ({ default: m.DrivenMediaPlayer })),
);

const TEXT_CAP_BYTES = 512 * 1024; // 512 KB
const MD_CAP_BYTES = 256 * 1024; // 256 KB

export interface PreviewStageProps {
  file: FileDto;
  kind: FileKind;
  /** Carried for API stability — autosave state now lives INSIDE the
   *  embed iframe (the SDK's own chrome shows "Saving…"), so the
   *  modal no longer subscribes. Drop this prop in the next major. */
  onAutosaveState?: (state: UseFileSourceAutoSaveReturn) => void;
}

export function PreviewStage({ file, kind }: PreviewStageProps) {
  switch (kind) {
    case "img":
      return <ImageStage file={file} />;
    case "pdf":
      return <PdfStage file={file} />;
    case "vid":
      return <VideoStage file={file} />;
    case "aud":
      return <AudioStage file={file} />;
    case "text":
      return <TextStage file={file} cap={TEXT_CAP_BYTES} />;
    case "md":
      return <MarkdownStage file={file} />;
    case "doc":
      return (
        <Suspense fallback={<EditorLoading />}>
          {/* mode='preview' hides the toolbar inside the iframe so the
              modal stage renders JUST the document canvas. */}
          <ErrorAwareDoc file={file} />
        </Suspense>
      );
    case "sheet":
      return (
        <Suspense fallback={<EditorLoading />}>
          <ErrorAwareSheet file={file} />
        </Suspense>
      );
    default:
      return <PlaceholderStage file={file} kind={kind} />;
  }
}

// ── Image ──────────────────────────────────────────────────────────────

function ImageStage({ file }: { file: FileDto }) {
  const [failed, setFailed] = useState(false);
  if (failed) return <FailureFallback file={file} />;
  return (
    <div style={mediaWrap()}>
      <img
        src={downloadUrl(file.id)}
        alt={file.name}
        onError={() => setFailed(true)}
        style={{
          maxWidth: "100%",
          maxHeight: "100%",
          objectFit: "contain",
          borderRadius: 10,
          boxShadow: "0 8px 28px rgba(15, 23, 42,.15)",
          background: "var(--paper)",
        }}
      />
    </div>
  );
}

// ── PDF ────────────────────────────────────────────────────────────────

function PdfStage({ file }: { file: FileDto }) {
  const [failed, setFailed] = useState(false);
  if (failed) return <FailureFallback file={file} />;
  return (
    <div style={{ width: "100%", height: "100%", background: "#fff" }}>
      <iframe
        src={`${downloadUrl(file.id)}#view=FitH`}
        title={file.name}
        onError={() => setFailed(true)}
        style={{ width: "100%", height: "100%", border: "none", display: "block" }}
      />
    </div>
  );
}

// ── Video ──────────────────────────────────────────────────────────────

function VideoStage({ file }: { file: FileDto }) {
  return (
    <div style={mediaWrap()}>
      <Suspense fallback={<MediaLoading kind="video" />}>
        <DrivenMediaPlayer file={file} kind="video" />
      </Suspense>
    </div>
  );
}

// ── Audio ──────────────────────────────────────────────────────────────

function AudioStage({ file }: { file: FileDto }) {
  return (
    <div style={{ ...mediaWrap(), flexDirection: "column", gap: 18 }}>
      <FileThumb name={file.name} kind="aud" size="big" />
      <Suspense fallback={<MediaLoading kind="audio" />}>
        <DrivenMediaPlayer file={file} kind="audio" />
      </Suspense>
    </div>
  );
}

function MediaLoading({ kind }: { kind: "video" | "audio" }) {
  return (
    <div
      role="status"
      aria-label={`Loading ${kind} player`}
      style={{
        width: kind === "video" ? "min(960px, 100%)" : "min(640px, 100%)",
        aspectRatio: kind === "video" ? "16 / 9" : "auto",
        height: kind === "audio" ? 88 : undefined,
        borderRadius: 12,
        background: "var(--bg-subtle)",
        border: "1px solid var(--line)",
      }}
    />
  );
}

// ── Text + Markdown ────────────────────────────────────────────────────

interface TextLoad {
  state: "loading" | "ready" | "error";
  body?: string;
  truncated?: boolean;
  error?: string;
}

function useCappedText(file: FileDto, cap: number): TextLoad {
  const [load, setLoad] = useState<TextLoad>({ state: "loading" });
  const seq = useRef(0);
  useEffect(() => {
    const my = ++seq.current;
    setLoad({ state: "loading" });
    (async () => {
      try {
        const res = await fetch(downloadUrl(file.id), { credentials: "same-origin" });
        if (!res.ok) throw new Error(`HTTP ${res.status}`);
        // For demo-mode blob: URLs we still want to honour the cap.
        const blob = await res.blob();
        const truncated = blob.size > cap;
        const slice = truncated ? blob.slice(0, cap) : blob;
        const text = await slice.text();
        if (seq.current === my) setLoad({ state: "ready", body: text, truncated });
      } catch (e) {
        if (seq.current === my) setLoad({ state: "error", error: (e as Error).message });
      }
    })();
  }, [file.id, cap]);
  return load;
}

function TextStage({ file, cap }: { file: FileDto; cap: number }) {
  const load = useCappedText(file, cap);
  if (load.state === "loading") return <Loading label="Loading preview…" />;
  if (load.state === "error") return <FailureFallback file={file} />;
  return (
    <div style={textWrap()}>
      {load.truncated && <TruncatedBanner cap={cap} />}
      <pre
        style={{
          margin: 0,
          padding: "20px 24px",
          fontFamily: "var(--font-mono, ui-monospace, monospace)",
          fontSize: 13,
          lineHeight: 1.55,
          color: "var(--ink)",
          whiteSpace: "pre-wrap",
          wordBreak: "break-word",
          background: "var(--card)",
          flex: 1,
          overflow: "auto",
        }}
      >
        {load.body}
      </pre>
    </div>
  );
}

function MarkdownStage({ file }: { file: FileDto }) {
  const load = useCappedText(file, MD_CAP_BYTES);
  const [html, setHtml] = useState<string | null>(null);

  useEffect(() => {
    if (load.state !== "ready" || !load.body) return;
    (async () => {
      // marked.parse returns a string in v18; await to support a future
      // async pipeline without breaking the build.
      const raw = await Promise.resolve(marked.parse(load.body!, { gfm: true, breaks: false }));
      // Sanitize. Default DOMPurify config strips iframe/object/embed/form
      // and dangerous attrs by default; we add ADD_ATTR for target/rel so
      // anchor tags can open in a new tab without getting scrubbed.
      const clean = DOMPurify.sanitize(raw, {
        ADD_ATTR: ["target", "rel"],
        FORBID_TAGS: ["iframe", "object", "embed", "form", "style"],
      });
      setHtml(clean);
    })();
  }, [load.state, load.body]);

  if (load.state === "loading") return <Loading label="Loading preview…" />;
  if (load.state === "error") return <FailureFallback file={file} />;
  if (html === null) return <Loading label="Rendering markdown…" />;

  return (
    <div style={textWrap()}>
      {load.truncated && <TruncatedBanner cap={MD_CAP_BYTES} />}
      <div
        className="cd-md"
        style={{
          margin: 0,
          padding: "26px 32px",
          fontFamily: "var(--font-sans)",
          fontSize: "var(--text-md)",
          lineHeight: "var(--leading-normal)",
          color: "var(--ink)",
          background: "var(--card)",
          flex: 1,
          overflow: "auto",
        }}
        // eslint-disable-next-line react/no-danger
        dangerouslySetInnerHTML={{ __html: html }}
      />
      <style>{`
        .cd-md h1, .cd-md h2, .cd-md h3, .cd-md h4 {
          font-family: var(--font-display);
          font-weight: 500;
          letter-spacing: var(--tracking-tight);
          color: var(--ink);
          margin: 1.4em 0 .5em;
          line-height: 1.2;
        }
        .cd-md h1 { font-size: 28px; }
        .cd-md h2 { font-size: 22px; }
        .cd-md h3 { font-size: 18px; }
        .cd-md p  { margin: .7em 0; }
        .cd-md a  { color: var(--ink); text-decoration: underline; text-decoration-thickness: 1px; }
        .cd-md code {
          font-family: var(--font-mono, ui-monospace, monospace);
          background: var(--bg-subtle);
          border: 1px solid var(--line);
          border-radius: 4px;
          padding: 1px 5px;
          font-size: .92em;
        }
        .cd-md pre {
          background: var(--bg-subtle);
          border: 1px solid var(--line);
          border-radius: 10px;
          padding: 12px 14px;
          overflow: auto;
          font-size: 12.5px;
          line-height: 1.55;
        }
        .cd-md pre code { background: transparent; border: 0; padding: 0; }
        .cd-md blockquote {
          margin: 1em 0;
          padding: 4px 14px;
          border-left: 3px solid var(--accent);
          color: var(--ink-soft);
          background: var(--bg-subtle);
          border-radius: 0 8px 8px 0;
        }
        .cd-md ul, .cd-md ol { padding-left: 22px; }
        .cd-md li { margin: .25em 0; }
        .cd-md hr { border: 0; border-top: 1px solid var(--line); margin: 1.6em 0; }
        .cd-md img { max-width: 100%; height: auto; border-radius: 8px; }
        .cd-md table { border-collapse: collapse; width: 100%; margin: 1em 0; }
        .cd-md th, .cd-md td { border: 1px solid var(--line); padding: 6px 10px; text-align: left; }
        .cd-md th { background: var(--bg-subtle); }
      `}</style>
    </div>
  );
}

// ── Doc / sheet / folder / generic — handoff stage ────────────────────

function PlaceholderStage({ file, kind }: { file: FileDto; kind: FileKind }) {
  return (
    <div style={mediaWrap()}>
      <div
        style={{
          width: "min(340px, 74%)",
          aspectRatio: kind === "fold" ? "1 / 1" : "1 / 1.3",
          borderRadius: 10,
          overflow: "hidden",
          boxShadow: "0 10px 40px rgba(15, 23, 42,.2)",
        }}
      >
        <FileThumb name={file.name} kind={kind} size="big" thumbnail={file.thumbnail} />
      </div>
    </div>
  );
}

// ── tiny shared primitives ─────────────────────────────────────────────

function mediaWrap(): React.CSSProperties {
  return {
    width: "100%",
    height: "100%",
    display: "flex",
    alignItems: "center",
    justifyContent: "center",
    padding: 24,
    background: "var(--bg-subtle)",
  };
}

function textWrap(): React.CSSProperties {
  return {
    width: "100%",
    height: "100%",
    display: "flex",
    flexDirection: "column",
    background: "var(--card)",
  };
}

function Loading({ label }: { label: string }) {
  return (
    <div
      style={{
        ...mediaWrap(),
        flexDirection: "column",
        gap: 10,
        color: "var(--muted)",
        fontSize: "var(--text-sm)",
      }}
    >
      <div
        style={{
          width: 22,
          height: 22,
          border: "2px solid var(--line-strong)",
          borderTopColor: "var(--ink)",
          borderRadius: "50%",
          animation: "cd-spin 900ms linear infinite",
        }}
      />
      <span>{label}</span>
      <style>{`@keyframes cd-spin { to { transform: rotate(360deg); } }`}</style>
    </div>
  );
}

/** Wraps the doc editor iframe with a Drive-side error state. When the
 *  SDK fires `casual.error` for a parse / load / boot failure we render
 *  FailureFallback instead of the iframe so users never see the raw
 *  SDK error UI ("Failed to Load Document", red stack-trace text). */
function ErrorAwareDoc({ file }: { file: FileDto }) {
  const [errored, setErrored] = useState(false);
  if (errored) return <FailureFallback file={file} />;
  return <CasualDocEditor file={file} mode="preview" onError={() => setErrored(true)} />;
}

function ErrorAwareSheet({ file }: { file: FileDto }) {
  const [errored, setErrored] = useState(false);
  if (errored) return <FailureFallback file={file} />;
  return <CasualSheetWorkspace file={file} mode="preview" onError={() => setErrored(true)} />;
}

function FailureFallback({ file }: { file: FileDto }) {
  const k = inferKind(file.name, file.content_type);
  return (
    <div style={{ ...mediaWrap(), flexDirection: "column", gap: 12 }}>
      <div
        style={{
          width: "min(260px, 60%)",
          aspectRatio: "1 / 1.3",
          borderRadius: 10,
          overflow: "hidden",
          boxShadow: "0 8px 28px rgba(15, 23, 42,.15)",
        }}
      >
        <FileThumb name={file.name} kind={k} size="big" />
      </div>
      <span style={{ fontSize: "var(--text-sm)", color: "var(--muted)" }}>
        Couldn&apos;t load preview — try downloading.
      </span>
    </div>
  );
}

function TruncatedBanner({ cap }: { cap: number }) {
  return (
    <div
      style={{
        padding: "8px 16px",
        background: "var(--accent-muted)",
        borderBottom: "1px solid rgba(200,164,92,.32)",
        fontSize: "var(--text-xs)",
        color: "var(--ink-soft)",
      }}
    >
      Showing the first {formatBytes(cap)}. Download the full file for the rest.
    </div>
  );
}

function EditorLoading() {
  return (
    <div
      style={{
        width: "100%",
        height: "100%",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        color: "var(--ink-soft)",
        fontSize: "var(--text-xs)",
      }}
    >
      Loading editor…
    </div>
  );
}

function formatBytes(b: number): string {
  if (b === 0) return "0 B";
  const units = ["B", "KB", "MB", "GB"];
  let v = b;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${i === 0 ? v : v.toFixed(v < 10 ? 1 : 0)} ${units[i]}`;
}
