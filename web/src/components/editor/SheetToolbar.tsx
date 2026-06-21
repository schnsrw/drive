/**
 * SheetToolbar — Drive-side formatting ribbon for the `.xlsx` editor
 * route. Sits above `<CasualSheetWorkspace mode="editor">` and drives
 * Univer through the SDK's `casual.command.execute` wire (sheet 0.7+).
 * UX-EDITOR-1 v2.
 *
 * Why Drive owns the toolbar instead of the SDK:
 *   - Univer's built-in ribbon needs the ribbon plugins which all
 *     resolve `IRPCChannelService` at construction; the SDK doesn't
 *     bundle a worker that provides it. Enabling the ribbon silently
 *     wedges the workbench mount.
 *   - Drive's design language (Slate paper, cyan accents) belongs to
 *     Drive — the SDK's toolbar would always look "borrowed".
 *
 * v0.7 surface (matches what the SDK's command wire supports today):
 *   - Undo / Redo
 *   - Font family + size
 *   - Bold / Italic / Underline / Strikethrough
 *   - Text colour + fill colour (swatches)
 *   - Align Left / Center / Right
 *   - Merge / Unmerge
 *
 * Later versions: borders, number formats, wrap, freeze rows/cols,
 * insert chart/image — each behind a new command-execute entry.
 */
import { useEffect, useState, type CSSProperties, type RefObject } from "react";
import {
  AlignCenter,
  AlignLeft,
  AlignRight,
  Bold,
  ChevronDown,
  Italic,
  Merge,
  Minus,
  PaintBucket,
  Plus,
  Redo,
  Strikethrough,
  Type,
  Underline,
  Undo,
  type LucideIcon,
} from "lucide-react";
import { DropdownMenu, Popover } from "radix-ui";

import type { SheetEmbedRef } from "./SheetEmbed.tsx";

export interface SheetFormatState {
  bold: boolean;
  italic: boolean;
  underline: boolean;
  strikethrough: boolean;
  align: "left" | "center" | "right" | null;
  // v0.7+ — null when the cell inherits the workbook default
  fontFamily: string | null;
  fontSize: number | null;
  textColor: string | null;
  bgColor: string | null;
}

/** The SDK 0.7 command union. Mirrors `CommandExecuteData['command']`
 *  from `@casualoffice/sheets`. */
type Command =
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
  | "unmerge";

interface CommandArgs {
  family?: string;
  size?: number;
  color?: string;
}

const FONT_FAMILIES = [
  "Calibri",
  "Arial",
  "Helvetica",
  "Inter",
  "Times New Roman",
  "Georgia",
  "Courier New",
  "Verdana",
  "JetBrains Mono",
];

const FONT_SIZES = [8, 9, 10, 11, 12, 14, 16, 18, 20, 24, 28, 32, 36, 48];

const TEXT_COLOR_SWATCHES = [
  "#000000",
  "#434343",
  "#666666",
  "#999999",
  "#cccccc",
  "#ffffff",
  "#e63946",
  "#f4a261",
  "#e9c46a",
  "#2a9d8f",
  "#264653",
  "#1a73e8",
  "#9c27b0",
];

const FILL_SWATCHES = [
  null, // null = no fill / clear
  "#fef3c7",
  "#fde68a",
  "#fed7aa",
  "#fca5a5",
  "#f3e8ff",
  "#c7d2fe",
  "#bfdbfe",
  "#a7f3d0",
  "#d9f99d",
  "#fef08a",
  "#fbcfe8",
  "#e2e8f0",
];

interface ToolDef {
  cmd: Command;
  Icon: LucideIcon;
  label: string;
  shortcut?: string;
  active?: (s: SheetFormatState) => boolean;
}

// Reuse v0.6 button groups for the boolean / nav toolbar items.
const TOGGLE_GROUPS: ToolDef[][] = [
  [
    { cmd: "undo", Icon: Undo, label: "Undo", shortcut: "⌘Z" },
    { cmd: "redo", Icon: Redo, label: "Redo", shortcut: "⌘Y" },
  ],
  [
    { cmd: "bold", Icon: Bold, label: "Bold", shortcut: "⌘B", active: (s) => s.bold },
    { cmd: "italic", Icon: Italic, label: "Italic", shortcut: "⌘I", active: (s) => s.italic },
    {
      cmd: "underline",
      Icon: Underline,
      label: "Underline",
      shortcut: "⌘U",
      active: (s) => s.underline,
    },
    {
      cmd: "strikethrough",
      Icon: Strikethrough,
      label: "Strikethrough",
      shortcut: "⌘⇧X",
      active: (s) => s.strikethrough,
    },
  ],
];

