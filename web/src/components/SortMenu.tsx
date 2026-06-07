/**
 * Sort dropdown — Radix DropdownMenu wrapped in our token palette.
 * Spec: docs/ux/09-sort-and-select.md.
 *
 * The component is purely presentational: state lives in <Files/> so it
 * can drive the actual sort + persist the selection.
 */
import { ArrowDown, ArrowUp, ArrowUpDown, Check } from "lucide-react";
import { DropdownMenu } from "radix-ui";

export type SortKey = "name" | "modified" | "size";
export type SortDir = "asc" | "desc";

const KEYS: { id: SortKey; label: string }[] = [
  { id: "name", label: "Name" },
  { id: "modified", label: "Modified" },
  { id: "size", label: "Size" },
];

export function SortMenu({
  sortKey,
  sortDir,
  onChange,
}: {
  sortKey: SortKey;
  sortDir: SortDir;
  onChange: (key: SortKey, dir: SortDir) => void;
}) {
  const activeLabel = KEYS.find((k) => k.id === sortKey)?.label ?? "Name";
  const ArrowIcon = sortDir === "asc" ? ArrowUp : ArrowDown;

  return (
    <DropdownMenu.Root>
      <DropdownMenu.Trigger asChild>
        <button type="button" style={triggerStyle()} aria-label="Sort">
          <ArrowUpDown size={13} strokeWidth={1.8} style={{ color: "var(--muted)" }} />
          <span>{activeLabel}</span>
          <ArrowIcon size={12} strokeWidth={2} style={{ color: "var(--muted)", marginLeft: 2 }} />
        </button>
      </DropdownMenu.Trigger>
      <DropdownMenu.Portal>
        <DropdownMenu.Content align="end" sideOffset={6} style={menuStyle()}>
          <Label>Sort by</Label>
          {KEYS.map((k) => (
            <Item key={k.id} onSelect={() => onChange(k.id, sortDir)}>
              {k.label}
              {k.id === sortKey && <Tick />}
            </Item>
          ))}
          <Sep />
          <Label>Direction</Label>
          <Item onSelect={() => onChange(sortKey, "asc")}>
            <ArrowUp size={13} strokeWidth={1.8} style={{ color: "var(--muted)" }} />
            Ascending
            {sortDir === "asc" && <Tick />}
          </Item>
          <Item onSelect={() => onChange(sortKey, "desc")}>
            <ArrowDown size={13} strokeWidth={1.8} style={{ color: "var(--muted)" }} />
            Descending
            {sortDir === "desc" && <Tick />}
          </Item>
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </DropdownMenu.Root>
  );
}

function Item({ children, onSelect }: { children: React.ReactNode; onSelect: () => void }) {
  return (
    <DropdownMenu.Item
      onSelect={(e) => {
        e.preventDefault();
        onSelect();
      }}
      style={itemStyle()}
      onMouseEnter={(e) => (e.currentTarget.style.background = "var(--bg-hover)")}
      onMouseLeave={(e) => (e.currentTarget.style.background = "transparent")}
    >
      {children}
    </DropdownMenu.Item>
  );
}

function Tick() {
  return <Check size={13} strokeWidth={2.2} style={{ marginLeft: "auto", color: "var(--accent)" }} />;
}

function Label({ children }: { children: React.ReactNode }) {
  return (
    <DropdownMenu.Label
      style={{
        fontSize: 10,
        letterSpacing: "2px",
        textTransform: "uppercase",
        color: "var(--muted-2)",
        fontWeight: 600,
        padding: "8px 10px 4px",
      }}
    >
      {children}
    </DropdownMenu.Label>
  );
}

function Sep() {
  return (
    <DropdownMenu.Separator style={{ height: 1, background: "var(--line)", margin: "4px 6px" }} />
  );
}

function triggerStyle(): React.CSSProperties {
  return {
    display: "inline-flex",
    alignItems: "center",
    gap: 6,
    padding: "7px 10px",
    borderRadius: 9,
    border: "1px solid var(--line)",
    background: "var(--card)",
    color: "var(--ink)",
    cursor: "pointer",
    fontFamily: "var(--font-sans)",
    fontSize: "var(--text-sm)",
    fontWeight: 500,
    transition: "background 150ms, border-color 150ms",
  };
}

function menuStyle(): React.CSSProperties {
  return {
    minWidth: 200,
    background: "var(--card)",
    border: "1px solid var(--line)",
    borderRadius: 12,
    boxShadow: "var(--shadow-hover)",
    padding: 6,
    fontFamily: "var(--font-sans)",
    fontSize: "var(--text-sm)",
    color: "var(--ink)",
    zIndex: 60,
    animation: "cd-menu-in 180ms var(--ease)",
  };
}

function itemStyle(): React.CSSProperties {
  return {
    display: "flex",
    alignItems: "center",
    gap: 10,
    padding: "8px 10px",
    borderRadius: 8,
    cursor: "pointer",
    userSelect: "none",
    outline: "none",
    transition: "background 120ms",
  };
}
