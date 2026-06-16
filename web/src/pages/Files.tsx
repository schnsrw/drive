import React, { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { ChevronLeft, ChevronRight as ChevronRightSeparator, Link2, MoreHorizontal, UploadCloud } from "lucide-react";
import { DropdownMenu } from "radix-ui";
import { toast } from "sonner";

import * as api from "../api/client.ts";
import {
  ApiError,
  defaultFilters,
  downloadUrl,
  hasActiveFilters,
  searchAdvanced,
  type FileDto,
  type FolderDto,
  type NoteSearchHit,
  type SearchFilters,
  type SearchResp,
  type SortBy as SearchSortBy,
  type SortDir as SearchSortDir,
  type Workspace,
} from "../api/client.ts";
import { useActiveWorkspaceId } from "../state/WorkspaceContext.tsx";
import { SearchToolbar } from "../components/SearchToolbar.tsx";
import { generateThumbnail } from "../api/thumbnail.ts";
import { forbiddenUploadExtension } from "../api/uploadPolicy.ts";
import { EmptyState } from "../components/EmptyState.tsx";
import { EntryContextMenu, EntryKebab, type Entry as MenuEntry, type EntryMenuHandlers } from "../components/EntryMenu.tsx";
import { FileMiniIcon, FileThumb, inferKind, type FileKind } from "../components/FileThumb.tsx";
import { FileViewingDot } from "../components/FileViewingDot.tsx";
import { NoResultsRecovery } from "../components/NoResultsRecovery.tsx";
import { PreviewModal } from "../components/PreviewModal.tsx";
import { RenameDialog } from "../components/RenameDialog.tsx";
import { SelectionBar } from "../components/SelectionBar.tsx";
import { ShareDialog } from "../components/ShareDialog.tsx";
import { SortMenu, type SortDir, type SortKey } from "../components/SortMenu.tsx";
import type { Density, ViewMode } from "../components/TopBar.tsx";
import { PromptDialog } from "../components/PromptDialog.tsx";
import {
  decodeSearchState,
  encodeSearchState,
  isStateNonEmpty,
  type UrlState,
} from "../lib/searchUrl.ts";
import { recordRecent } from "../lib/recentSearches.ts";
import { usePresenceActions, usePresenceUsers } from "../state/PresenceContext.tsx";
import { useAuth } from "../auth/AuthContext.tsx";
import { markPaint } from "../lib/searchMetrics.ts";

const SORT_KEY_STORAGE = "cd-sort-key-v1";

/** SR6 — read search state from the address bar on mount. Wrapped in
 * try/catch + window guard so SSR / Safari quirks degrade to "empty
 * search" rather than throwing the SPA. */
function readInitialUrlState(): UrlState {
  if (typeof window === "undefined") {
    return decodeSearchState("");
  }
  try {
    return decodeSearchState(window.location.search);
  } catch {
    return decodeSearchState("");
  }
}

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
  | {
      kind: "ready";
      folders: FolderDto[];
      files: FileDto[];
      /** Search-mode only — notes that matched the query. Folder
       * listing never sets this. The grid renders a small "Notes"
       * section above the folders+files when present. */
      notes?: NoteSearchHit[];
    }
  | { kind: "error"; message: string };

type Entry =
  | { kind: "folder"; folder: FolderDto }
  | { kind: "file"; file: FileDto };

export function Files({
  view,
  density,
  query,
  uploadRequested,
  onUploadHandled,
  newFolderRequested,
  onNewFolderHandled,
  newBlankRequested,
  newBlankKind,
  onNewBlankHandled,
  onItemCount,
}: {
  view: ViewMode;
  density: Density;
  query: string;
  uploadRequested: number;
  onUploadHandled: () => void;
  newFolderRequested: number;
  onNewFolderHandled: () => void;
  /** Bump to request a blank file create. Kind picks the template. */
  newBlankRequested: number;
  newBlankKind: "docx" | "xlsx" | null;
  onNewBlankHandled: () => void;
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

  // Phase 3 search — chip-driven filters + cursor pagination.
  // SR6 — initial state is read from URL search params so reload +
  // deep-link land back on the same search. The encode/decode pair
  // lives in `lib/searchUrl.ts`; defaults are omitted so a clean URL
  // means a clean state.
  const initialUrl = readInitialUrlState();
  const [searchFilters, setSearchFilters] = useState<SearchFilters>(initialUrl.filters);
  const [searchSort, setSearchSort] = useState<SearchSortBy>(initialUrl.sort);
  const [searchSortDir, setSearchSortDir] = useState<SearchSortDir>(initialUrl.sortDir);
  const [searchMeta, setSearchMeta] = useState<{
    total: { files: number; folders: number; notes: number; exact: boolean };
    nextCursor: string | null;
    sortApplied: SearchSortBy;
  } | null>(null);
  const [loadingMore, setLoadingMore] = useState(false);

  // SR6 — keep the URL in sync with the current search state. Writes
  // via `history.replaceState` so back/forward isn't polluted with a
  // history entry per keystroke. Skips the write when the serialized
  // string already matches what's in the address bar (avoids a
  // re-render storm when the popstate handler below echoes state
  // through). Routes that aren't "search" (everything-default) clear
  // the query string entirely so the URL doesn't read like noise.
  useEffect(() => {
    if (typeof window === "undefined") return;
    const next = encodeSearchState({
      query,
      filters: searchFilters,
      sort: searchSort,
      sortDir: searchSortDir,
    });
    const hasContent = isStateNonEmpty({
      query,
      filters: searchFilters,
      sort: searchSort,
      sortDir: searchSortDir,
    });
    const current = window.location.search.startsWith("?")
      ? window.location.search.slice(1)
      : window.location.search;
    if (next === current) return;
    const url =
      hasContent && next.length > 0
        ? `${window.location.pathname}?${next}${window.location.hash}`
        : `${window.location.pathname}${window.location.hash}`;
    try {
      window.history.replaceState(window.history.state, "", url);
    } catch {
      /* private mode / sandboxed iframes can throw — silent. */
    }
  }, [query, searchFilters, searchSort, searchSortDir]);

  // SR6 — back / forward replays the URL into local state. The
  // popstate event fires after the browser updates `window.location`,
  // so we just decode the latest. Query is owned by Shell.tsx; emit a
  // `cd:search-query` event for it to pick up.
  useEffect(() => {
    if (typeof window === "undefined") return;
    function onPop() {
      const decoded = decodeSearchState(window.location.search);
      setSearchFilters(decoded.filters);
      setSearchSort(decoded.sort);
      setSearchSortDir(decoded.sortDir);
      window.dispatchEvent(
        new CustomEvent<string>("cd:search-query", { detail: decoded.query }),
      );
    }
    window.addEventListener("popstate", onPop);
    return () => window.removeEventListener("popstate", onPop);
  }, []);

  // SR11 — TopBar dispatches `cd:search-commit` on Enter / blur with
  // a non-empty query. We pair the query with the currently-active
  // filter snapshot and record both — the dropdown re-applies both
  // when the user clicks an entry. Dedup + cap-to-10 lives in the
  // helper.
  useEffect(() => {
    function onCommit(e: Event) {
      const q = (e as CustomEvent<string>).detail;
      if (typeof q !== "string" || q.trim().length === 0) return;
      recordRecent(q, searchFilters);
      window.dispatchEvent(new Event("cd:recents-changed"));
    }
    function onApplyFilters(e: Event) {
      const detail = (e as CustomEvent<SearchFilters>).detail;
      if (detail && typeof detail === "object") {
        setSearchFilters({ ...defaultFilters(), ...detail });
      }
    }
    window.addEventListener("cd:search-commit", onCommit);
    window.addEventListener("cd:apply-filters", onApplyFilters);
    return () => {
      window.removeEventListener("cd:search-commit", onCommit);
      window.removeEventListener("cd:apply-filters", onApplyFilters);
    };
  }, [searchFilters]);

  // Workspaces list — needed for the SearchToolbar's Workspace chip
  // (only when the user has more than one) and scope picker.
  const [workspaces, setWorkspaces] = useState<Workspace[]>([]);
  useEffect(() => {
    let alive = true;
    void (async () => {
      try {
        const r = await api.listWorkspaces();
        if (alive) setWorkspaces(r.workspaces);
      } catch {
        /* ignored — toolbar degrades gracefully without the list */
      }
    })();
    return () => {
      alive = false;
    };
  }, []);

  // Whether to show the search UI vs the folder listing. Driven by
  // either a non-trivial query OR any active chip filter.
  const inSearchMode = query.trim().length >= 2 || hasActiveFilters(searchFilters);

  // SR7 — re-run-after-action signal. When `refresh()` is called from
  // inside search mode (after a rename / trash / share / bulk op) we
  // must NOT swap the result pane back to a folder listing; the
  // query is still in the input and that would be a confusing snap.
  // Bumping this tick causes the search effect to re-fire with the
  // current filter set.
  const [searchRefreshTick, setSearchRefreshTick] = useState(0);

  const refresh = useCallback(async () => {
    // SR7 — search-mode refresh re-runs the search instead of pulling
    // a folder listing. The search effect picks up the tick and
    // re-fetches `/api/search` with the current q + filters + sort.
    if (inSearchMode) {
      setSearchRefreshTick((t) => t + 1);
      return;
    }
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
  }, [current, inSearchMode, onItemCount, workspaceId]);

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

  // Phase 3 search effect — drives /api/search with the chip filter
  // set + sort + pagination. 50 ms debounce on every input change to
  // meet the SR15 spec budget (p95 keystroke→paint < 200 ms); was
  // 200 ms but that alone ate the entire user-perceived wait.
  // AbortController cancels the in-flight request on each new keystroke
  // or filter flip so stale responses never overwrite fresh ones, so
  // a tighter debounce just means more cancels — not more wasted work.
  useEffect(() => {
    if (!inSearchMode) {
      // Returned to neutral (no query + no filters) → folder listing.
      if (searched) {
        setSearched(false);
        setSearchMeta(null);
        void refresh();
      }
      return;
    }
    const controller = new AbortController();
    const handle = setTimeout(async () => {
      setState({ kind: "loading" });
      try {
        const filters: SearchFilters = { ...searchFilters, q: query.trim() };
        const data: SearchResp = await searchAdvanced(
          filters,
          { sort: searchSort, sort_dir: searchSortDir, limit: 30 },
          controller.signal,
        );
        setState({
          kind: "ready",
          folders: data.folders,
          files: data.files,
          notes: data.notes,
        });
        setSearched(true);
        setSearchMeta({
          total: data.total,
          nextCursor: data.next_cursor ?? null,
          sortApplied: data.sort_applied,
        });
        onItemCount(data.folders.length + data.files.length + data.notes.length);
        // SR15 — close the keystroke→paint measurement window AFTER
        // the browser has painted the new result pane. Double-rAF
        // pushes us past React's commit (rAF #1) and into the next
        // composited frame (rAF #2), so the timestamp lines up with
        // what the user actually sees.
        requestAnimationFrame(() => requestAnimationFrame(markPaint));
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
    }, 50);
    return () => {
      clearTimeout(handle);
      controller.abort();
    };
    // refresh + searched + setters are intentionally not in the dep set
    // — they would re-fire the effect every render. Only the live
    // search inputs should re-trigger search.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [
    query,
    inSearchMode,
    searchFilters,
    searchSort,
    searchSortDir,
    workspaceId,
    onItemCount,
    // SR7 — re-run the search when an in-place action (rename / trash
    // / share / bulk op) calls `refresh()` while in search mode.
    searchRefreshTick,
  ]);

  // Infinite scroll: when in search mode + a next_cursor exists,
  // fetch and append the next page. Triggered by an IntersectionObserver
  // attached to a sentinel ~2 viewports below the result list.
  const loadMoreRef = useRef<HTMLDivElement | null>(null);
  useEffect(() => {
    if (!inSearchMode || !searchMeta?.nextCursor || loadingMore) return;
    const sentinel = loadMoreRef.current;
    if (!sentinel) return;
    const observer = new IntersectionObserver(
      (entries) => {
        if (!entries[0].isIntersecting) return;
        if (loadingMore || !searchMeta?.nextCursor) return;
        setLoadingMore(true);
        void (async () => {
          try {
            const filters: SearchFilters = { ...searchFilters, q: query.trim() };
            const data = await searchAdvanced(filters, {
              sort: searchSort,
              sort_dir: searchSortDir,
              limit: 30,
              after: searchMeta.nextCursor!,
            });
            setState((s) =>
              s.kind === "ready"
                ? {
                    kind: "ready",
                    folders: [...s.folders, ...data.folders],
                    files: [...s.files, ...data.files],
                    notes: [...(s.notes ?? []), ...data.notes],
                  }
                : s,
            );
            setSearchMeta((m) =>
              m
                ? {
                    total: data.total,
                    nextCursor: data.next_cursor ?? null,
                    sortApplied: data.sort_applied,
                  }
                : m,
            );
          } catch {
            /* swallowed — caller will retry by scrolling again */
          } finally {
            setLoadingMore(false);
          }
        })();
      },
      { rootMargin: "0px 0px 800px 0px" },
    );
    observer.observe(sentinel);
    return () => observer.disconnect();
  }, [
    inSearchMode,
    searchMeta?.nextCursor,
    loadingMore,
    searchFilters,
    searchSort,
    searchSortDir,
    query,
  ]);

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

  const [newFolderOpen, setNewFolderOpen] = useState(false);
  const lastNewFolderTickRef = useRef(newFolderRequested);
  useEffect(() => {
    if (newFolderRequested === lastNewFolderTickRef.current) return;
    lastNewFolderTickRef.current = newFolderRequested;
    if (newFolderRequested === 0) return;
    setNewFolderOpen(true);
    onNewFolderHandled();
  }, [newFolderRequested, onNewFolderHandled, refresh, current.id]);

  // Blank-template creation (docx / xlsx). Sidebar bumps the tick + sets
  // the kind; we fetch the bundled template, wrap it as a File with a
  // unique name in the current folder, and route through the same
  // upload path so progress / quota / thumbnails behave consistently.
  const lastNewBlankTickRef = useRef(newBlankRequested);
  useEffect(() => {
    if (newBlankRequested === lastNewBlankTickRef.current) return;
    lastNewBlankTickRef.current = newBlankRequested;
    if (newBlankRequested === 0 || !newBlankKind) return;
    void (async () => {
      onNewBlankHandled();
      try {
        const ext = newBlankKind;
        const mime =
          ext === "docx"
            ? "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
            : "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet";
        // Tiny timestamp suffix so back-to-back New clicks don't collide.
        // Drive's PATCH /api/files/{id} rename is one click away if the
        // user wants something nicer.
        const base = ext === "docx" ? "Untitled" : "Untitled spreadsheet";
        const stamp = new Date()
          .toISOString()
          .replace(/[-:T]/g, "")
          .slice(2, 12); // YYMMDDhhmm
        const name = `${base} ${stamp}.${ext}`;
        // Prefix with Vite's BASE_URL — on local dev the SPA is served at /,
        // on Pages it's served at /demo-app/, and the templates live under
        // both. Hardcoding `/templates/...` made the Pages build 404.
        const resp = await fetch(`${import.meta.env.BASE_URL}templates/blank.${ext}`);
        if (!resp.ok) throw new Error(`template fetch failed: HTTP ${resp.status}`);
        const blob = await resp.blob();
        const file = new File([blob], name, { type: mime });
        const thumb = await generateThumbnail(file).catch(() => null);
        const created = await api.uploadFile(file, current.id, thumb, workspaceId);
        toast.success(`Created ${created.name}`);
        refresh();
      } catch (err) {
        const msg = err instanceof Error ? err.message : "Failed to create file";
        toast.error(msg);
      }
    })();
  }, [
    newBlankRequested,
    newBlankKind,
    onNewBlankHandled,
    refresh,
    current.id,
    workspaceId,
  ]);

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

  // RT4 — quiet peer-action toast. Watches the rolling action buffer
  // (PresenceContext) and pops a sonner toast when a peer renames /
  // trashes / etc. a file that's currently in the user's grid. Self-
  // actions and out-of-view targets are silently skipped per the
  // brief ("don't spam"). lastSeenTsRef dedupes across renders;
  // we only ever consider entries newer than the last batch.
  const presenceActions = usePresenceActions();
  const presenceUsers = usePresenceUsers();
  const { status: authStatus } = useAuth();
  const myUserId = authStatus.kind === "authed" ? authStatus.me.user_id ?? null : null;
  const lastSeenActionTsRef = useRef(0);
  useEffect(() => {
    if (presenceActions.length === 0) return;
    // Build a quick lookup for currently-rendered targets. Both file
    // and folder ids count — folder rename events still need to land.
    const visibleIds = new Set<string>();
    for (const e of filteredEntries) {
      visibleIds.add(e.kind === "file" ? e.file.id : e.folder.id);
    }
    let dirty = false;
    let newestTs = lastSeenActionTsRef.current;
    // Walk oldest-first so toasts fire in chronological order on the
    // initial burst (`presenceActions` is newest-first per the
    // PresenceContext spec).
    for (let i = presenceActions.length - 1; i >= 0; i--) {
      const a = presenceActions[i];
      if (a.received_at <= lastSeenActionTsRef.current) continue;
      if (a.received_at > newestTs) newestTs = a.received_at;
      if (myUserId && a.user_id === myUserId) continue;
      if (!a.target_id || !visibleIds.has(a.target_id)) continue;
      const verb = verbFor(a.action);
      if (!verb) continue;
      const actor = presenceUsers.find((u) => u.user_id === a.user_id)?.username ?? "Someone";
      const targetName = a.target_name ?? "a file";
      toast.message(`${actor} ${verb} ${targetName}`, { duration: 3000 });
      dirty = true;
    }
    lastSeenActionTsRef.current = newestTs;
    // Re-pull so the renamed name actually reflects in the row the
    // user just got toasted about. The search-aware refresh handles
    // both browse + search modes.
    if (dirty) void refresh();
    // refresh is intentionally not in deps — it's stable per-render
    // via useCallback and adding it would re-fire the effect on
    // every workspace change.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [presenceActions, filteredEntries, presenceUsers, myUserId]);

  // Per-entry menu handlers — built once, accept the entry inline so the
  // menu in every row/card binds to the right target.
  //
  // Click model (user directive, 2026-06-16):
  //   - Single left-click on a card → PreviewModal (metadata + preview)
  //     for ALL file types — video, md, pdf, sheet, docx, image, etc.
  //   - Double left-click on a card → `/file/<id>` editor view, also
  //     for ALL file types.
  //   - Menu "Open" mirrors double-click (jump to the editor route).
  //   - Menu "Preview" mirrors single-click (modal).
  //   - Menu "See details" mirrors Preview today; Phase 2 will replace
  //     it with a dedicated details panel (sharing + roles + audit log).
  //
  // Single-click debounce — the browser always fires `click` before
  // `dblclick`, so a naive single-click handler that opens the modal
  // would intercept the second click and the dblclick handler would
  // never fire (or fire against the modal overlay). Hold the
  // single-click action behind a 250 ms timer; if a dblclick lands
  // first, cancel the pending modal and route to the editor instead.
  const singleClickTimerRef = useRef<number | null>(null);
  function openInEditorRoute(entry: Entry) {
    if (singleClickTimerRef.current !== null) {
      window.clearTimeout(singleClickTimerRef.current);
      singleClickTimerRef.current = null;
    }
    if (entry.kind === "folder") {
      enterFolder(entry.folder);
      return;
    }
    const url = `/file/${encodeURIComponent(entry.file.id)}`;
    window.history.pushState({ file: entry.file }, "", url);
    window.dispatchEvent(new PopStateEvent("popstate"));
  }
  function handleSingleOrDouble(_file: FileDto, idx: number) {
    if (singleClickTimerRef.current !== null) {
      window.clearTimeout(singleClickTimerRef.current);
    }
    singleClickTimerRef.current = window.setTimeout(() => {
      singleClickTimerRef.current = null;
      setPreviewIdx(idx);
    }, 250);
  }

  function handlersFor(entry: MenuEntry): EntryMenuHandlers {
    // `Open` → editor route for every file type. The FileFullscreen
    // page already branches on inferKind to mount the right SDK
    // (CasualDoc / CasualSheet / image viewer / video / PDF / text /
    // generic download), so this works for non-editor types too.
    const openInEditor = (target: FileDto) => {
      const url = `/file/${encodeURIComponent(target.id)}`;
      window.history.pushState({ file: target }, "", url);
      window.dispatchEvent(new PopStateEvent("popstate"));
    };
    const preview = (id: string) => {
      const i = fileList.findIndex((f) => f.id === id);
      if (i >= 0) setPreviewIdx(i);
    };
    const open = (id: string) => {
      const target = fileList.find((f) => f.id === id);
      if (target) openInEditor(target);
    };
    // `See details` opens the same PreviewModal as Preview today.
    // Phase 2 swaps to a dedicated panel that shows people-with-access
    // (sharing) + manage-by-roles + audit log of that file.
    const details = (id: string) => preview(id);
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
      onOpen: () => open(file.id),
      onPreview: () => preview(file.id),
      onDetails: () => details(file.id),
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
      data-density={density}
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
        searching={inSearchMode}
        count={total}
        searchTotals={
          inSearchMode && searchMeta
            ? {
                files: searchMeta.total.files,
                folders: searchMeta.total.folders,
                notes: searchMeta.total.notes,
                exact: searchMeta.total.exact,
              }
            : undefined
        }
        onBack={goBack}
        onJumpTo={jumpTo}
        sort={sort}
        onSortChange={changeSort}
        showSort={!inSearchMode && state.kind === "ready" && total > 0}
      />

      {inSearchMode && (
        <SearchToolbar
          filters={searchFilters}
          sort={searchSort}
          sortDir={searchSortDir}
          workspaces={workspaces}
          activeWorkspaceName={
            workspaces.find((w) => w.id === workspaceId)?.name ?? "This workspace"
          }
          insideFolder={path.length > 1}
          activeWorkspaceId={workspaceId}
          onFiltersChange={setSearchFilters}
          onSortChange={(s, d) => {
            setSearchSort(s);
            setSearchSortDir(d);
          }}
          onClearAll={() => setSearchFilters(defaultFilters(searchFilters.scope))}
        />
      )}

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
            {/* SR12 — when the search came back empty AND there's
                at least one filter to relax, surface the recovery
                panel; otherwise fall back to the generic
                empty-state ("Try a different search."). The panel's
                computeRelaxations() returns [] when nothing's
                actionable, so we check before rendering. */}
            {inSearchMode ? (
              <NoResultsRecovery
                query={query}
                filters={searchFilters}
                onRelax={(next) => setSearchFilters(next)}
              />
            ) : null}
            {!inSearchMode || !hasActiveFilters(searchFilters) ? (
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
            ) : null}
          </div>
        )}
        {/* SR-NOTES: in search mode, surface matching notes above the
            files+folders grid. Clicks dispatch the same custom event
            CommandPalette uses, so the Notes tab opens the right page. */}
        {state.kind === "ready" && inSearchMode && (state.notes?.length ?? 0) > 0 && (
          <NoteResultsSection
            notes={state.notes!}
            onOpen={(id) => {
              window.dispatchEvent(
                new CustomEvent<string>("cd:open-note", { detail: id }),
              );
              window.dispatchEvent(
                new CustomEvent<string>("cd:nav", { detail: "notes" }),
              );
            }}
            onCopyLink={(id) => {
              // SR7 remnant — share a deep-link to this specific
              // note. Shell hydrates `?note=<id>` on mount: routes
              // to the Notes tab + fires `cd:open-note`.
              const url = `${window.location.origin}${window.location.pathname}?note=${encodeURIComponent(id)}`;
              if (typeof navigator !== "undefined" && navigator.clipboard) {
                void navigator.clipboard
                  .writeText(url)
                  .then(() => toast.success("Link copied"))
                  .catch(() => toast.error("Couldn't copy — copy from address bar"));
              } else {
                toast.error("Clipboard isn't available in this browser");
              }
            }}
          />
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
                if (entry.kind === "folder") {
                  enterFolder(entry.folder);
                } else {
                  const i = fileList.findIndex((f) => f.id === entry.file.id);
                  if (i >= 0) handleSingleOrDouble(entry.file, i);
                }
              }}
              onEntryDoubleClick={openInEditorRoute}
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
                if (entry.kind === "folder") {
                  enterFolder(entry.folder);
                } else {
                  const i = fileList.findIndex((f) => f.id === entry.file.id);
                  if (i >= 0) handleSingleOrDouble(entry.file, i);
                }
              }}
              onEntryDoubleClick={openInEditorRoute}
              handlersFor={handlersFor}
            />
          ))}
        {state.kind === "error" && (
          <div style={{ marginTop: 40 }}>
            <EmptyState title="Couldn't load files." subtitle={state.message} />
          </div>
        )}

        {/* Infinite-scroll sentinel + end-of-results divider. */}
        {inSearchMode && state.kind === "ready" && (
          <>
            {searchMeta?.nextCursor && (
              <div
                ref={loadMoreRef}
                style={{
                  display: "flex",
                  justifyContent: "center",
                  padding: "20px 0 30px",
                  color: "var(--muted)",
                  fontSize: "var(--text-xs)",
                }}
                aria-live="polite"
              >
                {loadingMore ? "Loading more…" : ""}
              </div>
            )}
            {!searchMeta?.nextCursor && total > 0 && (
              <div
                role="status"
                style={{
                  display: "flex",
                  alignItems: "center",
                  justifyContent: "center",
                  gap: 10,
                  padding: "20px 0 30px",
                  color: "var(--muted)",
                  fontSize: "var(--text-xs)",
                  letterSpacing: "0.06em",
                  textTransform: "uppercase",
                }}
              >
                <span style={{ flex: 1, maxWidth: 60, height: 1, background: "var(--line)" }} />
                End of results
                <span style={{ flex: 1, maxWidth: 60, height: 1, background: "var(--line)" }} />
              </div>
            )}
          </>
        )}
      </Stage>

      {dragOver && (
        <div
          style={{
            position: "absolute",
            inset: 0,
            background: "rgba(232, 237, 242,.85)",
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

      <PromptDialog
        open={newFolderOpen}
        title="New folder"
        label="Name"
        placeholder="Untitled folder"
        defaultValue="Untitled folder"
        submitLabel="Create folder"
        validate={(v) => {
          if (v.length < 1) return "Required";
          if (v.length > 200) return "Name is too long";
          if (/[\/\\\0]/.test(v)) return "Slashes and null bytes aren't allowed";
          return null;
        }}
        onSubmit={async (name) => {
          try {
            await api.createFolder(name, current.id, workspaceId);
            toast.success("Folder created");
            void refresh();
          } catch {
            toast.error("Couldn't create folder");
          }
        }}
        onClose={() => setNewFolderOpen(false)}
      />
    </div>
  );
}

function NoteResultsSection({
  notes,
  onOpen,
  onCopyLink,
}: {
  notes: NoteSearchHit[];
  onOpen: (id: string) => void;
  /** SR7 remnant — note hits gain a "Copy link" kebab action so users
   * can share a deep-link to a specific note. Bounded scope; full
   * rename / move / trash routing through the Notes tab UI. */
  onCopyLink: (id: string) => void;
}) {
  return (
    <section aria-label="Note results" style={{ marginBottom: 18 }}>
      <h2
        style={{
          margin: "8px 0 8px",
          fontSize: "var(--text-xs)",
          letterSpacing: "0.08em",
          textTransform: "uppercase",
          color: "var(--muted)",
          fontWeight: 600,
        }}
      >
        Notes
      </h2>
      <ul
        style={{
          listStyle: "none",
          margin: 0,
          padding: 0,
          display: "grid",
          gap: 4,
        }}
      >
        {notes.map((n) => (
          <NoteResultRow
            key={n.id}
            note={n}
            onOpen={() => onOpen(n.id)}
            onCopyLink={() => onCopyLink(n.id)}
          />
        ))}
      </ul>
    </section>
  );
}

function NoteResultRow({
  note,
  onOpen,
  onCopyLink,
}: {
  note: NoteSearchHit;
  onOpen: () => void;
  onCopyLink: () => void;
}) {
  return (
    <li style={{ position: "relative" }}>
      <button
        type="button"
        onClick={onOpen}
        style={{
          display: "flex",
          alignItems: "center",
          gap: 10,
          width: "100%",
          padding: "8px 40px 8px 10px",
          borderRadius: 8,
          background: "transparent",
          border: "1px solid var(--line)",
          color: "var(--ink)",
          cursor: "pointer",
          textAlign: "left",
          fontFamily: "var(--font-sans)",
          fontSize: "var(--text-sm)",
          transition: "background 120ms, border-color 120ms",
        }}
        onMouseEnter={(e) => {
          e.currentTarget.style.background = "var(--bg-hover)";
          e.currentTarget.style.borderColor = "var(--line-strong)";
        }}
        onMouseLeave={(e) => {
          e.currentTarget.style.background = "transparent";
          e.currentTarget.style.borderColor = "var(--line)";
        }}
      >
        <span
          aria-hidden="true"
          style={{
            width: 22,
            height: 22,
            display: "inline-flex",
            alignItems: "center",
            justifyContent: "center",
            borderRadius: 6,
            background: "var(--bg-subtle)",
            color: "var(--muted)",
            fontSize: 11,
            flexShrink: 0,
          }}
        >
          ¶
        </span>
        <span style={{ minWidth: 0, flex: 1 }}>{note.title}</span>
        <span style={{ fontSize: 11, color: "var(--muted-2)" }}>Open in Notes →</span>
      </button>
      <NoteResultKebab onCopyLink={onCopyLink} />
    </li>
  );
}

function NoteResultKebab({ onCopyLink }: { onCopyLink: () => void }) {
  // Matches the discoverability pattern from file / list rows — kebab
  // sits at 0.55 opacity by default (never invisible) and brightens
  // on row hover or focus. Same Radix DropdownMenu primitives + token
  // styles the SortMenu / EntryKebab use, so a future "Rename" /
  // "Trash" addition slots in without reshaping the surface.
  return (
    <DropdownMenu.Root>
      <DropdownMenu.Trigger asChild>
        <button
          type="button"
          aria-label="Note actions"
          onClick={(e) => e.stopPropagation()}
          onMouseDown={(e) => e.stopPropagation()}
          style={{
            position: "absolute",
            top: "50%",
            right: 8,
            transform: "translateY(-50%)",
            display: "inline-flex",
            alignItems: "center",
            justifyContent: "center",
            width: 26,
            height: 26,
            border: "none",
            background: "transparent",
            color: "var(--muted)",
            opacity: 0.55,
            borderRadius: 6,
            cursor: "pointer",
            transition: "opacity 180ms, background 150ms, color 150ms",
          }}
          onMouseEnter={(e) => {
            e.currentTarget.style.opacity = "1";
            e.currentTarget.style.background = "var(--bg-hover)";
            e.currentTarget.style.color = "var(--ink)";
          }}
          onMouseLeave={(e) => {
            e.currentTarget.style.opacity = "0.55";
            e.currentTarget.style.background = "transparent";
            e.currentTarget.style.color = "var(--muted)";
          }}
        >
          <MoreHorizontal size={15} strokeWidth={1.8} />
        </button>
      </DropdownMenu.Trigger>
      <DropdownMenu.Portal>
        <DropdownMenu.Content
          align="end"
          sideOffset={6}
          style={{
            minWidth: 160,
            background: "var(--card)",
            border: "1px solid var(--line)",
            borderRadius: 12,
            boxShadow: "var(--shadow-lg)",
            padding: 6,
            fontFamily: "var(--font-sans)",
            fontSize: "var(--text-sm)",
            color: "var(--ink)",
            zIndex: 60,
            animation: "cd-popover-in 160ms var(--ease)",
          }}
        >
          <DropdownMenu.Item
            onSelect={(e) => {
              e.preventDefault();
              onCopyLink();
            }}
            style={{
              display: "flex",
              alignItems: "center",
              gap: 9,
              padding: "8px 10px",
              borderRadius: 8,
              cursor: "pointer",
              userSelect: "none",
              outline: "none",
              transition: "background 120ms",
            }}
            onMouseEnter={(e) => (e.currentTarget.style.background = "var(--bg-hover)")}
            onMouseLeave={(e) => (e.currentTarget.style.background = "transparent")}
          >
            <Link2 size={13} strokeWidth={1.8} aria-hidden="true" />
            Copy link
          </DropdownMenu.Item>
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </DropdownMenu.Root>
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
  searchTotals,
  onBack,
  onJumpTo,
  sort,
  onSortChange,
  showSort,
}: {
  path: Crumb[];
  searching: boolean;
  count: number;
  /** When present (search mode), drives the per-kind count chip
   * ("142 files · 6 folders · 3 notes"). */
  searchTotals?: { files: number; folders: number; notes: number; exact: boolean };
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
          {searchTotals ? (
            <span
              aria-live="polite"
              style={{ fontSize: "var(--text-sm)", color: "var(--muted)", paddingBottom: 4 }}
            >
              {formatSearchTotals(searchTotals)}
            </span>
          ) : (
            (count > 0 || searching) && (
              <span style={{ fontSize: "var(--text-sm)", color: "var(--muted)", paddingBottom: 4 }}>
                {count} {count === 1 ? "item" : "items"}
              </span>
            )
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

function formatSearchTotals(t: {
  files: number;
  folders: number;
  notes: number;
  exact: boolean;
}): string {
  const parts: string[] = [];
  if (t.files > 0) parts.push(`${t.files} ${t.files === 1 ? "file" : "files"}`);
  if (t.folders > 0) parts.push(`${t.folders} ${t.folders === 1 ? "folder" : "folders"}`);
  if (t.notes > 0) parts.push(`${t.notes} ${t.notes === 1 ? "note" : "notes"}`);
  const body = parts.length === 0 ? "No matches" : parts.join(" · ");
  return t.exact || parts.length === 0 ? body : `${body} (more)`;
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
  onEntryDoubleClick,
  handlersFor,
}: {
  entries: Entry[];
  uploading: string[];
  selection: Set<string>;
  onEntryClick: (e: React.MouseEvent, entry: Entry) => void;
  onEntryDoubleClick?: (entry: Entry) => void;
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
            onDoubleClick={onEntryDoubleClick ? () => onEntryDoubleClick(e) : undefined}
            handlers={handlersFor(e)}
          />
        ) : (
          <FileCard
            key={e.file.id}
            file={e.file}
            selected={selection.has(e.file.id)}
            onClick={(ev) => onEntryClick(ev, e)}
            onDoubleClick={onEntryDoubleClick ? () => onEntryDoubleClick(e) : undefined}
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
  onDoubleClick,
  handlers,
}: {
  folder: FolderDto;
  selected: boolean;
  onClick: (e: React.MouseEvent) => void;
  onDoubleClick?: (e: React.MouseEvent) => void;
  handlers: EntryMenuHandlers;
}) {
  return (
    <EntryContextMenu entry={{ kind: "folder", folder }} handlers={handlers}>
      <Card
        onClick={onClick}
        onDoubleClick={onDoubleClick}
        folder
        selected={selected}
        kebab={<EntryKebab entry={{ kind: "folder", folder }} handlers={handlers} />}
      >
        <div style={{ height: "var(--cd-card-thumb-h)", overflow: "hidden" }}>
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
  onDoubleClick,
  handlers,
}: {
  file: FileDto;
  selected: boolean;
  onClick: (e: React.MouseEvent) => void;
  onDoubleClick?: (e: React.MouseEvent) => void;
  handlers: EntryMenuHandlers;
}) {
  const kind = inferKind(file.name, file.content_type);
  return (
    <EntryContextMenu entry={{ kind: "file", file }} handlers={handlers}>
      <Card
        onClick={onClick}
        onDoubleClick={onDoubleClick}
        selected={selected}
        kebab={<EntryKebab entry={{ kind: "file", file }} handlers={handlers} />}
      >
        <div
          style={{
            height: "var(--cd-card-thumb-h)",
            overflow: "hidden",
            borderBottom: "1px solid var(--line)",
            position: "relative",
          }}
        >
          {/* RT3 — peer-viewing dot. Renders null when no one else
              is viewing this file; tinted with that peer's avatar
              colour when they are. */}
          <FileViewingDot fileId={file.id} placement="card" />
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
          height: "var(--cd-card-thumb-h)",
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

// `Card` is a Radix `asChild` consumer (the `<EntryContextMenu>` wraps
// every card with a `ContextMenu.Trigger asChild`). For that to work,
// Card MUST forward refs AND spread arbitrary props through to the
// underlying <div> — otherwise Radix's injected `onContextMenu` and
// `ref` get dropped on the floor and right-click does nothing. This is
// what BUG-RIGHT-CLICK turned out to be post-reskin: the surface
// refactor lost the `...rest` spread.
const Card = React.forwardRef<
  HTMLDivElement,
  {
    children: React.ReactNode;
    onClick?: (e: React.MouseEvent) => void;
    folder?: boolean;
    kebab?: React.ReactNode;
    selected?: boolean;
  } & Omit<React.HTMLAttributes<HTMLDivElement>, "onClick" | "children">
>(function Card({ children, onClick, folder, kebab, selected, ...rest }, ref) {
  return (
    <div
      ref={ref}
      onClick={onClick}
      className={folder ? "cd-folder-card" : "cd-file-card"}
      {...rest}
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
        ...(rest.style ?? {}),
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
            /* Always visible at a subtle 0.55 so the affordance is
             * discoverable; brightens to 1 on card hover. Previously
             * opacity:0 by default made users believe the menu didn't
             * exist (and right-click was the only other path). */
            opacity: 0.55,
            transform: "translateY(0)",
            transition: "opacity 180ms",
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
            background: "var(--card)",
            border: "1px solid var(--line-strong)",
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
        /* Keyboard users — the kebab button itself getting focus also
         * lights it up so the menu is reachable via Tab. */
        .cd-card-kebab:focus-within {
          opacity: 1 !important;
        }
      `}</style>
    </div>
  );
});

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
    <div style={{ padding: "var(--cd-card-meta-pad-y) var(--cd-card-meta-pad-x)" }}>
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
  onEntryDoubleClick,
  handlersFor,
}: {
  entries: Entry[];
  uploading: string[];
  selection: Set<string>;
  onEntryClick: (e: React.MouseEvent, entry: Entry) => void;
  onEntryDoubleClick?: (entry: Entry) => void;
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
          padding: "var(--cd-list-row-pad-y) var(--cd-list-row-pad-x)",
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
                onDoubleClick={onEntryDoubleClick ? () => onEntryDoubleClick(e) : undefined}
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
              fileId={e.file.id}
              name={e.file.name}
              kind={kind}
              type={labelForKind(kind)}
              modified={relative(e.file.modified_at)}
              size={formatBytes(e.file.size)}
              selected={selection.has(e.file.id)}
              onClick={(ev) => onEntryClick(ev, e)}
              onDoubleClick={onEntryDoubleClick ? () => onEntryDoubleClick(e) : undefined}
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

// Same `asChild` forwardRef contract as `Card` — without it the
// list-row right-click context menu silently no-ops.
const ListRow = React.forwardRef<
  HTMLDivElement,
  {
    /** Optional — only the file rows have it; the upload-ghost row
     * passes nothing because there's no committed id yet. When
     * present, used to look up peer-viewing state for the dot. */
    fileId?: string;
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
  } & Omit<React.HTMLAttributes<HTMLDivElement>, "onClick">
>(function ListRow(
  { fileId, name, kind, type, modified, size, onClick, last, ghost, kebab, thumbnail, thumbUrls, selected, ...rest },
  ref,
) {
  return (
    <div
      ref={ref}
      onClick={onClick}
      className="cd-list-row"
      {...rest}
      style={{
        display: "grid",
        gridTemplateColumns: "2.5fr 1fr 1fr 80px 42px",
        alignItems: "center",
        padding: "var(--cd-list-row-pad-y) var(--cd-list-row-pad-x)",
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
            width: "var(--cd-list-row-thumb)",
            height: "var(--cd-list-row-thumb)",
            borderRadius: 7,
            overflow: "hidden",
            flexShrink: 0,
            display: "flex",
          }}
        >
          <FileThumb name={name} kind={kind} size="small" thumbnail={thumbnail} thumbUrls={thumbUrls} />
        </span>
        {fileId && <FileViewingDot fileId={fileId} placement="list" />}
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
          /* Discoverable by default — was opacity:0 so the menu was
           * invisible until hover, which on touch + casual desktop
           * use looked like "no actions exist." */
          opacity: 0.55,
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
});

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
            background: "rgba(15, 23, 42,.02)",
          }}
        />
      ))}
      <style>{`@keyframes cd-shimmer { from { opacity:.6 } to { opacity:1 } }`}</style>
    </div>
  );
}

// ─── helpers ────────────────────────────────────────────────────────────

/** RT4 — translate a server-side audit action string into the
 * human-readable verb for the quiet peer toast. Returns null when the
 * action shouldn't surface as a toast (e.g. self-uploads aren't
 * informative since the SPA shows its own progress chrome). */
function verbFor(action: string): string | null {
  switch (action) {
    case "files.rename":
    case "folders.rename":
      return "renamed";
    case "files.trash":
      return "moved to trash";
    case "files.upload":
      return "uploaded";
    case "folders.create":
      return "created folder";
    default:
      return null;
  }
}

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
