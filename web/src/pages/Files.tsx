import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { ChevronLeft, ChevronRight as ChevronRightSeparator, UploadCloud } from "lucide-react";
import { toast } from "sonner";

import * as api from "../api/client.ts";
import { ApiError, downloadUrl, type FileDto, type FolderDto } from "../api/client.ts";
import { useActiveWorkspaceId } from "../state/WorkspaceContext.tsx";
import { generateThumbnail } from "../api/thumbnail.ts";
import { forbiddenUploadExtension } from "../api/uploadPolicy.ts";
import { EmptyState } from "../components/EmptyState.tsx";
import { EntryContextMenu, EntryKebab, type Entry as MenuEntry, type EntryMenuHandlers } from "../components/EntryMenu.tsx";
import { FileMiniIcon, FileThumb, inferKind, type FileKind } from "../components/FileThumb.tsx";
import { PreviewModal } from "../components/PreviewModal.tsx";
import { RenameDialog } from "../components/RenameDialog.tsx";
import { SelectionBar } from "../components/SelectionBar.tsx";
import { ShareDialog } from "../components/ShareDialog.tsx";
import { SortMenu, type SortDir, type SortKey } from "../components/SortMenu.tsx";
import type { ViewMode } from "../components/TopBar.tsx";

const SORT_KEY_STORAGE = "cd-sort-key-v1";

interface StoredSort {
  key: SortKey;
  dir: SortDir;
}

function loadStoredSort(): StoredSort {
  try {
    const raw = window.localStorage.getItem(SORT_KEY_STORAGE);
    if (raw) {
      const parsed = JSON.parse(raw) as Partial<StoredSort>;
      const key: SortKey = ["name", "modified", "size"].includes(parsed.key as string)
        ? (parsed.key as SortKey)
        : "name";
      const dir: SortDir = parsed.dir === "desc" ? "desc" : "asc";
      return { key, dir };
    }
  } catch {
    /* ignored — fall through to defaults */
  }
  return { key: "name", dir: "asc" };
}

function persistSort(s: StoredSort) {
  try {
    window.localStorage.setItem(SORT_KEY_STORAGE, JSON.stringify(s));
  } catch {
    /* ignored */
  }
}

/**
 * Run `worker` over `items` with at most `n` in flight at once. Returns
 * the same shape as `Promise.allSettled` so the call site keeps working
 * the same way for partial-failure handling. Pipeline §6.6.
 */
async function mapWithConcurrency<I, O>(
  items: I[],
  n: number,
  worker: (item: I, index: number) => Promise<O>,
): Promise<PromiseSettledResult<O>[]> {
  const results: PromiseSettledResult<O>[] = new Array(items.length);
  let next = 0;
  const lane = async () => {
    while (true) {
      const i = next++;
      if (i >= items.length) return;
      try {
        results[i] = { status: "fulfilled", value: await worker(items[i], i) };
      } catch (reason) {
        results[i] = { status: "rejected", reason };
      }
    }
  };
  await Promise.all(Array.from({ length: Math.min(n, items.length) }, lane));
  return results;
}

function entryId(e: Entry): string {
  return e.kind === "folder" ? e.folder.id : e.file.id;
}

interface Crumb {
  id: string | null; // null = root
  name: string;
}

type LoadState =
  | { kind: "loading" }
  | { kind: "ready"; folders: FolderDto[]; files: FileDto[] }
  | { kind: "error"; message: string };

type Entry =
  | { kind: "folder"; folder: FolderDto }
  | { kind: "file"; file: FileDto };