const ALIGN_GROUP: ToolDef[] = [
  { cmd: "align-left", Icon: AlignLeft, label: "Align left", active: (s) => s.align === "left" },
  {
    cmd: "align-center",
    Icon: AlignCenter,
    label: "Align center",
    active: (s) => s.align === "center",
  },
  {
    cmd: "align-right",
    Icon: AlignRight,
    label: "Align right",
    active: (s) => s.align === "right",
  },
];

export function SheetToolbar({
  iframeRef,
  formatState,
}: {
  iframeRef: RefObject<SheetEmbedRef | null>;
  formatState: SheetFormatState;
}) {
  // Each toolbar control calls this to dispatch a command (with args)
  // over the embed transport to the iframe's active selection.
  // `SheetEmbedRef.executeCommand(command, args?)` carries args intact.
  function dispatch(cmd: Command, args?: CommandArgs) {
    iframeRef.current?.executeCommand(cmd, args);
  }

  // Hotkeys: ⌘B/I/U/⇧X + ⌘Z/⌘Y. Browser already gives Univer the
  // keystrokes inside the iframe, but this ensures the host's
  // toolbar acts as a single source of truth when host focus is on
  // the chrome (filename input, share dialog) rather than the iframe.
  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      if (!(e.metaKey || e.ctrlKey)) return;
      const tag = (e.target as HTMLElement | null)?.tagName;
      if (tag === "INPUT" || tag === "TEXTAREA") return;
      const k = e.key.toLowerCase();
      let cmd: Command | null = null;
      if (e.shiftKey && k === "x") cmd = "strikethrough";
      else if (k === "b") cmd = "bold";
      else if (k === "i") cmd = "italic";
      else if (k === "u") cmd = "underline";
      else if (k === "z") cmd = "undo";
      else if (k === "y") cmd = "redo";
      if (cmd) {
        dispatch(cmd);
      }
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return (
    <div
      data-testid="sheet-toolbar"
      role="toolbar"
      aria-label="Spreadsheet formatting"
      style={{
        flex: "0 0 auto",
        display: "flex",
        alignItems: "center",
        gap: 8,
        padding: "6px 14px",
        borderBottom: "1px solid var(--line)",
        background: "var(--card)",
        overflowX: "auto",
      }}
    >
      <ToolGroup
        tools={TOGGLE_GROUPS[0]}
        formatState={formatState}
        dispatch={dispatch}
        divider
      />
      <FontFamilyPicker value={formatState.fontFamily} dispatch={dispatch} />
      <Divider />
      <FontSizeStepper value={formatState.fontSize} dispatch={dispatch} />
      <Divider />
      <ToolGroup
        tools={TOGGLE_GROUPS[1]}
        formatState={formatState}
        dispatch={dispatch}
        divider
      />
      <TextColorButton current={formatState.textColor} dispatch={dispatch} />
      <FillColorButton current={formatState.bgColor} dispatch={dispatch} />
      <Divider />
      <ToolGroup
        tools={ALIGN_GROUP}
        formatState={formatState}
        dispatch={dispatch}
        divider
      />
      <MergeButton dispatch={dispatch} />
    </div>
  );
}

function Divider() {
  return (
    <span
      aria-hidden
      style={{
        width: 1,
        height: 22,
        background: "var(--line)",
        flexShrink: 0,
      }}
    />
  );
}

function ToolGroup({
  tools,
  formatState,
  dispatch,
  divider,
}: {
  tools: ToolDef[];
  formatState: SheetFormatState;
  dispatch: (cmd: Command, args?: CommandArgs) => void;
  divider: boolean;
}) {
  return (
    <>
      <div style={{ display: "flex", gap: 2 }}>
        {tools.map((t) => (
          <ToolButton
            key={t.cmd}
            tool={t}
            active={t.active?.(formatState) ?? false}
            onClick={() => dispatch(t.cmd)}
          />
        ))}
      </div>
      {divider && <Divider />}
    </>
  );
}

function ToolButton({
  tool,
  active,
  onClick,
}: {
  tool: ToolDef;
  active: boolean;
  onClick: () => void;
}) {
  const [hovered, setHovered] = useState(false);
  return (
    <button
      type="button"
      onClick={onClick}
      aria-label={tool.label}
      aria-pressed={active}
      title={tool.shortcut ? `${tool.label} (${tool.shortcut})` : tool.label}
      data-testid={`sheet-tool-${tool.cmd}`}
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
      style={iconButtonStyle(active, hovered)}
    >
      <tool.Icon size={15} strokeWidth={1.8} />
    </button>
  );
}

