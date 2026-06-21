/**
 * CasualSheetWorkspace — Drive's mount for `.xlsx` files. Drive is a HOST
 * that embeds the Casual Sheets editor in a sandboxed `<iframe>` (via
 * `<SheetEmbed>`), NOT a direct Univer mount. Univer runs inside the
 * iframe's own runtime; Drive's app bundle stays free of `@univerjs/*`.
 *
 * Two surfaces, one component:
 *   - `mode="preview"` (Preview modal): iframe in `viewMode="preview"`,
 *     which the SDK enforces as READ-ONLY (no editing, no chrome). Drive
 *     adds no toolbar — it's a viewer.
 *   - `mode="editor"` (fullscreen `/file/<id>` route): iframe in
 *     `viewMode="editor"` — the SDK renders the FULL editor chrome inside
 *     the iframe (menu bar, formatting toolbar, formula bar, sheet tabs,
 *     status bar). Drive does NOT hand-roll a toolbar; it only frames the
 *     editor. The package IS the editor (Excalidraw model).
 *
 * Persistence (host stores, SDK never does): load/save bytes round-trip
 * through `<SheetEmbed>`'s `HostFileBridge`, backed by `DriveFileSource`
 * (cookie + CSRF `/api/files/{id}/content`, WOPI). Drive's auth/file/WOPI
 * domain logic is untouched — only the editor-embedding layer is here.
 *
 * `embedBasePath` resolves under Drive's own origin via Vite's BASE_URL,
 * so the iframe loads `${BASE_URL}embed/sheets/embed.html`. That runtime
 * is copied from `@casualoffice/sheets/embed/*` into `public/embed/sheets/`
 * by `scripts/copy-embed.mjs` at prebuild time.
 */

import { useMemo, useRef } from "react";

import { type FileDto } from "../../api/client.ts";
import { DriveFileSource } from "../../file-source/DriveFileSource.ts";
import { withSaveStatus, type OnSaveStatus } from "./save-status.ts";
import {
  SheetEmbed,
  type HostFileBridge,
  type SheetEmbedError,
  type SheetEmbedRef,
} from "./SheetEmbed.tsx";

/** Re-export so existing host call sites that referenced the workspace's
 *  error type keep compiling. */
export type IframeErrorData = SheetEmbedError;

export interface CasualSheetWorkspaceProps {
  file: FileDto;
  /** `preview` = read-only viewer (modal mount). `editor` = full editing;
   *  the SDK renders its own chrome inside the iframe (fullscreen route). */
  mode?: "preview" | "editor";
  /** Fires on every save attempt. Drives the "Saving… / Saved / Failed"
   *  pill in `<FileFullscreen>`. */
  onSaveStatus?: OnSaveStatus;
  /** Fires when the iframe surfaces a load / parse / boot failure so
   *  Drive's PreviewStage can swap in a friendly fallback card. */
  onError?: (data: IframeErrorData) => void;
}

export function CasualSheetWorkspace({
  file,
  mode = "preview",
  onSaveStatus,
  onError,
}: CasualSheetWorkspaceProps) {
  // Latch the callback so the bridge memo doesn't churn when the host
  // re-renders for an unrelated reason. The host can swap the function
  // freely — the bridge always invokes the current one.
  const onSaveStatusRef = useRef(onSaveStatus);
  onSaveStatusRef.current = onSaveStatus;

  // `<SheetEmbed>` wants the smaller `HostFileBridge` shape, not the full
  // FileSource interface. Adapt our DriveFileSource here. Save() is wrapped
  // so each attempt announces transitions to the host's save-status pill.
  const bridge = useMemo<HostFileBridge>(() => {
    const fs = new DriveFileSource(file);
    const rawSave = async (docId: string, bytes: ArrayBuffer, opts?: { etag?: string }) => {
      const result = await fs.save(docId, bytes, opts);
      return { etag: result.etag };
    };
    return {
      open: (docId) => fs.open(docId),
      save: withSaveStatus(rawSave, (s) => onSaveStatusRef.current?.(s)),
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [file.id]);

  const embedBasePath = `${import.meta.env.BASE_URL}embed/sheets`;

  const iframeRef = useRef<SheetEmbedRef | null>(null);

  if (mode === "preview") {
    // Read-only viewer — the SDK enforces read-only for viewMode=preview.
    return (
      <SheetEmbed
        ref={iframeRef}
        fileSource={bridge}
        docId={file.id}
        viewMode="preview"
        embedBasePath={embedBasePath}
        testId="casual-sheet-workspace"
        onError={onError}
      />
    );
  }

  // Full editor — the SDK renders the complete chrome inside the iframe;
  // Drive only frames it (no hand-rolled toolbar).
  return (
    <SheetEmbed
      ref={iframeRef}
      fileSource={bridge}
      docId={file.id}
      viewMode="editor"
      embedBasePath={embedBasePath}
      testId="casual-sheet-workspace"
      onError={onError}
    />
  );
}