export function Files({
  view,
  query,
  uploadRequested,
  onUploadHandled,
  newFolderRequested,
  onNewFolderHandled,
  onItemCount,
}: {
  view: ViewMode;
  query: string;
  uploadRequested: number;
  onUploadHandled: () => void;
  newFolderRequested: number;
  onNewFolderHandled: () => void;
  onItemCount: (n: number) => void;
}) {
  // Active workspace — switching it resets the breadcrumb and refetches.
  const workspaceId = useActiveWorkspaceId();

  // Breadcrumb path: always starts with root.
  const [path, setPath] = useState<Crumb[]>([{ id: null, name: "My Drive" }]);
  const current = path[path.length - 1];

  // When the workspace changes, drop the breadcrumb back to root — folder
  // ids from the prior workspace would 404 (or worse, leak metadata via
  // the find_by_id path) under the new scope.
  const lastWorkspaceRef = useRef(workspaceId);
  useEffect(() => {
    if (lastWorkspaceRef.current === workspaceId) return;
    lastWorkspaceRef.current = workspaceId;
    setPath([{ id: null, name: "My Drive" }]);
  }, [workspaceId]);

  const [state, setState] = useState<LoadState>({ kind: "loading" });
  const [uploading, setUploading] = useState<string[]>([]);
  const [dragOver, setDragOver] = useState(false);
  const fileInputRef = useRef<HTMLInputElement | null>(null);

  // Preview modal
  const [previewIdx, setPreviewIdx] = useState<number | null>(null);

  // Rename dialog
  const [renaming, setRenaming] = useState<MenuEntry | null>(null);

  // Share dialog (files only — folder shares are v0.2)
  const [sharing, setSharing] = useState<FileDto | null>(null);

  // Sort — persisted to localStorage.
  const [sort, setSort] = useState<StoredSort>(loadStoredSort);
  function changeSort(key: SortKey, dir: SortDir) {
    const next = { key, dir };
    setSort(next);
    persistSort(next);
  }

  // Multi-select.
  const [selection, setSelection] = useState<Set<string>>(new Set());
  const [selectionAnchor, setSelectionAnchor] = useState<string | null>(null);

  // Track whether the latest load was a search vs a folder listing so we
  // know if the user's `query` is "live" against the rendered set.
  const [searched, setSearched] = useState(false);

  const refresh = useCallback(async () => {
    setState({ kind: "loading" });
    setSearched(false);
    try {
      if (current.id === null) {
        const data = await api.listRoot(workspaceId);
        setState({ kind: "ready", folders: data.folders, files: data.files });
        onItemCount(data.folders.length + data.files.length);
      } else {
        const detail = await api.getFolder(current.id);
        setState({
          kind: "ready",
          folders: detail.children.folders,
          files: detail.children.files,
        });
        onItemCount(detail.children.folders.length + detail.children.files.length);
      }
    } catch (err) {
      const msg =
        err instanceof ApiError
          ? err.status === 401
            ? "Signed out for security."
            : `Couldn't load files (${err.status}).`
          : "Couldn't reach the server.";
      setState({ kind: "error", message: msg });
    }
  }, [current, onItemCount, workspaceId]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  // Cmd-K palette → "open file" routes here via a CustomEvent. We look
  // up the file in whichever list is currently rendered; if it's not
  // there (different folder or a search result that didn't survive the
  // last fetch), fall back to fetching its metadata and opening anyway.
  useEffect(() => {
    function onOpen(e: Event) {
      const id = (e as CustomEvent<string>).detail;
      if (!id) return;
      if (state.kind !== "ready") return;
      const idx = state.files.findIndex((f) => f.id === id);
      if (idx >= 0) {
        setPreviewIdx(idx);
        return;
      }
      // Not in the current pane — pull it as a singleton list so the
      // preview modal has something to render.
      void (async () => {
        try {
          const meta = await fetch(`/api/files/${encodeURIComponent(id)}`)
            .then((r) => (r.ok ? r.json() : null))
            .catch(() => null);
          if (meta && typeof meta === "object" && "id" in meta) {
            setState({ kind: "ready", folders: [], files: [meta as FileDto] });
            setPreviewIdx(0);
          }
        } catch {
          /* ignored — palette caller already toasted */
        }
      })();
    }
    window.addEventListener("cd:open-file", onOpen);
    return () => window.removeEventListener("cd:open-file", onOpen);
  }, [state]);

  // Switch to global search when the query gets long enough; otherwise
  // fall back to the current folder listing. 200ms debounce + abort the
  // in-flight request on the next keystroke so we don't flash stale data.
  useEffect(() => {
    const q = query.trim();
    if (q.length < 2) {
      // Returned to a too-short query and we were previously in search
      // mode → re-list the current folder.
      if (searched) void refresh();
      return;
    }
    const controller = new AbortController();
    const handle = setTimeout(async () => {
      setState({ kind: "loading" });
      try {
        const data = await api.searchAll(q, controller.signal, workspaceId);
        setState({ kind: "ready", folders: data.folders, files: data.files });
        setSearched(true);
        onItemCount(data.folders.length + data.files.length);
      } catch (err) {
        if (controller.signal.aborted) return;
        const msg =
          err instanceof ApiError
            ? err.status === 401
              ? "Signed out for security."
              : `Search failed (${err.status}).`
            : "Couldn't reach the server.";
        setState({ kind: "error", message: msg });
      }
    }, 200);
    return () => {
      clearTimeout(handle);
      controller.abort();
    };
    // refresh + searched are intentionally not in the dep set — they
    // would re-fire the effect every render. Only the live query string
    // and the active workspace should re-trigger search.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [query, onItemCount, workspaceId]);

  // Parent-triggered upload + new-folder. Both use a "tick" counter
  // that the parent increments on action. We track the last seen tick
  // in a ref so the effect only fires when the prop CHANGES — not on
  // mount with a carried-over value. Without this, switching tabs and
  // returning re-mounts <Files/> with the old tick still > 0 and the
  // file picker keeps popping open unprompted.
  const lastUploadTickRef = useRef(uploadRequested);
  useEffect(() => {
    if (uploadRequested === lastUploadTickRef.current) return;
    lastUploadTickRef.current = uploadRequested;
    if (uploadRequested > 0) {
      fileInputRef.current?.click();
      onUploadHandled();
    }
  }, [uploadRequested, onUploadHandled]);

  const lastNewFolderTickRef = useRef(newFolderRequested);
  useEffect(() => {
    if (newFolderRequested === lastNewFolderTickRef.current) return;
    lastNewFolderTickRef.current = newFolderRequested;
    if (newFolderRequested === 0) return;
    (async () => {
      const name = window.prompt("Folder name", "Untitled folder");
      if (name && name.trim()) {
        try {
          await api.createFolder(name.trim(), current.id, workspaceId);
          toast.success("Folder created");
          void refresh();
        } catch {
          toast.error("Couldn't create folder");
        }
      }
      onNewFolderHandled();
    })();
  }, [newFolderRequested, onNewFolderHandled, refresh, current.id]);

  const uploadAll = useCallback(
    async (files: FileList | File[]) => {
      const all = Array.from(files);
      if (all.length === 0) return;

      // Client-side blocklist — save the round-trip when we already know
      // the server will refuse. The server enforces the same list.
      const blocked = all.filter((f) => forbiddenUploadExtension(f.name) !== null);
      const list = all.filter((f) => forbiddenUploadExtension(f.name) === null);
      if (blocked.length > 0) {
        const exts = Array.from(
          new Set(blocked.map((f) => `.${forbiddenUploadExtension(f.name)}`)),
        ).join(", ");
        toast.error(
          `${blocked.length} blocked: ${exts}`,
          { description: "These file types can't be uploaded for security reasons." },
        );
      }
      if (list.length === 0) return;

      setUploading(list.map((f) => f.name));
      // Pipeline §6.6 — concurrent upload cap. Dragging in 20 files
      // shouldn't open 20 multipart connections + spin 20 thumbnail
      // canvases at once. Cap at 4; mirrors the server's per-user upload
      // rate limit so we batch instead of bursting.
      const results = await mapWithConcurrency(list, 4, async (f) => {
        const thumb = await generateThumbnail(f).catch(() => null);
        return api.uploadFile(f, current.id, thumb, workspaceId);
      });
      setUploading([]);
      const ok = results.filter((r) => r.status === "fulfilled").length;
      // Any failure here is server-side (network, quota, magic-byte sniff
      // once it lands). Surface the first explanatory error inline.
      const firstErr = results.find((r) => r.status === "rejected") as PromiseRejectedResult | undefined;
      if (ok === list.length) {
        toast.success(`Uploaded ${ok} ${ok === 1 ? "file" : "files"}`);
      } else if (ok > 0) {
        toast.warning(`Uploaded ${ok} of ${list.length}, ${list.length - ok} failed`);
      } else if (firstErr) {
        const e = firstErr.reason as { status?: number; body?: { error?: string; extension?: string } };
        if (e?.status === 415 && e?.body?.extension) {
          toast.error(`.${e.body.extension} files aren't allowed.`);
        } else {
          toast.error(e?.body?.error ?? "Upload failed");
        }
      }
      void refresh();
    },
    [refresh, current.id, workspaceId],
  );

  function onDrop(e: React.DragEvent) {
    e.preventDefault();
    setDragOver(false);
    if (e.dataTransfer.files.length > 0) void uploadAll(e.dataTransfer.files);
  }
  function onFilePicked(e: React.ChangeEvent<HTMLInputElement>) {
    if (e.target.files) void uploadAll(e.target.files);
    e.target.value = "";
  }

  function enterFolder(f: FolderDto) {
    setPath((p) => [...p, { id: f.id, name: f.name }]);
  }
  function goBack() {
    setPath((p) => (p.length > 1 ? p.slice(0, -1) : p));
  }
  function jumpTo(idx: number) {
    setPath((p) => p.slice(0, idx + 1));
  }

  // Filter for search + sort. Folders always come before files within the
  // chosen sort key; that's the spec, and it matches every reference Drive.
  const filteredEntries = useMemo<Entry[]>(() => {
    if (state.kind !== "ready") return [];
    const q = query.trim().toLowerCase();
    const folders = state.folders
      .filter((f) => !q || f.name.toLowerCase().includes(q))
      .map((f) => ({ kind: "folder" as const, folder: f }));
    const files = state.files
      .filter((f) => !q || f.name.toLowerCase().includes(q))
      .map((f) => ({ kind: "file" as const, file: f }));

    const cmp = (a: Entry, b: Entry): number => {
      switch (sort.key) {
        case "modified": {
          const ta = (a.kind === "folder" ? a.folder.modified_at : a.file.modified_at) ?? "";
          const tb = (b.kind === "folder" ? b.folder.modified_at : b.file.modified_at) ?? "";
          return ta.localeCompare(tb);
        }
        case "size": {
          // Folders don't have a recursive size in v0 — fall back to name
          // for parity. Files compare numerically.
          if (a.kind === "folder" && b.kind === "folder") {
            return a.folder.name.localeCompare(b.folder.name, undefined, { numeric: true });
          }
          if (a.kind === "file" && b.kind === "file") {
            return a.file.size - b.file.size;
          }
          return 0;
        }
        case "name":
        default: {
          const na = a.kind === "folder" ? a.folder.name : a.file.name;
          const nb = b.kind === "folder" ? b.folder.name : b.file.name;
          return na.localeCompare(nb, undefined, { numeric: true, sensitivity: "base" });
        }
      }
    };

    folders.sort(cmp);
    files.sort(cmp);
    if (sort.dir === "desc") {
      folders.reverse();
      files.reverse();
    }
    return [...folders, ...files];
  }, [state, query, sort]);

  const total = filteredEntries.length;
  const fileList = useMemo(
    () => filteredEntries.filter((e): e is { kind: "file"; file: FileDto } => e.kind === "file").map((e) => e.file),
    [filteredEntries],
  );

  // Per-entry menu handlers — built once, accept the entry inline so the
  // menu in every row/card binds to the right target.
  function handlersFor(entry: MenuEntry): EntryMenuHandlers {
    const openFile = (id: string) => {
      const i = fileList.findIndex((f) => f.id === id);
      if (i >= 0) setPreviewIdx(i);
    };
    if (entry.kind === "folder") {
      return {
        onOpen: () => enterFolder(entry.folder),
        onRename: () => setRenaming(entry),
        onTrash: () => {
          toast.info("Folder trash is coming in v0.2.", {
            description: "The recursive trash + restore flow ships alongside the Trash surface.",
          });
        },
      };
    }
    const file = entry.file;
    return {
      onOpen: () => openFile(file.id),
      onPreview: () => openFile(file.id),
      onDetails: () => openFile(file.id),
      onRename: () => setRenaming(entry),
      onShare: () => setSharing(file),
      onDownload: () => {
        const url = downloadUrl(file.id);
        const a = document.createElement("a");
        a.href = url;
        a.download = file.name;
        document.body.appendChild(a);
        a.click();
        document.body.removeChild(a);
      },
      onTrash: async () => {
        try {
          await api.trashFile(file.id);
          toast.success(`Moved "${file.name}" to trash`);
          void refresh();
        } catch {
          toast.error("Couldn't trash the file.");
        }
      },
    };
  }

  // Backspace = back (when not typing). ⌘/Ctrl-A selects every entry. Esc
  // clears selection.
  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      const tag = document.activeElement?.tagName;
      const typing = tag === "INPUT" || tag === "TEXTAREA";
      if (e.key === "Backspace" && !typing && path.length > 1) {
        e.preventDefault();
        goBack();
        return;
      }
      if (e.key === "Escape" && selection.size > 0) {
        e.preventDefault();
        setSelection(new Set());
        return;
      }
      if (!typing && (e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "a" && filteredEntries.length > 0) {
        e.preventDefault();
        setSelection(new Set(filteredEntries.map(entryId)));
      }
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [path.length, selection.size, filteredEntries]);

  // Selection always resets when the folder changes — carrying a selection
  // across folder boundaries is a v0.2 polish (would need bulk-move-by-id).
  useEffect(() => {
    setSelection(new Set());
    setSelectionAnchor(null);
  }, [current.id]);

  // Pointer-driven selection: clicks dispatch on modifier keys. Returns
  // `true` if the caller should still treat the click as an "open" action
  // (no selection happened, single bare click on already-selected item).
  function handleEntryClick(
    e: React.MouseEvent,
    entry: Entry,
    list: Entry[],
  ): "open" | "selected" {
    const id = entryId(entry);
    if (e.shiftKey && selectionAnchor) {
      const from = list.findIndex((x) => entryId(x) === selectionAnchor);
      const to = list.findIndex((x) => entryId(x) === id);
      if (from === -1 || to === -1) return "selected";
      const [a, b] = from < to ? [from, to] : [to, from];
      const range = list.slice(a, b + 1).map(entryId);
      const next = new Set(selection);
      range.forEach((rid) => next.add(rid));
      setSelection(next);
      return "selected";
    }
    if (e.metaKey || e.ctrlKey) {
      const next = new Set(selection);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      setSelection(next);
      setSelectionAnchor(id);
      return "selected";
    }
    // Plain click: if there's an existing multi-selection, replace it with
    // just this item and proceed to open. If this item is the only one
    // already selected, treat as open. Otherwise replace selection.
    if (selection.size === 0) {
      setSelectionAnchor(id);
      return "open";
    }
    setSelection(new Set());
    setSelectionAnchor(id);
    return "open";
  }

  async function bulkTrash() {
    const ids = Array.from(selection);
    const fileIds = ids.filter((id) =>
      filteredEntries.some((e) => e.kind === "file" && e.file.id === id),
    );
    const folderCount = ids.length - fileIds.length;
    if (folderCount > 0) {
      toast.info("Folder trash is coming in v0.2.", {
        description: `${folderCount} folder${folderCount === 1 ? "" : "s"} skipped.`,
      });
    }
    const results = await Promise.allSettled(fileIds.map((id) => api.trashFile(id)));
    const ok = results.filter((r) => r.status === "fulfilled").length;
    if (ok > 0) toast.success(`Moved ${ok} file${ok === 1 ? "" : "s"} to trash`);
    if (ok < fileIds.length) toast.error(`${fileIds.length - ok} failed`);
    setSelection(new Set());
    void refresh();
  }

  function bulkDownload() {
    const fileIds = Array.from(selection).filter((id) =>
      filteredEntries.some((e) => e.kind === "file" && e.file.id === id),
    );
    if (fileIds.length === 0) {
      toast.info("Folder download is coming in v0.2.");
      return;
    }
    fileIds.forEach((id) => {
      const entry = filteredEntries.find((e) => e.kind === "file" && e.file.id === id) as
        | { kind: "file"; file: FileDto }
        | undefined;
      if (!entry) return;
      const a = document.createElement("a");
      a.href = downloadUrl(entry.file.id);
      a.download = entry.file.name;
      document.body.appendChild(a);
      a.click();
      document.body.removeChild(a);
    });
    toast.success(`Downloading ${fileIds.length} file${fileIds.length === 1 ? "" : "s"}`);
  }

  return (
    <div
      onDragOver={(e) => {
        e.preventDefault();
        setDragOver(true);
      }}
      onDragLeave={(e) => {
        if (e.currentTarget === e.target) setDragOver(false);
      }}
      onDrop={onDrop}
      style={{
        position: "relative",
        flex: 1,
        display: "flex",
        flexDirection: "column",
        background: "var(--paper)",
        overflow: "auto",
        padding: "26px 40px 60px",
      }}
    >
      <Header
        path={path}
        searching={!!query}
        count={total}
        onBack={goBack}
        onJumpTo={jumpTo}
        sort={sort}
        onSortChange={changeSort}
        showSort={state.kind === "ready" && total > 0}
      />

      <input
        ref={fileInputRef}
        type="file"
        multiple
        onChange={onFilePicked}
        style={{ display: "none" }}
      />

      <Stage key={current.id ?? "root"}>
        {state.kind === "loading" && <GridSkeleton view={view} />}
        {state.kind === "ready" && total === 0 && uploading.length === 0 && (
          <div style={{ marginTop: 40 }}>
            <EmptyState
              title={
                query
                  ? `No files match "${query}"`
                  : path.length > 1
                    ? "This folder is empty."
                    : "Your Drive is empty."
              }
              subtitle={
                query
                  ? "Try a different search."
                  : "Drop files here or use the New button to add something."
              }
            />
          </div>
        )}
        {state.kind === "ready" &&
          (total > 0 || uploading.length > 0) &&
          (view === "grid" ? (
            <GridView
              entries={filteredEntries}
              uploading={uploading}
              selection={selection}
              onEntryClick={(e, entry) => {
                const action = handleEntryClick(e, entry, filteredEntries);
                if (action !== "open") return;
                if (entry.kind === "folder") enterFolder(entry.folder);
                else {
                  const i = fileList.findIndex((f) => f.id === entry.file.id);
                  if (i >= 0) setPreviewIdx(i);
                }
              }}
              handlersFor={handlersFor}
            />
          ) : (
            <ListView
              entries={filteredEntries}
              uploading={uploading}
              selection={selection}
              onEntryClick={(e, entry) => {
                const action = handleEntryClick(e, entry, filteredEntries);
                if (action !== "open") return;
                if (entry.kind === "folder") enterFolder(entry.folder);
                else {
                  const i = fileList.findIndex((f) => f.id === entry.file.id);
                  if (i >= 0) setPreviewIdx(i);
                }
              }}
              handlersFor={handlersFor}
            />
          ))}
        {state.kind === "error" && (
          <div style={{ marginTop: 40 }}>
            <EmptyState title="Couldn't load files." subtitle={state.message} />
          </div>
        )}
      </Stage>

      {dragOver && (
        <div
          style={{
            position: "absolute",
            inset: 0,
            background: "rgba(242,240,234,.85)",
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            pointerEvents: "none",
            zIndex: 10,
          }}
        >
          <div
            style={{
              display: "flex",
              flexDirection: "column",
              alignItems: "center",
              gap: 12,
              padding: "24px 32px",
              border: "2px dashed var(--accent)",
              borderRadius: "var(--radius-xl)",
              background: "var(--card)",
              color: "var(--ink)",
              boxShadow: "var(--shadow-md)",
            }}
          >
            <UploadCloud size={32} strokeWidth={1.8} style={{ color: "var(--accent)" }} />
            <span style={{ fontSize: "var(--text-md)", fontWeight: 500 }}>
              Drop to upload to {current.name}
            </span>
          </div>
        </div>
      )}

      <PreviewModal
        files={fileList}
        index={previewIdx ?? 0}
        open={previewIdx !== null}
        onClose={() => setPreviewIdx(null)}
        onChangeIndex={(i) => setPreviewIdx(i)}
      />

      {renaming && (
        <RenameDialog
          open
          current={renaming.kind === "folder" ? renaming.folder.name : renaming.file.name}
          label={renaming.kind === "folder" ? "Folder" : "File"}
          onClose={() => setRenaming(null)}
          onSubmit={async (newName) => {
            if (renaming.kind === "folder") {
              await api.renameFolder(renaming.folder.id, newName);
            } else {
              await api.renameFile(renaming.file.id, newName);
            }
            toast.success("Renamed");
            void refresh();
          }}
        />
      )}

      <ShareDialog open={sharing !== null} file={sharing} onClose={() => setSharing(null)} />

      {selection.size > 0 && (
        <SelectionBar
          count={selection.size}
          onClear={() => setSelection(new Set())}
          onDownload={bulkDownload}
          onTrash={bulkTrash}
        />
      )}
    </div>
  );
}

function Stage({ children }: { children: React.ReactNode }) {
  return (
    <div
      style={{
        animation: "cd-stage 420ms var(--ease)",
      }}
    >
      {children}
      <style>
        {`
          @keyframes cd-stage {
            from { opacity: 0; transform: translateY(8px); }
            to   { opacity: 1; transform: translateY(0); }
          }
        `}
      </style>
    </div>
  );
}

function Header({
  path,
  searching,
  count,
  onBack,
  onJumpTo,
  sort,
  onSortChange,
  showSort,
}: {
  path: Crumb[];
  searching: boolean;
  count: number;
  onBack: () => void;
  onJumpTo: (idx: number) => void;
  sort: { key: SortKey; dir: SortDir };
  onSortChange: (key: SortKey, dir: SortDir) => void;
  showSort: boolean;
}) {
  const deep = path.length > 1;
  const current = path[path.length - 1];

  return (
    <div style={{ display: "flex", alignItems: "flex-end", gap: 14, marginBottom: 30 }}>
      {deep && (
        <button
          type="button"
          aria-label="Back"
          title="Back (Backspace)"
          onClick={onBack}
          style={{
            width: 34,
            height: 34,
            borderRadius: 10,
            border: "1px solid var(--line)",
            background: "var(--card)",
            cursor: "pointer",
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            color: "var(--ink)",
            flexShrink: 0,
            marginBottom: 2,
            transition: "background 150ms, transform 150ms",
          }}
          onMouseOver={(e) => {
            e.currentTarget.style.background = "var(--bg-hover)";
            e.currentTarget.style.transform = "translateX(-2px)";
          }}
          onMouseOut={(e) => {
            e.currentTarget.style.background = "var(--card)";
            e.currentTarget.style.transform = "";
          }}
        >
          <ChevronLeft size={17} strokeWidth={2} />
        </button>
      )}

      <div style={{ flex: 1 }}>
        {/* Breadcrumbs */}
        {deep && !searching && (
          <div
            style={{
              display: "flex",
              alignItems: "center",
              gap: 7,
              fontSize: "var(--text-sm)",
              color: "var(--muted)",
              marginBottom: 8,
              flexWrap: "wrap",
            }}
          >
            {path.slice(0, -1).map((c, i) => (
              <CrumbButton key={i} label={c.name} onClick={() => onJumpTo(i)} sep />
            ))}
          </div>
        )}

        <div style={{ display: "flex", alignItems: "center", gap: 14 }}>
          <h1
            style={{
              margin: 0,
              fontFamily: "var(--font-display)",
              fontSize: "var(--text-3xl)",
              fontWeight: 500,
              letterSpacing: "var(--tracking-tighter)",
              color: "var(--ink)",
            }}
          >
            {searching ? "Search results" : current.name}
          </h1>
          {(count > 0 || searching) && (
            <span style={{ fontSize: "var(--text-sm)", color: "var(--muted)", paddingBottom: 4 }}>
              {count} {count === 1 ? "item" : "items"}
            </span>
          )}
        </div>
      </div>

      {showSort && (
        <div style={{ paddingBottom: 4, flexShrink: 0 }}>
          <SortMenu sortKey={sort.key} sortDir={sort.dir} onChange={onSortChange} />
        </div>
      )}
    </div>
  );
}

function CrumbButton({ label, onClick, sep }: { label: string; onClick: () => void; sep?: boolean }) {
  return (
    <>
      <button
        type="button"
        onClick={onClick}
        style={{
          border: "none",
          background: "transparent",
          cursor: "pointer",
          color: "var(--muted)",
          fontSize: "var(--text-sm)",
          padding: "3px 5px",
          borderRadius: 7,
          transition: "background 150ms, color 150ms",
        }}
        onMouseOver={(e) => {
          e.currentTarget.style.background = "var(--bg-hover)";
          e.currentTarget.style.color = "var(--ink)";
        }}
        onMouseOut={(e) => {
          e.currentTarget.style.background = "transparent";
          e.currentTarget.style.color = "var(--muted)";
        }}
      >
        {label}
      </button>
      {sep && <ChevronRightSeparator size={13} style={{ color: "var(--muted-2)" }} />}
    </>
  );
}

// ─── Views ───────────────────────────────────────────────────────────

function GridView({
  entries,
  uploading,
  selection,
  onEntryClick,
  handlersFor,
}: {
  entries: Entry[];
  uploading: string[];
  selection: Set<string>;
  onEntryClick: (e: React.MouseEvent, entry: Entry) => void;
  handlersFor: (entry: MenuEntry) => EntryMenuHandlers;
}) {
  return (
    <div
      style={{
        display: "grid",
        gridTemplateColumns: "repeat(auto-fill, minmax(190px, 1fr))",
        gap: 16,
      }}
    >
      {entries.map((e) =>
        e.kind === "folder" ? (
          <FolderCard
            key={e.folder.id}
            folder={e.folder}
            selected={selection.has(e.folder.id)}
            onClick={(ev) => onEntryClick(ev, e)}
            handlers={handlersFor(e)}
          />
        ) : (
          <FileCard
            key={e.file.id}
            file={e.file}
            selected={selection.has(e.file.id)}
            onClick={(ev) => onEntryClick(ev, e)}
            handlers={handlersFor(e)}
          />
        ),
      )}
      {uploading.map((name) => (
        <GhostCard key={name} name={name} />
      ))}
    </div>
  );
}

function FolderCard({
  folder,
  selected,
  onClick,
  handlers,
}: {
  folder: FolderDto;
  selected: boolean;
  onClick: (e: React.MouseEvent) => void;
  handlers: EntryMenuHandlers;
}) {
  return (
    <EntryContextMenu entry={{ kind: "folder", folder }} handlers={handlers}>
      <Card
        onClick={onClick}
        folder
        selected={selected}
        kebab={<EntryKebab entry={{ kind: "folder", folder }} handlers={handlers} />}
      >
        <div style={{ height: 130, overflow: "hidden" }}>
          <FileThumb name={folder.name} kind="fold" />
        </div>
        <CardMeta name={folder.name} kind="fold" sub={`Folder · ${relative(folder.modified_at)}`} />
      </Card>
    </EntryContextMenu>
  );
}

function FileCard({
  file,
  selected,
  onClick,
  handlers,
}: {
  file: FileDto;
  selected: boolean;
  onClick: (e: React.MouseEvent) => void;
  handlers: EntryMenuHandlers;
}) {
  const kind = inferKind(file.name, file.content_type);
  return (
    <EntryContextMenu entry={{ kind: "file", file }} handlers={handlers}>
      <Card
        onClick={onClick}
        selected={selected}
        kebab={<EntryKebab entry={{ kind: "file", file }} handlers={handlers} />}
      >
        <div style={{ height: 130, overflow: "hidden", borderBottom: "1px solid var(--line)" }}>
          <FileThumb
            name={file.name}
            kind={kind}
            thumbnail={file.thumbnail}
            thumbUrls={file.thumb_urls}
          />
        </div>
        <CardMeta name={file.name} kind={kind} sub={`${labelForKind(kind)} · ${relative(file.modified_at)}`} />
      </Card>
    </EntryContextMenu>
  );
}

function GhostCard({ name }: { name: string }) {
  return (
    <Card>
      <div
        style={{
          height: 130,
          background: "var(--bg-hover)",
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
        }}
      >
        <UploadCloud size={28} strokeWidth={1.6} style={{ color: "var(--accent)" }} />
      </div>
      <CardMeta name={name} kind="generic" sub="Uploading…" />
    </Card>
  );
}

function Card({
  children,
  onClick,
  folder,
  kebab,
  selected,
}: {
  children: React.ReactNode;
  onClick?: (e: React.MouseEvent) => void;
  folder?: boolean;
  kebab?: React.ReactNode;
  selected?: boolean;
}) {
  return (
    <div
      onClick={onClick}
      className={folder ? "cd-folder-card" : "cd-file-card"}
      style={{
        background: selected ? "var(--bg-selected)" : "var(--card)",
        border: `${selected ? "2px" : "1px"} solid ${selected ? "var(--accent)" : "var(--line)"}`,
        borderRadius: "var(--radius)",
        overflow: "hidden",
        cursor: onClick ? "pointer" : "default",
        transition: "transform 300ms var(--ease), box-shadow 300ms, border-color 300ms",
        boxShadow: "var(--shadow)",
        position: "relative",
        userSelect: "none",
      }}
      onMouseOver={(e) => {
        e.currentTarget.style.transform = "translateY(-3px)";
        e.currentTarget.style.boxShadow = "var(--shadow-hover)";
        if (!selected) e.currentTarget.style.borderColor = "var(--line-strong)";
      }}
      onMouseOut={(e) => {
        e.currentTarget.style.transform = "";
        e.currentTarget.style.boxShadow = "var(--shadow)";
        if (!selected) e.currentTarget.style.borderColor = "var(--line)";
      }}
    >
      {children}
      {kebab && (
        <span
          className="cd-card-kebab"
          style={{
            position: "absolute",
            top: 10,
            right: 10,
            opacity: 0,
            transform: "translateY(-2px)",
            transition: "opacity 180ms, transform 180ms",
          }}
        >
          {kebab}
        </span>
      )}
      {folder && (
        <span
          aria-hidden
          style={{
            position: "absolute",
            top: 12,
            right: 12,
            width: 26,
            height: 26,
            borderRadius: 8,
            background: "rgba(251,250,246,.92)",
            border: "1px solid var(--line)",
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            opacity: 0,
            transform: "translateX(4px)",
            transition: "opacity 200ms, transform 200ms",
            pointerEvents: "none",
          }}
          className="cd-open-hint"
        >
          <ChevronRightSeparator size={13} style={{ color: "var(--ink)" }} />
        </span>
      )}
      <style>{`
        .cd-folder-card:hover .cd-open-hint,
        .cd-folder-card:hover .cd-card-kebab,
        .cd-file-card:hover .cd-card-kebab {
          opacity: 1;
          transform: translateX(0) translateY(0);
        }
      `}</style>
    </div>
  );
}

function CardMeta({
  name,
  kind,
  sub,
}: {
  name: string;
  kind: FileKind;
  sub: string;
}) {
  return (
    <div style={{ padding: "13px 15px" }}>
      <div style={{ display: "flex", alignItems: "center", gap: 9 }}>
        <FileMiniIcon kind={kind} />
        <span
          style={{
            fontSize: "var(--text-base)",
            fontWeight: 500,
            whiteSpace: "nowrap",
            overflow: "hidden",
            textOverflow: "ellipsis",
          }}
        >
          {name}
        </span>
      </div>
      <div
        style={{
          fontSize: "var(--text-xs)",
          color: "var(--muted)",
          marginTop: 6,
          display: "flex",
          gap: 7,
        }}
      >
        <span>{sub}</span>
      </div>
    </div>
  );
}

function ListView({
  entries,
  uploading,
  selection,
  onEntryClick,
  handlersFor,
}: {
  entries: Entry[];
  uploading: string[];
  selection: Set<string>;
  onEntryClick: (e: React.MouseEvent, entry: Entry) => void;
  handlersFor: (entry: MenuEntry) => EntryMenuHandlers;
}) {
  return (
    <div
      style={{
        background: "var(--card)",
        border: "1px solid var(--line)",
        borderRadius: "var(--radius)",
        overflow: "hidden",
        boxShadow: "var(--shadow)",
      }}
    >
      <div
        style={{
          display: "grid",
          gridTemplateColumns: "2.5fr 1fr 1fr 80px 42px",
          alignItems: "center",
          padding: "13px 22px",
          gap: 16,
          fontSize: "var(--text-xs)",
          letterSpacing: "1.5px",
          textTransform: "uppercase",
          color: "var(--muted-2)",
          fontWeight: 600,
          borderBottom: "1px solid var(--line)",
        }}
      >
        <span>Name</span>
        <span>Type</span>
        <span>Modified</span>
        <span style={{ textAlign: "right" }}>Size</span>
        <span />
      </div>
      {entries.map((e, i) => {
        const last = i === entries.length - 1 && uploading.length === 0;
        if (e.kind === "folder") {
          const entry: MenuEntry = { kind: "folder", folder: e.folder };
          const handlers = handlersFor(entry);
          return (
            <EntryContextMenu key={e.folder.id} entry={entry} handlers={handlers}>
              <ListRow
                name={e.folder.name}
                kind="fold"
                type="Folder"
                modified={relative(e.folder.modified_at)}
                size="—"
                last={last}
                selected={selection.has(e.folder.id)}
                onClick={(ev) => onEntryClick(ev, e)}
                kebab={<EntryKebab entry={entry} handlers={handlers} />}
              />
            </EntryContextMenu>
          );
        }
        const kind = inferKind(e.file.name, e.file.content_type);
        const entry: MenuEntry = { kind: "file", file: e.file };
        const handlers = handlersFor(entry);
        return (
          <EntryContextMenu key={e.file.id} entry={entry} handlers={handlers}>
            <ListRow
              name={e.file.name}
              kind={kind}
              type={labelForKind(kind)}
              modified={relative(e.file.modified_at)}
              size={formatBytes(e.file.size)}
              selected={selection.has(e.file.id)}
              onClick={(ev) => onEntryClick(ev, e)}
              last={last}
              kebab={<EntryKebab entry={entry} handlers={handlers} />}
              thumbnail={e.file.thumbnail}
              thumbUrls={e.file.thumb_urls}
            />
          </EntryContextMenu>
        );
      })}
      {uploading.map((name) => (
        <ListRow key={name} name={name} kind="generic" type="Uploading…" modified="" size="" ghost last />
      ))}
    </div>
  );
}

function ListRow({
  name,
  kind,
  type,
  modified,
  size,
  onClick,
  last,
  ghost,
  kebab,
  thumbnail,
  thumbUrls,
  selected,
}: {
  name: string;
  kind: FileKind;
  type: string;
  modified: string;
  size: string;
  onClick?: (e: React.MouseEvent) => void;
  last?: boolean;
  ghost?: boolean;
  kebab?: React.ReactNode;
  thumbnail?: string | null;
  thumbUrls?: { small: string; medium: string; large: string } | null;
  selected?: boolean;
}) {
  return (
    <div
      onClick={onClick}
      className="cd-list-row"
      style={{
        display: "grid",
        gridTemplateColumns: "2.5fr 1fr 1fr 80px 42px",
        alignItems: "center",
        padding: "13px 22px",
        gap: 16,
        fontSize: "var(--text-base)",
        cursor: onClick ? "pointer" : "default",
        borderBottom: last ? "none" : "1px solid var(--line)",
        opacity: ghost ? 0.6 : 1,
        background: selected ? "var(--bg-selected)" : "transparent",
        boxShadow: selected ? "inset 2px 0 0 var(--accent)" : "none",
        transition: "background 150ms",
        userSelect: "none",
      }}
      onMouseOver={(e) => {
        if (onClick && !selected) e.currentTarget.style.background = "var(--bg-row-hover)";
      }}
      onMouseOut={(e) => {
        e.currentTarget.style.background = selected ? "var(--bg-selected)" : "transparent";
      }}
    >
      <div style={{ display: "flex", alignItems: "center", gap: 12, minWidth: 0, fontWeight: 500 }}>
        <span
          style={{
            width: 30,
            height: 30,
            borderRadius: 7,
            overflow: "hidden",
            flexShrink: 0,
            display: "flex",
          }}
        >
          <FileThumb name={name} kind={kind} size="small" thumbnail={thumbnail} thumbUrls={thumbUrls} />
        </span>
        <span style={{ whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>{name}</span>
      </div>
      <span style={{ color: "var(--muted)", fontSize: "var(--text-sm)" }}>{type}</span>
      <span style={{ color: "var(--muted)", fontSize: "var(--text-sm)" }}>{modified}</span>
      <span
        className="tabular-nums"
        style={{ color: "var(--muted)", fontSize: "var(--text-sm)", textAlign: "right" }}
      >
        {size}
      </span>
      <span
        className="cd-row-kebab"
        style={{
          display: "flex",
          justifyContent: "flex-end",
          opacity: 0,
          transition: "opacity 180ms",
        }}
      >
        {kebab}
      </span>
      <style>{`
        .cd-list-row:hover .cd-row-kebab,
        .cd-list-row:focus-within .cd-row-kebab { opacity: 1; }
      `}</style>
    </div>
  );
}

function GridSkeleton({ view }: { view: ViewMode }) {
  if (view === "grid") {
    return (
      <div
        style={{
          display: "grid",
          gridTemplateColumns: "repeat(auto-fill, minmax(190px, 1fr))",
          gap: 16,
        }}
      >
        {Array.from({ length: 6 }).map((_, i) => (
          <div
            key={i}
            style={{
              background: "var(--card)",
              border: "1px solid var(--line)",
              borderRadius: "var(--radius)",
              height: 188,
              animation: "cd-shimmer 1.4s ease-in-out infinite alternate",
            }}
          />
        ))}
        <style>{`@keyframes cd-shimmer { from { opacity:.6 } to { opacity:1 } }`}</style>
      </div>
    );
  }
  return (
    <div
      style={{
        background: "var(--card)",
        border: "1px solid var(--line)",
        borderRadius: "var(--radius)",
        overflow: "hidden",
      }}
    >
      {Array.from({ length: 6 }).map((_, i) => (
        <div
          key={i}
          style={{
            height: 48,
            borderBottom: i === 5 ? "none" : "1px solid var(--line)",
            animation: "cd-shimmer 1.4s ease-in-out infinite alternate",
            background: "rgba(26,26,30,.02)",
          }}
        />
      ))}
      <style>{`@keyframes cd-shimmer { from { opacity:.6 } to { opacity:1 } }`}</style>
    </div>
  );
}

// ─── helpers ────────────────────────────────────────────────────────────

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

function relative(iso: string): string {
  const then = new Date(iso).getTime();
  const now = Date.now();
  const diff = Math.floor((now - then) / 1000);
  if (diff < 60) return "just now";
  if (diff < 3600) return `${Math.floor(diff / 60)} min ago`;
  if (diff < 86_400) return `${Math.floor(diff / 3600)} hrs ago`;
  if (diff < 7 * 86_400) return `${Math.floor(diff / 86_400)} days ago`;
  return new Date(iso).toLocaleDateString(undefined, {
    year: "numeric",
    month: "short",
    day: "numeric",
  });
}
