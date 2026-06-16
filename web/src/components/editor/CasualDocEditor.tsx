/**
 * CasualDocEditor — Drive's mount for `.docx` files via the iframe
 * variant `<CasualEditorIframe>` from `@schnsrw/docx-js-editor@>=1.1.0`.
 *
 * Why the iframe variant (not the direct mount):
 *   - CSS isolation. Univer's design tokens + the docx editor's CSS
 *     no longer leak into Drive's tree.
 *   - React-runtime isolation. The SDK's React 19 instance ran into
 *     `LocaleService: Locale not initialized` when mounted alongside
 *     Drive's React tree — that crash goes away with the iframe.
 *   - `viewMode='preview'` hides the toolbar inside the iframe so the
 *     Preview modal renders JUST the rendered document canvas.
 *     `viewMode='editor'` shows the full toolbar for `/file/<id>`.
 *
 * The iframe is same-origin: its `src` resolves under Drive's own
 * domain (`${BASE_URL}embed/docs/embed.html?...`) — the embed runtime
 * is copied from `@schnsrw/docx-js-editor/embed/*` into Drive's
 * `public/embed/docs/` by `scripts/copy-embed.mjs` at prebuild time.
 */

import { useMemo, useRef } from "react";

import { CasualEditorIframe } from "@schnsrw/docx-js-editor";

import { type FileDto } from "../../api/client.ts";
import { DriveFileSource } from "../../file-source/DriveFileSource.ts";
import { withSaveStatus, type OnSaveStatus } from "./save-status.ts";

export interface CasualDocEditorProps {
  file: FileDto;
  /** `preview` = no toolbar, just canvas (modal mount). `editor` =
   *  full editor chrome (fullscreen route). */
  mode?: "preview" | "editor";
  /** Optional callback that fires on every save attempt. Drives the
   *  "Saving… / Saved / Failed" pill in `<FileFullscreen>`. */
  onSaveStatus?: OnSaveStatus;
  /** Fires when the iframe surfaces a parse / load / boot failure.
   *  Drive's PreviewStage swaps the iframe for a friendly fallback
   *  card so users never see the SDK's raw error UI. */
  onError?: (data: {
    code: "embed_not_served" | "load_failed" | "parse_failed" | "boot_failed" | "internal";
    message: string;
  }) => void;
}

export function CasualDocEditor({
  file,
  mode = "preview",
  onSaveStatus,
  onError,
}: CasualDocEditorProps) {
  // Latch the callback so the wrapped source isn't recreated on every
  // host render (the host can swap the function freely).
  const onSaveStatusRef = useRef(onSaveStatus);
  onSaveStatusRef.current = onSaveStatus;

  const fileSource = useMemo(() => {
    const fs = new DriveFileSource(file);
    // Patch save() so every save transition runs through the status
    // tracker. `bind` so the method keeps its `this` context inside
    // DriveFileSource (it touches `this.file`).
    const originalSave = fs.save.bind(fs);
    fs.save = withSaveStatus(originalSave, (s) => onSaveStatusRef.current?.(s));
    return fs;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [file.id]);

  const embedBasePath = `${import.meta.env.BASE_URL}embed/docs`;

  return (
    <CasualEditorIframe
      fileSource={fileSource}
      docId={file.id}
      viewMode={mode}
      embedBasePath={embedBasePath}
      testId="casual-doc-editor"
      onError={onError}
    />
  );
}