function FontFamilyPicker({
  value,
  dispatch,
}: {
  value: string | null;
  dispatch: (cmd: Command, args?: CommandArgs) => void;
}) {
  const label = value ?? "Calibri";
  return (
    <DropdownMenu.Root>
      <DropdownMenu.Trigger asChild>
        <button
          type="button"
          aria-label="Font family"
          title="Font family"
          data-testid="sheet-tool-font-family"
          style={pickerButtonStyle(140)}
        >
          <span
            style={{
              flex: 1,
              textAlign: "left",
              overflow: "hidden",
              textOverflow: "ellipsis",
              whiteSpace: "nowrap",
              fontFamily: label,
            }}
          >
            {label}
          </span>
          <ChevronDown size={14} />
        </button>
      </DropdownMenu.Trigger>
      <DropdownMenu.Portal>
        <DropdownMenu.Content
          align="start"
          sideOffset={4}
          style={menuStyle()}
          data-testid="sheet-font-family-menu"
        >
          {FONT_FAMILIES.map((f) => (
            <DropdownMenu.Item
              key={f}
              onSelect={() => dispatch("set-font-family", { family: f })}
              data-testid={`sheet-font-${f.replace(/\s+/g, "-").toLowerCase()}`}
              style={menuItemStyle(value === f)}
            >
              <span style={{ fontFamily: f }}>{f}</span>
            </DropdownMenu.Item>
          ))}
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </DropdownMenu.Root>
  );
}

function FontSizeStepper({
  value,
  dispatch,
}: {
  value: number | null;
  dispatch: (cmd: Command, args?: CommandArgs) => void;
}) {
  const current = value ?? 11;
  const dec = () => {
    const next = FONT_SIZES[Math.max(0, FONT_SIZES.indexOf(current) - 1)] ?? current - 1;
    if (next !== current) dispatch("set-font-size", { size: next });
  };
  const inc = () => {
    const idx = FONT_SIZES.indexOf(current);
    const next = FONT_SIZES[Math.min(FONT_SIZES.length - 1, (idx === -1 ? FONT_SIZES.length : idx) + 1)] ?? current + 1;
    if (next !== current) dispatch("set-font-size", { size: next });
  };
  return (
    <div
      style={{
        display: "inline-flex",
        alignItems: "center",
        border: "1px solid var(--line)",
        borderRadius: 6,
        height: 28,
        padding: "0 2px",
      }}
    >
      <button
        type="button"
        onClick={dec}
        aria-label="Decrease font size"
        data-testid="sheet-tool-font-size-dec"
        style={smallIconBtn()}
      >
        <Minus size={13} />
      </button>
      <DropdownMenu.Root>
        <DropdownMenu.Trigger asChild>
          <button
            type="button"
            aria-label="Font size"
            data-testid="sheet-tool-font-size"
            style={{
              minWidth: 28,
              padding: "0 4px",
              fontSize: "var(--text-sm)",
              fontWeight: 500,
              color: "var(--text)",
              background: "transparent",
              border: "none",
              cursor: "pointer",
            }}
          >
            {current}
          </button>
        </DropdownMenu.Trigger>
        <DropdownMenu.Portal>
          <DropdownMenu.Content align="start" sideOffset={4} style={menuStyle()}>
            {FONT_SIZES.map((s) => (
              <DropdownMenu.Item
                key={s}
                onSelect={() => dispatch("set-font-size", { size: s })}
                data-testid={`sheet-font-size-${s}`}
                style={menuItemStyle(s === current)}
              >
                {s}
              </DropdownMenu.Item>
            ))}
          </DropdownMenu.Content>
        </DropdownMenu.Portal>
      </DropdownMenu.Root>
      <button
        type="button"
        onClick={inc}
        aria-label="Increase font size"
        data-testid="sheet-tool-font-size-inc"
        style={smallIconBtn()}
      >
        <Plus size={13} />
      </button>
    </div>
  );
}

function TextColorButton({
  current,
  dispatch,
}: {
  current: string | null;
  dispatch: (cmd: Command, args?: CommandArgs) => void;
}) {
  const stripColor = current ?? "#e63946";
  return (
    <ColorPopover
      label="Text colour"
      testId="sheet-tool-text-color"
      icon={<Type size={15} strokeWidth={1.8} />}
      stripColor={stripColor}
      swatches={TEXT_COLOR_SWATCHES.map((c) => ({ value: c, label: c }))}
      onPickColor={(c) => dispatch("set-text-color", { color: c })}
      onReset={() => dispatch("reset-text-color")}
    />
  );
}

function FillColorButton({
  current,
  dispatch,
}: {
  current: string | null;
  dispatch: (cmd: Command, args?: CommandArgs) => void;
}) {
  const stripColor = current ?? "#fef3c7";
  return (
    <ColorPopover
      label="Fill colour"
      testId="sheet-tool-bg-color"
      icon={<PaintBucket size={15} strokeWidth={1.8} />}
      stripColor={stripColor}
      swatches={FILL_SWATCHES.map((c) => ({
        value: c,
        label: c ?? "No fill",
      }))}
      onPickColor={(c) => dispatch("set-bg-color", { color: c })}
      onReset={() => dispatch("reset-bg-color")}
    />
  );
}

