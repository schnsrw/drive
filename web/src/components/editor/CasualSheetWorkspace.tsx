/**
 * CasualSheetWorkspace — Drive's mount for `.xlsx` files via the iframe
 * variant `<CasualSheetsIframe>` from `@schnsrw/casual-sheets@>=0.5.0`.
 *
 * Same rationale as `CasualDocEditor`:
 *   - CSS isolation (Univer chrome no longer bleeds into Drive's tree).
 *   - React-runtime isolation (kills the `LocaleService` init crash
 *     that the direct mount caused in 0.4.x).
 *   - `viewMode='preview'` hides the toolbar / header / footer; the
 *     iframe renders JUST the grid canvas — the actual UX the Preview
 *     modal needs.
 *
 * `embedBasePath` resolves under Drive's own origin via Vite's
 * BASE_URL so the iframe loads from `${BASE_URL}embed/sheets/embed.html`.
 * The embed runtime is copied from
 * `@schnsrw/casual-sheets/embed/*` into `public/embed/sheets/` by
 * `scripts/copy-embed.mjs` at prebuild time.
 */

import { useEffect, useMemo, useRef, useState } from "react";

import {
  CasualSheetsIframe,
  type CasualSheetsIframeRef,
  type HostFileBridge,
} from "@schnsrw/casual-sheets/sheets";

import { type FileDto } from "../../api/client.ts";
import { DriveFileSource } from "../../file-source/DriveFileSource.ts";
import { withSaveStatus, type OnSaveStatus } from "./save-status.ts";
import { SheetToolbar, type SheetFormatState } from "./SheetToolbar.tsx";

export interface IframeErrorData {
  code: "embed_not_served" | "load_failed" | "parse_failed" | "boot_failed" | "internal";
  message: string;
}

export interface CasualSheetWorkspaceProps {
  file: FileDto;
  /** `preview` = no toolbar, just canvas (modal mount). `editor` =
   *  full Office chrome (fullscreen route). */
  mode?: "preview" | "editor";
  /** Optional callback that fires on every save attempt. Drives the
   *  "Saving… / Saved / Failed" pill in `<FileFullscreen>`. */
  onSaveStatus?: OnSaveStatus;
  /** Fires when the iframe surfaces a parse / load / boot failure.
   *  Drive's PreviewStage swaps the iframe for a friendly fallback
   *  card so users never see the SDK's raw error UI. */
  onError?: (data: IframeErrorData) => void;
}

const IDLE_FORMAT_STATE: SheetFormatState = {
  bold: false,
  italic: false,
  underline: false,
  strikethrough: false,
  align: null,
};

export function CasualSheetWorkspace({
  file,
  mode = "preview",
  onSaveStatus,
  onError,
}: CasualSheetWorkspaceProps) {
  // Latch the callback so the bridge memo doesn't churn when the
  // host re-renders for an unrelated reason. The host can swap the
  // function freely — the bridge always invokes the current one.
  const onSaveStatusRef = useRef(onSaveStatus);
  onSaveStatusRef.current = onSaveStatus;

  // `CasualSheetsIframe` wants the smaller `HostFileBridge` shape, not
  // the full FileSource interface. Adapt our DriveFileSource here.
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

  // Drive-side toolbar + format state — sheet 0.6+ wire.
  const iframeRef = useRef<CasualSheetsIframeRef | null>(null);
  const [formatState, setFormatState] = useState<SheetFormatState>(IDLE_FORMAT_STATE);

  // Reset format state when the workbook changes so a stale flag
  // doesn't ride over into the new file.
  useEffect(() => {
    setFormatState(IDLE_FORMAT_STATE);
  }, [file.id]);

  if (mode === "preview") {
    return (
      <CasualSheetsIframe
        ref={iframeRef}
        fileSource={bridge}
        docId={file.id}
        viewMode={mode}
        embedBasePath={embedBasePath}
        testId="casual-sheet-workspace"
        onError={onError}
      />
    );
  }

  return (
    <div style={{ display: "flex", flexDirection: "column", height: "100%", minHeight: 0 }}>
      <SheetToolbar iframeRef={iframeRef} formatState={formatState} />
      <div style={{ flex: 1, minHeight: 0 }}>
        <CasualSheetsIframe
          ref={iframeRef}
          fileSource={bridge}
          docId={file.id}
          viewMode={mode}
          embedBasePath={embedBasePath}
          testId="casual-sheet-workspace"
          onError={onError}
          onSelectionFormatState={setFormatState}
        />
      </div>
    </div>
  );
}
