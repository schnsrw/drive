/**
 * FileFullscreen — the `/file/<id>` route. Drive's in-app editor
 * surface, peer of `/home`, `/notes`, `/activity`. ED1 gap (a) in
 * `PIPELINE.md` — the editor breaks out of the Preview modal's
 * 1000×640 frame into the full viewport.
 *
 * Lifecycle:
 *   1. Mount with `fileId` from the URL.
 *   2. Fetch the FileDto via `GET /api/files/{id}` so we know name +
 *      content_type + version. Bytes are NOT fetched here — the SDK
 *      wrapper's own DriveFileSource handles that.
 *   3. Infer kind via FileThumb's `inferKind`. For `doc` mount
 *      `<CasualDocEditor>`; for `sheet` mount
 *      `<CasualSheetWorkspace mode="editor">`. Anything else falls
 *      through to a "no editor for this format" surface — the user
 *      can still download via the back-to-Drive button.
 *   4. A slim top bar shows the filename + a back arrow. Cmd-K
 *      shortcut is intentionally not bound here — the editor's own
 *      shortcuts own this surface.
 *
 * What this page does NOT do:
 *   - Auth gate — `App.tsx` already wraps Router in `<AuthProvider>`,
 *     so a `/file/<id>` URL on an unauthed visitor bounces them to
 *     `<SignIn />`. The fullscreen route only renders when authed.
 *   - File picker / sidebar — the editor wants the whole viewport.
 *     Use the back arrow (or browser back) to return to `/home`.
 *   - Co-edit toggle — already inherited from `<CasualDocEditor>`
 *     via `VITE_DRIVE_COLLAB_BACKEND_URL`. The wrapper handles it.
 */

import { lazy, Suspense, useEffect, useState } from "react";
import { ArrowLeft } from "lucide-react";

import { getFile, type FileDto } from "../api/client.ts";
import { inferKind } from "../components/FileThumb.tsx";

// Same lazy-load pattern as PreviewStage — both surfaces share the
// same SDK chunks but tax different routes, so the Suspense
// boundary lives per consumer.
const CasualDocEditor = lazy(() =>
  import("../components/editor/CasualDocEditor.tsx").then((m) => ({
    default: m.CasualDocEditor,
  })),
);
const CasualSheetWorkspace = lazy(() =>
  import("../components/editor/CasualSheetWorkspace.tsx").then((m) => ({
    default: m.CasualSheetWorkspace,
  })),
);

type LoadState =
  | { kind: "loading" }
  | { kind: "ready"; file: FileDto }
  | { kind: "error"; message: string };

export interface FileFullscreenProps {
  fileId: string;
}

/** Pull a FileDto out of `history.state` when Files navigated us here.
 *  Avoids the cold `GET /api/files/{id}` round trip on the hot path
 *  (open-from-file-list); we still fetch when state is empty. */
function fileFromHistory(fileId: string): FileDto | null {
  try {
    const st = window.history.state;
    if (st && typeof st === "object" && "file" in st) {
      const f = (st as { file?: FileDto }).file;
      if (f && f.id === fileId) return f;
    }
  } catch {
    /* ignored */
  }
  return null;
}

