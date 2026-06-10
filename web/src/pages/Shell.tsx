import { lazy, Suspense, useEffect, useState } from "react";

import { DEMO_MODE } from "../api/client.ts";
import { useAuth } from "../auth/AuthContext.tsx";
import { CommandPalette } from "../components/CommandPalette.tsx";
import { ComingSoon } from "../components/ComingSoon.tsx";
import { DemoBanner } from "../components/DemoBanner.tsx";
import { EmptyState } from "../components/EmptyState.tsx";
import { HelpModal } from "../components/HelpModal.tsx";
import { Sidebar, type NavId } from "../components/Sidebar.tsx";
import { TopBar, type Density, type ViewMode } from "../components/TopBar.tsx";
import { decodeSearchState } from "../lib/searchUrl.ts";
import { Activity } from "./Activity.tsx";
import { Admin } from "./Admin.tsx";
import { Files } from "./Files.tsx";
// Notes is route-split so the Tiptap + ProseMirror bundle (~180 KB
// gzipped) only loads when the user navigates to Notes. Spec:
// docs/research/17-notes-general-user-ux.md §"Threat model" → bundle.
const Notes = lazy(() => import("./Notes.tsx").then((m) => ({ default: m.Notes })));
import { Settings } from "./Settings.tsx";

export function Shell() {
  const { status } = useAuth();
  const username = status.kind === "authed" ? status.me.admin : "admin";
  const [nav, setNav] = useState<NavId>("home");
  const [view, setView] = useState<ViewMode>("grid");
  // SR4 — result density. Persisted per-user in localStorage so the
  // choice survives page reloads. SSR-safe (typeof check) for the rare
  // case the SPA is prerendered. Doesn't affect page size.
  const [density, setDensity] = useState<Density>(() => readDensity());
  useEffect(() => {
    try {
      window.localStorage.setItem(DENSITY_STORAGE_KEY, density);
    } catch {
      /* localStorage can throw in Safari private mode — silent. */
    }
  }, [density]);
  // SR6 — seed query from `?q=…` on mount so deep-links and reloads
  // restore the search bar. Filters/sort restore themselves inside
  // Files.tsx (it owns that state).
  const [query, setQuery] = useState(() => readQueryFromUrl());
  const [itemCount, setItemCount] = useState(0);
  const [uploadTick, setUploadTick] = useState(0);
  const [newFolderTick, setNewFolderTick] = useState(0);
  const [newBlankTick, setNewBlankTick] = useState(0);
  const [newBlankKind, setNewBlankKind] = useState<"docx" | "xlsx" | null>(null);
  const [helpOpen, setHelpOpen] = useState(false);
  const [paletteOpen, setPaletteOpen] = useState(false);

  // SR7 — `?note=<id>` deep-link from a copied note search-result.
  // Routes to the Notes tab + fires `cd:open-note` once Notes has had
  // a chance to mount; matches the path the CommandPalette uses.
  // Runs ONCE on mount via the empty dep array; subsequent navigation
  // is event-driven so we don't re-route every render.
  useEffect(() => {
    if (typeof window === "undefined") return;
    const params = new URLSearchParams(window.location.search);
    const noteId = params.get("note");
    if (!noteId) return;
    setNav("notes");
    // Defer the open event so the lazy <Notes> chunk has its
    // listener attached before the event fires.
    const handle = window.setTimeout(() => {
      window.dispatchEvent(
        new CustomEvent<string>("cd:open-note", { detail: noteId }),
      );
    }, 200);
    return () => window.clearTimeout(handle);
  }, []);

  // `?` opens the help modal when the user isn't typing. Listen to the
  // bell's "View all activity →" deep-link too so a click in the dropdown
  // routes to the Activity tab.
  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      const tag = document.activeElement?.tagName;
      const typing = tag === "INPUT" || tag === "TEXTAREA";
      if (typing) return;
      if (e.key === "?" || (e.key === "/" && e.shiftKey)) {
        e.preventDefault();
        setHelpOpen(true);
      }
    }
    function onNav(e: Event) {
      const detail = (e as CustomEvent<string>).detail;
      // Files.tsx fires `cd:nav` when a note search-result is clicked —
      // need to flip to the Notes tab before the matching `cd:open-note`
      // event lands so Notes.tsx is mounted to receive it.
      if (detail === "activity" || detail === "notes" || detail === "home") {
        setNav(detail);
      }
    }
    // SR6 — Files.tsx owns URL writes (it has filters + sort too) and
    // fires `cd:search-query` after parsing popstate so this side can
    // sync without two parallel popstate handlers fighting.
    function onSearchQuery(e: Event) {
      const detail = (e as CustomEvent<string>).detail;
      setQuery(typeof detail === "string" ? detail : "");
    }
    window.addEventListener("keydown", onKey);
    window.addEventListener("cd:nav", onNav);
    window.addEventListener("cd:search-query", onSearchQuery);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("cd:nav", onNav);
      window.removeEventListener("cd:search-query", onSearchQuery);
    };
  }, []);

  return (
    <div className="h-full w-full flex flex-col" style={{ background: "var(--paper)" }}>
      {DEMO_MODE && <DemoBanner />}
      <div className="flex" style={{ flex: 1, minHeight: 0 }}>
      <Sidebar
        current={nav}
        onSelect={setNav}
        itemCount={itemCount}
        onNewFolder={() => setNewFolderTick((t) => t + 1)}
        onUpload={() => setUploadTick((t) => t + 1)}
        onNewDocument={() => {
          setNewBlankKind("docx");
          setNewBlankTick((t) => t + 1);
        }}
        onNewSpreadsheet={() => {
          setNewBlankKind("xlsx");
          setNewBlankTick((t) => t + 1);
        }}
        username={username}
      />
      <div className="flex-1 flex flex-col" style={{ minWidth: 0 }}>
        {nav === "home" && (
          <div style={{ padding: "26px 40px 0" }}>
            <TopBar
              query={query}
              onQueryChange={setQuery}
              view={view}
              onViewChange={setView}
              density={density}
              onDensityChange={setDensity}
              onShowHelp={() => setHelpOpen(true)}
            />
          </div>
        )}
        <main style={{ flex: 1, display: "flex", flexDirection: "column", minHeight: 0 }}>
          {nav === "home" && (
            <Files
              view={view}
              density={density}
              query={query}
              uploadRequested={uploadTick}
              onUploadHandled={() => {}}
              newFolderRequested={newFolderTick}
              onNewFolderHandled={() => {}}
              newBlankRequested={newBlankTick}
              newBlankKind={newBlankKind}
              onNewBlankHandled={() => setNewBlankKind(null)}
              onItemCount={setItemCount}
            />
          )}
          {nav === "recent" && (
            <CenteredPane>
              <ComingSoon
                title="Recently opened files"
                description="See the last 20 files you opened — across every folder — at the top of your Drive."
                bullets={[
                  "Auto-tracks open events and snapshots them per user",
                  "Filterable by type and date",
                  "Persists across sessions",
                ]}
              />
            </CenteredPane>
          )}
          {nav === "starred" && (
            <CenteredPane>
              <ComingSoon
                title="Starred files and folders"
                description="Pin the things you keep coming back to. Stars work across folders and survive renames."
                bullets={[
                  "Star/unstar from the preview modal or context menu",
                  "Star a folder to pin the whole tree",
                  "Synced across sessions and devices once multi-user lands",
                ]}
              />
            </CenteredPane>
          )}
          {nav === "shared" && (
            <CenteredPane>
              <ComingSoon
                title="Shared with you"
                description="Files other members of your workspace share with you appear here — ranked by recent activity."
                bullets={[
                  "View files shared via direct invite or share-link",
                  "Filter by sender and permission level (view / comment / edit)",
                  "Multi-user is queued for v0.2",
                ]}
              />
            </CenteredPane>
          )}
          {nav === "trash" && (
            <CenteredPane>
              <EmptyState
                title="Trash is empty."
                subtitle="Files you delete will appear here for 30 days before being permanently removed."
              />
            </CenteredPane>
          )}
          {nav === "notes" && (
            <Suspense
              fallback={
                <CenteredPane>
                  <EmptyState title="Loading notes…" subtitle="" />
                </CenteredPane>
              }
            >
              <Notes />
            </Suspense>
          )}
          {nav === "activity" && <Activity />}
          {nav === "admin" && <Admin onNavigate={(t) => setNav(t)} />}
          {nav === "settings" && <Settings />}
        </main>
      </div>
      </div>

      <HelpModal open={helpOpen} onClose={() => setHelpOpen(false)} />
      <CommandPalette
        open={paletteOpen}
        onOpenChange={setPaletteOpen}
        onNavigate={setNav}
        onOpenFile={(file) => {
          // Surface the right tab + fire a custom event Files listens for.
          setNav("home");
          window.dispatchEvent(
            new CustomEvent<string>("cd:open-file", { detail: file.id }),
          );
        }}
        onOpenNote={(id) => {
          setNav("notes");
          window.dispatchEvent(
            new CustomEvent<string>("cd:open-note", { detail: id }),
          );
        }}
        onShowHelp={() => setHelpOpen(true)}
      />
    </div>
  );
}

const DENSITY_STORAGE_KEY = "cd:files:density";

function readQueryFromUrl(): string {
  if (typeof window === "undefined") return "";
  try {
    return decodeSearchState(window.location.search).query;
  } catch {
    return "";
  }
}

function readDensity(): Density {
  if (typeof window === "undefined") return "comfortable";
  try {
    const raw = window.localStorage.getItem(DENSITY_STORAGE_KEY);
    return raw === "compact" ? "compact" : "comfortable";
  } catch {
    return "comfortable";
  }
}

function CenteredPane({ children }: { children: React.ReactNode }) {
  return (
    <div
      style={{
        flex: 1,
        overflow: "auto",
        background: "var(--paper)",
        padding: "40px 40px 60px",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
      }}
    >
      {children}
    </div>
  );
}
