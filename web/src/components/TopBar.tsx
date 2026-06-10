import { ChangeEvent, useEffect, useState } from "react";
import { Grid3x3, HelpCircle, List, Rows3, Rows4, Search } from "lucide-react";

import { clearRecent, getRecent, type RecentSearch } from "../lib/recentSearches.ts";
import { NotificationsBell } from "./NotificationsBell.tsx";
import { RecentSearchesPopover } from "./RecentSearchesPopover.tsx";

export type ViewMode = "grid" | "list";
export type Density = "comfortable" | "compact";

/** SR14 — fixed id so the search input's `aria-controls` and the
 * recents popover's listbox both reference the same node. */
const RECENTS_LISTBOX_ID = "cd-search-recents-listbox";

export function TopBar({
  query,
  onQueryChange,
  view,
  onViewChange,
  density,
  onDensityChange,
  onShowHelp,
}: {
  query: string;
  onQueryChange: (q: string) => void;
  view: ViewMode;
  onViewChange: (v: ViewMode) => void;
  density: Density;
  onDensityChange: (d: Density) => void;
  onShowHelp: () => void;
}) {
  // SR11 — recent-searches dropdown state. Recents are loaded lazily
  // (only when the input gains focus, so a never-focused TopBar
  // doesn't pay the localStorage parse) and refreshed whenever Files
  // emits `cd:recents-changed` after a commit.
  const [inputFocused, setInputFocused] = useState(false);
  const [recents, setRecents] = useState<RecentSearch[]>([]);
  // SR14 — id of the currently-highlighted option in the recents
  // popover. Mirrored on the input as `aria-activedescendant` so
  // screen readers announce the row as the user arrows through.
  const [activeOptionId, setActiveOptionId] = useState<string | null>(null);

  useEffect(() => {
    function refresh() {
      setRecents(getRecent());
    }
    window.addEventListener("cd:recents-changed", refresh);
    return () => window.removeEventListener("cd:recents-changed", refresh);
  }, []);

  const popoverOpen = inputFocused && recents.length > 0;
  return (
    <header
      style={{
        display: "flex",
        alignItems: "center",
        gap: 12,
        marginBottom: 26,
      }}
    >
      <div
        role="search"
        style={{ position: "relative", flex: "1 1 auto", maxWidth: 300, marginLeft: "auto" }}
      >
        <Search
          size={16}
          strokeWidth={2}
          aria-hidden="true"
          style={{
            position: "absolute",
            left: 14,
            top: "50%",
            transform: "translateY(-50%)",
            color: "var(--muted)",
          }}
        />
        <input
          type="text"
          placeholder="Search files and folders"
          value={query}
          role="combobox"
          aria-autocomplete="list"
          aria-controls={RECENTS_LISTBOX_ID}
          aria-expanded={popoverOpen}
          aria-activedescendant={popoverOpen ? activeOptionId ?? undefined : undefined}
          aria-label="Search files and folders"
          onChange={(e: ChangeEvent<HTMLInputElement>) => onQueryChange(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && query.trim().length > 0) {
              // SR11 — Files owns search state, so it gets to record
              // the commit alongside its current filter snapshot.
              window.dispatchEvent(
                new CustomEvent<string>("cd:search-commit", { detail: query }),
              );
            }
          }}
          style={{
            width: "100%",
            border: "1px solid var(--line)",
            background: "var(--card)",
            borderRadius: 12,
            padding: "11px 14px 11px 40px",
            fontFamily: "var(--font-sans)",
            fontSize: "var(--text-base)",
            color: "var(--ink)",
            outline: "none",
            transition: "border-color 200ms, box-shadow 200ms",
          }}
          onFocus={(e) => {
            e.currentTarget.style.borderColor = "var(--line-strong)";
            e.currentTarget.style.boxShadow = "0 0 0 4px rgba(26,26,30,.04)";
            setInputFocused(true);
            setRecents(getRecent());
          }}
          onBlur={(e) => {
            e.currentTarget.style.borderColor = "var(--line)";
            e.currentTarget.style.boxShadow = "";
            // Defer the close so a click on a popover entry
            // (mousedown fires before blur) lands before the popover
            // unmounts.
            setTimeout(() => setInputFocused(false), 120);
            // Also commit on blur when the user typed something but
            // never hit Enter — keeps the recents list useful for
            // users who navigate via clicks instead of keyboard.
            if (query.trim().length > 0) {
              window.dispatchEvent(
                new CustomEvent<string>("cd:search-commit", { detail: query }),
              );
            }
          }}
        />
        <RecentSearchesPopover
          open={popoverOpen}
          recents={recents}
          query={query}
          listboxId={RECENTS_LISTBOX_ID}
          onActiveOptionChange={setActiveOptionId}
          onPick={(rec) => {
            onQueryChange(rec.query);
            window.dispatchEvent(
              new CustomEvent<typeof rec.filters>("cd:apply-filters", {
                detail: rec.filters,
              }),
            );
            setInputFocused(false);
          }}
          onClear={() => {
            clearRecent();
            setRecents([]);
          }}
          onClose={() => setInputFocused(false)}
        />
      </div>

      <ViewToggle value={view} onChange={onViewChange} />
      <DensityToggle value={density} onChange={onDensityChange} />
      <NotificationsBell />
      <button
        type="button"
        aria-label="Keyboard shortcuts"
        title="Keyboard shortcuts (?)"
        onClick={onShowHelp}
        style={{
          width: 36,
          height: 36,
          borderRadius: 11,
          border: "1px solid var(--line)",
          background: "var(--card)",
          color: "var(--muted)",
          cursor: "pointer",
          display: "inline-flex",
          alignItems: "center",
          justifyContent: "center",
          transition: "background 150ms, border-color 150ms, color 150ms",
        }}
        onMouseOver={(e) => {
          e.currentTarget.style.background = "var(--bg-hover)";
          e.currentTarget.style.color = "var(--ink)";
          e.currentTarget.style.borderColor = "var(--line-strong)";
        }}
        onMouseOut={(e) => {
          e.currentTarget.style.background = "var(--card)";
          e.currentTarget.style.color = "var(--muted)";
          e.currentTarget.style.borderColor = "var(--line)";
        }}
      >
        <HelpCircle size={17} strokeWidth={1.8} />
      </button>
    </header>
  );
}

