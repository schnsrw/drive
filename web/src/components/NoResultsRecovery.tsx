/**
 * SR12 partial — no-results recovery panel.
 *
 * Spec: docs/ux/12-search-surface.md §"No-results recovery".
 *
 * When a search comes back empty AND at least one filter chip is
 * active, we don't strand the user on a dead end. Instead the panel
 * lists one-click relaxations:
 *
 *   - Drop the Type filter (if any types selected)
 *   - Drop the Owner filter (if any owners selected)
 *   - Search in trash too (if include_trashed is false)
 *   - Widen Modified to All (if modified_after / modified_before set)
 *   - Widen Created to All (if created_after / created_before set)
 *   - Drop the Size filter (if size_min / size_max set)
 *   - Drop the share-link filter
 *   - Search across all workspaces (if scope ≠ "all")
 *
 * Each is one click; the filters mutate in place and the existing
 * search effect re-runs.
 *
 * Did-you-mean (OpenSearch phrase_suggester) is the other half of
 * the SR12 row — deferred until S2 lands.
 */
import { ArrowUpRight } from "lucide-react";

import { defaultFilters, type SearchFilters } from "../api/client.ts";

interface Relaxation {
  /** Stable id; only used for the React key. */
  id: string;
  /** Label rendered on the action button. Sentence-case, present
   * tense — matches the polish-principles "warm, direct" copy
   * rule. */
  label: string;
  /** Produces the new filter state. Receives current filters so
   * the relaxation can target one dimension surgically. */
  apply: (current: SearchFilters) => SearchFilters;
}

interface Props {
  query: string;
  filters: SearchFilters;
  onRelax: (next: SearchFilters) => void;
}

export function NoResultsRecovery({ query, filters, onRelax }: Props) {
  const options = computeRelaxations(filters);
  if (options.length === 0) return null;

  return (
    <section
      aria-label="No results — try widening the search"
      style={{
        margin: "40px auto 0",
        maxWidth: 460,
        padding: "22px 22px 16px",
        border: "1px solid var(--line)",
        borderRadius: 14,
        background: "var(--card)",
        boxShadow: "var(--shadow)",
        fontFamily: "var(--font-sans)",
      }}
    >
      <h2
        style={{
          margin: 0,
          fontFamily: "var(--font-display)",
          fontSize: "var(--text-lg)",
          fontWeight: 500,
          color: "var(--ink)",
          letterSpacing: "-0.01em",
        }}
      >
        {query.trim() ? `No matches for "${query.trim()}"` : "No matches with the current filters"}
      </h2>
      <p
        style={{
          margin: "8px 0 18px",
          fontSize: "var(--text-sm)",
          color: "var(--muted)",
          lineHeight: 1.5,
        }}
      >
        Try widening the search:
      </p>
      <ul style={{ listStyle: "none", margin: 0, padding: 0, display: "grid", gap: 6 }}>
        {options.map((o) => (
          <li key={o.id}>
            <button
              type="button"
              onClick={() => onRelax(o.apply(filters))}
              style={{
                display: "flex",
                alignItems: "center",
                gap: 10,
                width: "100%",
                padding: "10px 12px",
                borderRadius: 10,
                border: "1px solid var(--line)",
                background: "var(--paper)",
                color: "var(--ink)",
                fontFamily: "var(--font-sans)",
                fontSize: "var(--text-sm)",
                cursor: "pointer",
                textAlign: "left",
                transition: "background 120ms, border-color 120ms",
              }}
              onMouseEnter={(e) => {
                e.currentTarget.style.background = "var(--bg-hover)";
                e.currentTarget.style.borderColor = "var(--line-strong)";
              }}
              onMouseLeave={(e) => {
                e.currentTarget.style.background = "var(--paper)";
                e.currentTarget.style.borderColor = "var(--line)";
              }}
            >
              <ArrowUpRight
                size={14}
                strokeWidth={1.8}
                aria-hidden="true"
                style={{ color: "var(--accent-strong)", flexShrink: 0 }}
              />
              <span style={{ flex: 1 }}>{o.label}</span>
            </button>
          </li>
        ))}
      </ul>
    </section>
  );
}

/** Inspect the active filter set and return the relaxations the user
 * can take. Ordered by how much they typically widen the result set:
 * trash and scope last (they can return very different rows). */
function computeRelaxations(f: SearchFilters): Relaxation[] {
  const out: Relaxation[] = [];

  if (f.types.length > 0) {
    out.push({
      id: "drop-type",
      label: `Drop the Type filter (${f.types.length} selected)`,
      apply: (c) => ({ ...c, types: [] }),
    });
  }
  if (f.owner_ids.length > 0) {
    out.push({
      id: "drop-owner",
      label: "Drop the Owner filter",
      apply: (c) => ({ ...c, owner_ids: [] }),
    });
  }
  if (f.modified_after || f.modified_before) {
    out.push({
      id: "drop-modified",
      label: "Widen Modified to All time",
      apply: (c) => ({ ...c, modified_after: undefined, modified_before: undefined }),
    });
  }
  if (f.created_after || f.created_before) {
    out.push({
      id: "drop-created",
      label: "Widen Created to All time",
      apply: (c) => ({ ...c, created_after: undefined, created_before: undefined }),
    });
  }
  if (f.size_min !== undefined || f.size_max !== undefined) {
    out.push({
      id: "drop-size",
      label: "Drop the Size filter",
      apply: (c) => ({ ...c, size_min: undefined, size_max: undefined }),
    });
  }
  if (f.has_share_link !== undefined) {
    out.push({
      id: "drop-share-link",
      label: "Drop the share-link filter",
      apply: (c) => ({ ...c, has_share_link: undefined }),
    });
  }
  if (!f.include_trashed) {
    out.push({
      id: "include-trashed",
      label: "Search in trash too",
      apply: (c) => ({ ...c, include_trashed: true }),
    });
  }
  if (f.scope !== "all") {
    out.push({
      id: "scope-all",
      label: "Search across all workspaces",
      apply: () => ({ ...defaultFilters("all"), q: f.q }),
    });
  }

  return out;
}
