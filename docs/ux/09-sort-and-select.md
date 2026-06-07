# 09 — Sort + multi-select

Companion to `02-surface-v2.md`. Covers the file-browser header sort menu and the multi-select / selection-bar surface.

## Pattern reference

**Dropbox / Google Drive / Finder** converge on the same two primitives:

1. A compact **sort dropdown** in the header — single key (Name / Modified / Size) with an asc/desc toggle. Folders pinned above files within whichever key is active.
2. **Cmd/Ctrl-click + Shift-click** to add or extend a selection, with a docked **selection bar** showing count + bulk actions.

We pick the same shape because:
- Both are universal — no learning curve.
- They compose cleanly with the existing right-click context menu (single-item) without competing for the same affordance.

## Sort dropdown

```
┌─ File-browser header (already shipped) ────────────────────────────────┐
│  My Drive · 12 items                       [search]   [grid|list]      │
│                                                                         │
│  Folders first  •  Name ▲     [ ⇅ Sort ▾ ]                              │
└─────────────────────────────────────────────────────────────────────────┘
```

- Trigger: a small ghost button labelled `↕ Sort` next to the view toggle.
- Body: a Radix DropdownMenu with two sub-groups —
  - **Sort by**: `Name` / `Modified` / `Size`
  - **Direction**: `Ascending` / `Descending`
- Default: `Name` + `Ascending`.
- Folders are always rendered before files within whichever sort is active. Sort by Modified shows folders sorted by their modified time first, then files. By Size, folders sort by name (we don't recursively size them in v0).
- Persistence: `cd-sort-key-v1` in `localStorage`. Survives reload + sign-out.
- Keyboard: nothing in v0. ⌘1 / ⌘2 cycling lands when we have time for shortcuts.

## Multi-select

### Modifiers (Mac names; Windows/Linux substitutes Ctrl)

| Input | Behaviour |
|---|---|
| Click a row/card | Single-select that item (clears previous selection if any). |
| ⌘-click | Toggle that item in the current selection. |
| Shift-click | Range-select: every item from the last clicked anchor to this one, inclusive. |
| ⌘-A | Select every visible entry (folders + files). |
| Esc | Clear selection. |
| Right-click on an *unselected* item | Selects only that item, then opens the context menu (preserves the existing single-item flow). |
| Right-click on a *selected* item | Opens the context menu against the **whole** selection. v0 only wires bulk-trash + bulk-download here. |

### Selection bar

Docked **at the bottom** of the file pane, slides up with the same 200 ms motion as toasts. Anchored to the viewport (not the scroll body) so it stays visible while the user scrolls a long list.

```
┌─ Bottom of the file pane ───────────────────────────────────────────┐
│   3 selected    [ Clear ]            [ ↓ Download (zip) ]  [ ⋮ ]   │
│                                       [ ⤓ Move ]   [ 🗑 Trash ]      │
└─────────────────────────────────────────────────────────────────────┘
```

(emoji shown for layout sketch only — real UI uses Lucide SVGs)

- Count chip on the left. Clicking it does nothing (the chip is informational).
- **Clear** button next to the chip is the "I'm done" affordance for non-keyboard users.
- Primary actions on the right, ordered safest → most destructive:
  1. **Download (zip)** — wired in v0 via parallel client-side download calls; backend bulk-zip endpoint is v0.2.
  2. **Move** — toast `"Move is coming in v0.2"` for now (matches the menu stub).
  3. **Trash** — confirms inline if >5 items: `"Move 7 files to trash?"` + a destructive button. ≤5 items go direct.
- The bar disappears (translate-Y + opacity, 180 ms) when selection drops to zero.

### Visual selection state

- Selected card: 2 px `--accent` outline + subtle `--bg-selected` overlay.
- Selected row: full-row `--bg-selected` background + a left-edge `--accent` accent stripe (2 px wide).
- Hover-over-selected: row darkens slightly; outline grows to 3 px.

## State checklist

| | Sort menu | Selection |
|---|---|---|
| Default | Name ▲ | empty |
| Loading | menu trigger disabled while listing | n/a |
| Empty (zero items) | menu trigger hidden | n/a |
| Active | persisted | count chip ≥ 1 |
| Error | menu still usable (errors live on the list, not the controls) | bulk action shows toast on partial failure |

## Out of scope (v0)

- Drag-rectangle selection (lasso) — v0.2 polish.
- Keyboard arrow-key navigation between cards/rows — Phase 2 paired with focus model.
- Server-side bulk endpoints (`/api/files/bulk-trash`, `/api/files/bulk-zip`) — v0.2.
- Multi-select across folder navigation — selection clears on folder change in v0; carrying it across is a v0.2 polish (and a hard one because of cross-folder bulk operations).