export function FileFullscreen({ fileId }: FileFullscreenProps) {
  const [state, setState] = useState<LoadState>(() => {
    const seeded = fileFromHistory(fileId);
    if (seeded) return { kind: "ready", file: seeded };
    return { kind: "loading" };
  });

  // Cold-load path. When the user lands here via refresh / shared
  // URL / bookmark, history.state is empty; resolve via the new
  // `GET /api/files/{id}` endpoint. Hot loads (open-from-Files
  // through PreviewModal) skip this entirely because the seeded
  // state above already produced `ready`.
  useEffect(() => {
    if (state.kind !== "loading") return;
    let cancelled = false;
    (async () => {
      try {
        const file = await getFile(fileId);
        if (cancelled) return;
        setState({ kind: "ready", file });
      } catch (err) {
        if (cancelled) return;
        const message = err instanceof Error ? err.message : String(err);
        setState({ kind: "error", message });
      }
    })();
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [fileId]);

  // Track the live filename for the tab title so refresh / share
  // shows the editor target. Cleared on unmount so other pages can
  // own the title.
  useEffect(() => {
    if (state.kind !== "ready") return;
    const prev = document.title;
    document.title = `${state.file.name} — Casual Drive`;
    return () => {
      document.title = prev;
    };
  }, [state]);

  const goBack = () => {
    window.history.pushState({}, "", "/");
    // App.tsx's Router only re-reads on pathname-changing nav. Use
    // popstate to nudge it; React-managed state in Shell follows.
    window.dispatchEvent(new PopStateEvent("popstate"));
  };

  return (
    <div
      data-testid="file-fullscreen"
      data-file-id={fileId}
      style={{
        position: "fixed",
        inset: 0,
        background: "var(--bg)",
        display: "flex",
        flexDirection: "column",
      }}
    >
      <header
        style={{
          flex: "0 0 auto",
          display: "flex",
          alignItems: "center",
          gap: 14,
          padding: "10px 18px",
          borderBottom: "1px solid var(--line)",
          background: "var(--card)",
        }}
      >
        <button
          type="button"
          onClick={goBack}
          aria-label="Back to Drive"
          data-testid="file-fullscreen-back"
          style={{
            padding: 6,
            border: "1px solid var(--line)",
            borderRadius: 8,
            background: "var(--card)",
            cursor: "pointer",
            display: "inline-flex",
          }}
        >
          <ArrowLeft size={16} />
        </button>
        <div
          data-testid="file-fullscreen-title"
          style={{
            fontSize: "var(--text-sm)",
            fontWeight: 600,
            color: "var(--text)",
            overflow: "hidden",
            textOverflow: "ellipsis",
            whiteSpace: "nowrap",
          }}
        >
          {state.kind === "ready" ? state.file.name : "Loading…"}
        </div>
      </header>
      <main style={{ flex: 1, minHeight: 0, position: "relative" }}>
        <FullscreenBody state={state} />
      </main>
    </div>
  );
}

function FullscreenBody({ state }: { state: LoadState }) {
  if (state.kind === "loading") {
    return (
      <div
        data-testid="file-fullscreen-loading"
        style={{
          width: "100%",
          height: "100%",
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          fontSize: "var(--text-sm)",
          color: "var(--text-muted)",
        }}
      >
        Opening file…
      </div>
    );
  }

  if (state.kind === "error") {
    return (
      <div
        data-testid="file-fullscreen-error"
        style={{
          width: "100%",
          height: "100%",
          display: "flex",
          flexDirection: "column",
          alignItems: "center",
          justifyContent: "center",
          gap: 8,
          padding: 24,
          textAlign: "center",
        }}
      >
        <div style={{ fontSize: 14, fontWeight: 600, color: "var(--text)" }}>
          Couldn't open this file
        </div>
        <div style={{ fontSize: 13, color: "var(--text-muted)", maxWidth: 420 }}>
          {state.message}
        </div>
      </div>
    );
  }

  const { file } = state;
  const kind = inferKind(file.name, file.content_type);

  if (kind === "doc") {
    return (
      <Suspense fallback={<LoadingFallback />}>
        <CasualDocEditor file={file} />
      </Suspense>
    );
  }
  if (kind === "sheet") {
    return (
      <Suspense fallback={<LoadingFallback />}>
        <CasualSheetWorkspace file={file} mode="editor" />
      </Suspense>
    );
  }

  return (
    <div
      data-testid="file-fullscreen-unsupported"
      style={{
        width: "100%",
        height: "100%",
        display: "flex",
        flexDirection: "column",
        alignItems: "center",
        justifyContent: "center",
        gap: 8,
        padding: 24,
        textAlign: "center",
      }}
    >
      <div style={{ fontSize: 14, fontWeight: 600, color: "var(--text)" }}>
        No editor for this format
      </div>
      <div style={{ fontSize: 13, color: "var(--text-muted)", maxWidth: 420 }}>
        Casual Drive's in-app editor only opens .docx and .xlsx files. Use the
        download button on the file's preview to grab the bytes.
      </div>
    </div>
  );
}

function LoadingFallback() {
  return (
    <div
      style={{
        width: "100%",
        height: "100%",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        fontSize: "var(--text-sm)",
        color: "var(--text-muted)",
      }}
    >
      Loading editor…
    </div>
  );
}