function ColorPopover({
  label,
  testId,
  icon,
  stripColor,
  swatches,
  onPickColor,
  onReset,
}: {
  label: string;
  testId: string;
  icon: React.ReactNode;
  stripColor: string;
  swatches: { value: string | null; label: string }[];
  onPickColor: (color: string) => void;
  onReset: () => void;
}) {
  return (
    <Popover.Root>
      <Popover.Trigger asChild>
        <button
          type="button"
          aria-label={label}
          title={label}
          data-testid={testId}
          style={{
            ...iconButtonStyle(false, false),
            flexDirection: "column",
            gap: 1,
            width: 30,
            padding: "2px 0",
          }}
        >
          {icon}
          <span
            aria-hidden
            style={{
              width: 18,
              height: 3,
              borderRadius: 1,
              background: stripColor,
            }}
          />
        </button>
      </Popover.Trigger>
      <Popover.Portal>
        <Popover.Content
          align="start"
          sideOffset={4}
          style={{
            ...menuStyle(),
            padding: 10,
            display: "grid",
            gridTemplateColumns: "repeat(7, 22px)",
            gap: 6,
            width: "auto",
          }}
        >
          {swatches.map(({ value, label: swatchLabel }, i) => (
            <button
              key={i}
              type="button"
              aria-label={swatchLabel}
              title={swatchLabel}
              onClick={() => {
                if (value == null) onReset();
                else onPickColor(value);
              }}
              data-testid={`${testId}-swatch-${i}`}
              style={{
                width: 22,
                height: 22,
                borderRadius: 4,
                border:
                  value == null
                    ? "1px dashed var(--line)"
                    : "1px solid rgba(0,0,0,0.1)",
                background: value ?? "transparent",
                cursor: "pointer",
                padding: 0,
                position: "relative",
              }}
            >
              {value == null && (
                <span
                  aria-hidden
                  style={{
                    position: "absolute",
                    inset: 0,
                    display: "flex",
                    alignItems: "center",
                    justifyContent: "center",
                    fontSize: 10,
                    color: "var(--muted)",
                  }}
                >
                  ⊘
                </span>
              )}
            </button>
          ))}
        </Popover.Content>
      </Popover.Portal>
    </Popover.Root>
  );
}

function MergeButton({
  dispatch,
}: {
  dispatch: (cmd: Command, args?: CommandArgs) => void;
}) {
  const [hovered, setHovered] = useState(false);
  return (
    <button
      type="button"
      onClick={() => dispatch("merge")}
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
      aria-label="Merge cells"
      title="Merge cells"
      data-testid="sheet-tool-merge"
      style={iconButtonStyle(false, hovered)}
    >
      <Merge size={15} strokeWidth={1.8} />
    </button>
  );
}

// ── style helpers ─────────────────────────────────────────────────────

function iconButtonStyle(active: boolean, hovered: boolean): CSSProperties {
  return {
    width: 30,
    height: 30,
    display: "inline-flex",
    alignItems: "center",
    justifyContent: "center",
    border: "1px solid transparent",
    borderRadius: 6,
    background: active
      ? "var(--accent-muted, #e8eaed)"
      : hovered
        ? "var(--bg-hover)"
        : "transparent",
    color: active ? "var(--accent, #1a73e8)" : "var(--text)",
    cursor: "pointer",
    padding: 0,
    flexShrink: 0,
  };
}

function pickerButtonStyle(width: number): CSSProperties {
  return {
    display: "inline-flex",
    alignItems: "center",
    gap: 4,
    height: 28,
    padding: "0 8px",
    border: "1px solid var(--line)",
    borderRadius: 6,
    background: "var(--card)",
    cursor: "pointer",
    fontSize: "var(--text-sm)",
    color: "var(--text)",
    width,
    flexShrink: 0,
  };
}

function smallIconBtn(): CSSProperties {
  return {
    width: 22,
    height: 22,
    display: "inline-flex",
    alignItems: "center",
    justifyContent: "center",
    border: "none",
    borderRadius: 4,
    background: "transparent",
    cursor: "pointer",
    color: "var(--muted)",
    padding: 0,
  };
}

function menuStyle(): CSSProperties {
  return {
    minWidth: 160,
    background: "var(--card)",
    border: "1px solid var(--line)",
    borderRadius: 8,
    boxShadow: "var(--shadow-md, 0 8px 24px rgba(0,0,0,.08))",
    padding: 4,
    fontSize: "var(--text-sm)",
    maxHeight: 320,
    overflowY: "auto",
  };
}

function menuItemStyle(selected: boolean): CSSProperties {
  return {
    padding: "6px 10px",
    borderRadius: 4,
    cursor: "pointer",
    color: "var(--text)",
    background: selected ? "var(--accent-muted, #e8eaed)" : "transparent",
    outline: "none",
  };
}
