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

import { useMemo, useRef } from "react";

import { CasualSheetsIframe, type HostFileBridge } from "@schnsrw/casual-sheets/sheets";

import { type FileDto } from "../../api/client.ts";
import { DriveFileSource } from "../../file-source/DriveFileSource.ts";
import { withSaveStatus, type OnSaveStatus } from "./save-status.ts";

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

  return (
    <CasualSheetsIframe
      fileSource={bridge}
      docId={file.id}
      viewMode={mode}
      embedBasePath={embedBasePath}
      testId="casual-sheet-workspace"
      onError={onError}
    />
  );
}
