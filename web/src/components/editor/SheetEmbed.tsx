/**
 * SheetEmbed — Drive's thin iframe host for the Casual Sheets editor.
 *
 * Drive is a HOST that embeds the editor in a sandboxed `<iframe>`; it does
 * NOT mount Univer directly. This component is deliberately built on the
 * lightweight `@casualoffice/sheets/embed` entry — `EmbedHostTransport` +
 * protocol types only, ZERO `@univerjs/*` imports. Univer runs entirely
 * inside the iframe's own `embed-runtime.js` (served same-origin from
 * `public/embed/sheets/`, copied by `scripts/copy-embed.mjs`).
 *
 * Why not `<CasualSheetsIframe>` from `@casualoffice/sheets/sheets`?
 *   That wrapper does the same job, but it's exported from the `/sheets`
 *   entry whose module statically imports the full Univer plugin set
 *   (`CasualSheets` direct-mount lives in the same module). Importing it
 *   drags ~9.8 MB of Univer into Drive's app bundle even though the host
 *   never runs Univer. Building our own ~120-line wrapper on `/embed`
 *   keeps Drive's bundle free of Univer entirely.
 *
 * viewMode contract (host → editor over postMessage):
 *   - `preview` → READ-ONLY. The SDK embed-runtime enforces read-only and
 *     hides its chrome; Drive does NOT hand-roll a read-only guard.
 *   - `editor`  → full editing. Drive renders its OWN chrome (toolbar +
 *     menu) around this iframe — see `<CasualSheetWorkspace mode="editor">`.
 *
 * Persistence (host stores, SDK never does): load/save bytes round-trip
 * through `HostFileBridge` (backed by `DriveFileSource` → WOPI / cookie +
 * CSRF `/api/files/{id}/content`). The editor requests bytes via
 * `casual.load.request`; Drive answers with xlsx bytes. On Ctrl/Cmd+S or
 * a host save command the editor pushes a snapshot back.
 */

import {
  forwardRef,
  useCallback,
  useEffect,
  useImperativeHandle,
  useRef,
  type CSSProperties,
} from "react";

import {
  EmbedHostTransport,
  type LoadResponseData,
  type SaveResponseData,
  type SelectionChangedData,
} from "@casualoffice/sheets/embed";

// `CommandExecuteData['command']` + `SelectionFormatStateData` are part of
// the SDK embed wire protocol but aren't in the published named-export list
// of any entry in 0.11.1 (an SDK packaging gap — they're referenced by
// `EmbedHostTransport`'s method signatures but not re-exported as named
// types). Mirror the two narrow shapes locally until the SDK exports them.
// These are stable wire contracts (see the SDK's `embed/protocol.ts`).

/** Formatting / navigation commands Drive's toolbar dispatches to the
 *  iframe's active selection via `executeCommand`. */
type SheetCommand =
  | "undo"
  | "redo"
  | "bold"
  | "italic"
  | "underline"
  | "strikethrough"
  | "align-left"
  | "align-center"
  | "align-right"
  | "set-font-family"
  | "set-font-size"
  | "set-text-color"
  | "reset-text-color"
  | "set-bg-color"
  | "reset-bg-color"
  | "merge"
  | "unmerge"
  | "numfmt-currency"
  | "numfmt-percent"
  | "numfmt-add-decimal"
  | "numfmt-subtract-decimal"
  | "numfmt-custom"
  | "wrap-toggle"
  | "freeze-first-row"
  | "freeze-first-column"
  | "freeze-none";

interface SheetCommandArgs {
  family?: string;
  size?: number;
  color?: string;
  pattern?: string;
}

/** Active-cell format read-back the editor emits so Drive's toolbar can
 *  reflect "pressed" / value state. Mirrors the SDK's
 *  `SelectionFormatStateData`. */
export interface SelectionFormatStateData {
  bold: boolean;
  italic: boolean;
  underline: boolean;
  strikethrough: boolean;
  align: "left" | "center" | "right" | null;
  fontFamily: string | null;
  fontSize: number | null;
  textColor: string | null;
  bgColor: string | null;
}

/** Bytes bridge the iframe round-trips load / save through. Backed by
 *  `DriveFileSource`. Kept minimal — the iframe never touches Drive's
 *  origin except in-memory while the workbook is open. */
export interface HostFileBridge {
  open(docId: string): Promise<{ bytes: ArrayBuffer; name: string; etag?: string }>;
  save?(docId: string, bytes: ArrayBuffer, opts?: { etag?: string }): Promise<{ etag: string }>;
}

/** Errors the iframe surfaces to the host so Drive can swap in a friendly
 *  fallback card instead of the SDK's raw error UI. */
export interface SheetEmbedError {
  code: "embed_not_served" | "load_failed" | "parse_failed" | "boot_failed" | "internal";
  message: string;
}

export interface SheetEmbedRef {
  /** Switch read-only/preview ↔ full editing without remounting. */
  setViewMode(mode: "preview" | "editor"): void;
  /** Dispatch a formatting / navigation command (bold, italic, undo, set
   *  font family/size/colour, …) against the iframe's active selection.
   *  Drive's own toolbar calls this. */
  executeCommand(command: SheetCommand, args?: SheetCommandArgs): void;
  iframe(): HTMLIFrameElement | null;
}