function ViewToggle({ value, onChange }: { value: ViewMode; onChange: (v: ViewMode) => void }) {
  return (
    <div
      style={{
        display: "flex",
        border: "1px solid var(--line)",
        borderRadius: 11,
        background: "var(--card)",
        padding: 3,
        gap: 2,
      }}
    >
      <ToggleButton active={value === "grid"} onClick={() => onChange("grid")} title="Grid view">
        <Grid3x3 size={17} strokeWidth={1.8} />
      </ToggleButton>
      <ToggleButton active={value === "list"} onClick={() => onChange("list")} title="List view">
        <List size={17} strokeWidth={1.8} />
      </ToggleButton>
    </div>
  );
}

/** SR4 — row-density toggle. `Rows3` = comfortable (3 visible rows in
 * the icon, taller rows in the grid); `Rows4` = compact (more rows
 * visible, tighter padding). Title text spells it out so the choice is
 * obvious even on a touch device where Lucide's icon difference is
 * subtle. */
function DensityToggle({ value, onChange }: { value: Density; onChange: (d: Density) => void }) {
  return (
    <div
      style={{
        display: "flex",
        border: "1px solid var(--line)",
        borderRadius: 11,
        background: "var(--card)",
        padding: 3,
        gap: 2,
      }}
      role="group"
      aria-label="Row density"
    >
      <ToggleButton
        active={value === "comfortable"}
        onClick={() => onChange("comfortable")}
        title="Comfortable density"
      >
        <Rows3 size={17} strokeWidth={1.8} />
      </ToggleButton>
      <ToggleButton
        active={value === "compact"}
        onClick={() => onChange("compact")}
        title="Compact density"
      >
        <Rows4 size={17} strokeWidth={1.8} />
      </ToggleButton>
    </div>
  );
}

function ToggleButton({
  active,
  onClick,
  title,
  children,
}: {
  active: boolean;
  onClick: () => void;
  title: string;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      title={title}
      onClick={onClick}
      style={{
        border: "none",
        background: active ? "var(--ink)" : "transparent",
        cursor: "pointer",
        padding: 8,
        borderRadius: 8,
        display: "flex",
        color: active ? "var(--paper)" : "var(--muted)",
        transition: "background 180ms, color 180ms",
      }}
      onMouseOver={(e) => {
        if (!active) e.currentTarget.style.background = "var(--bg-hover)";
      }}
      onMouseOut={(e) => {
        if (!active) e.currentTarget.style.background = "transparent";
      }}
    >
      {children}
    </button>
  );
}