export interface SheetEmbedProps {
  fileSource: HostFileBridge;
  docId: string;
  /** `preview` = read-only (SDK-enforced). `editor` = full editing. */
  viewMode: "preview" | "editor";
  /** Where the same-origin embed runtime is served from. Defaults to
   *  `${BASE_URL}embed/sheets`. */
  embedBasePath: string;
  onSelectionChanged?: (data: SelectionChangedData) => void;
  /** Fires when the active cell's format (bold/italic/font/colour/…)
   *  changes, so Drive's toolbar can reflect "pressed" / value state. */
  onSelectionFormatState?: (data: SelectionFormatStateData) => void;
  onError?: (data: SheetEmbedError) => void;
  testId?: string;
}

const FRAME_STYLE: CSSProperties = {
  width: "100%",
  height: "100%",
  border: "none",
  display: "block",
};

export const SheetEmbed = forwardRef<SheetEmbedRef, SheetEmbedProps>(function SheetEmbed(
  props,
  ref,
) {
  const {
    fileSource,
    docId,
    viewMode,
    embedBasePath,
    onSelectionChanged,
    onSelectionFormatState,
    onError,
    testId = "casual-sheet-workspace",
  } = props;

  const iframeRef = useRef<HTMLIFrameElement | null>(null);
  const transportRef = useRef<EmbedHostTransport | null>(null);

  // Latch props the transport closures read so swapping a callback or
  // flipping viewMode doesn't force the iframe to re-create its transport.
  const fileSourceRef = useRef(fileSource);
  fileSourceRef.current = fileSource;
  const viewModeRef = useRef(viewMode);
  viewModeRef.current = viewMode;
  const onSelectionChangedRef = useRef(onSelectionChanged);
  onSelectionChangedRef.current = onSelectionChanged;
  const onSelectionFormatStateRef = useRef(onSelectionFormatState);
  onSelectionFormatStateRef.current = onSelectionFormatState;
  const onErrorRef = useRef(onError);
  onErrorRef.current = onError;

  const onLoad = useCallback(async (req: { docId: string }): Promise<LoadResponseData> => {
    try {
      const { bytes, name, etag } = await fileSourceRef.current.open(req.docId);
      return { ok: true, bytes, fileName: name, ...(etag !== undefined ? { etag } : {}) };
    } catch (err) {
      return {
        ok: false,
        code: "open_failed",
        message: err instanceof Error ? err.message : String(err),
      };
    }
  }, []);

  const onSave = useCallback(
    async (req: { docId: string; bytes: ArrayBuffer; baseEtag?: string }): Promise<SaveResponseData> => {
      const save = fileSourceRef.current.save;
      if (!save) {
        return { ok: false, code: "save_unsupported", message: "host fileSource has no save()" };
      }
      try {
        const opts = req.baseEtag !== undefined ? { etag: req.baseEtag } : undefined;
        const { etag } = await save(req.docId, req.bytes, opts);
        return { ok: true, etag };
      } catch (err) {
        return {
          ok: false,
          code: "save_failed",
          message: err instanceof Error ? err.message : String(err),
        };
      }
    },
    [],
  );

  // (Re)bind the transport every time the iframe document (re)loads.
  const onIframeLoad = useCallback(() => {
    const iframe = iframeRef.current;
    if (!iframe?.contentWindow) return;
    transportRef.current?.destroy();
    const transport = new EmbedHostTransport({
      app: "sheet",
      iframeWindow: iframe.contentWindow,
      embedOrigin: window.location.origin,
    });
    transport.on({
      onLoadRequest: onLoad,
      onSaveRequest: onSave,
      onSelectionChanged: (d) => onSelectionChangedRef.current?.(d),
      onSelectionFormatState: (d) => onSelectionFormatStateRef.current?.(d),
      onError: (d) => onErrorRef.current?.(d as SheetEmbedError),
      onEditorReady: () => {
        transport.sendHostHello({ capabilities: ["load", "save"] });
        // The editor mounts in whatever viewMode the URL carried; send the
        // current one explicitly so a viewMode change before `ready` lands.
        transport.sendSetViewMode({ viewMode: viewModeRef.current });
      },
    });
    transportRef.current = transport;
  }, [onLoad, onSave]);

  // Push live viewMode changes to the iframe. `preview` ⇒ the SDK makes
  // the workbook read-only; we just forward the mode.
  useEffect(() => {
    transportRef.current?.sendSetViewMode({ viewMode });
  }, [viewMode]);

  useEffect(() => {
    return () => {
      transportRef.current?.destroy();
      transportRef.current = null;
    };
  }, []);

  useImperativeHandle(
    ref,
    () => ({
      setViewMode: (mode) => transportRef.current?.sendSetViewMode({ viewMode: mode }),
      executeCommand: (command, args) =>
        transportRef.current?.sendCommandExecute({ command, args }),
      iframe: () => iframeRef.current,
    }),
    [],
  );

  const url =
    `${embedBasePath}/embed.html` +
    `?app=sheet` +
    `&docId=${encodeURIComponent(docId)}` +
    `&viewMode=${viewMode}`;

  return (
    <iframe
      ref={iframeRef}
      src={url}
      onLoad={onIframeLoad}
      title="Casual Sheets"
      sandbox="allow-scripts allow-same-origin allow-downloads allow-modals"
      style={FRAME_STYLE}
      data-testid={testId}
    />
  );
});
