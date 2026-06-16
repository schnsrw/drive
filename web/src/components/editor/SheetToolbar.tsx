/**
 * SheetToolbar — Drive-side formatting ribbon for the `.xlsx` editor
 * route. Sits above `<CasualSheetWorkspace mode="editor">` and drives
 * Univer through the SDK's `casual.command.execute` wire (sheet
 * 0.6+). UX-EDITOR-1.
 *
 * Why Drive owns the toolbar instead of the SDK:
 *   - Univer's built-in ribbon needs the ribbon plugins which all
 *     resolve `IRPCChannelService` at construction; the SDK doesn't
 *     bundle a worker that provides it. Enabling the ribbon silently
 *     wedges the workbench mount.
 *   - Drive's design language (Slate paper, cyan accents) belongs to
 *     Drive — the SDK's toolbar would always look "borrowed".
 *
 * v1 surface (matches what the SDK's command wire supports today):
 *   - Undo / Redo
 *   - Bold / Italic / Underline / Strikethrough
 *   - Align Left / Center / Right
 *
 * Font family / size / colour / fill / merge land in v2 once the
 * SDK extends the command-execute payload union.
 */
import { useEffect, useState, type RefObject } from "react";
import {
  AlignCenter,
  AlignLeft,
  AlignRight,
  Bold,
  Italic,
  Redo,
  Strikethrough,
  Underline,
  Undo,
  type LucideIcon,
} from "lucide-react";

import type { CasualSheetsIframeRef } from "@schnsrw/casual-sheets/sheets";

export interface SheetFormatState {
  bold: boolean;
  italic: boolean;
  underline: boolean;
  strikethrough: boolean;
  align: "left" | "center" | "right" | null;
}

type Command =
  | "undo"
  | "redo"
  | "bold"
  | "italic"
  | "underline"
  | "strikethrough"
  | "align-left"
  | "align-center"
  | "align-right";

interface ToolDef {
  cmd: Command;
  Icon: LucideIcon;
  label: string;
  shortcut?: string;
  active?: (s: SheetFormatState) => boolean;
}

const TOOLS: ToolDef[][] = [
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
  [
    {
      cmd: "align-left",
      Icon: AlignLeft,
      label: "Align left",
      active: (s) => s.align === "left",
    },
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
  ],
];

export function SheetToolbar({
  iframeRef,
  formatState,
}: {
  iframeRef: RefObject<CasualSheetsIframeRef | null>;
  formatState: SheetFormatState;
}) {
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
        iframeRef.current?.executeCommand(cmd);
      }
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [iframeRef]);

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
      }}
    >
      {TOOLS.map((group, i) => (
        <ToolGroup
          key={i}
          tools={group}
          formatState={formatState}
          onClick={(cmd) => iframeRef.current?.executeCommand(cmd)}
          divider={i < TOOLS.length - 1}
        />
      ))}
    </div>
  );
}

function ToolGroup({
  tools,
  formatState,
  onClick,
  divider,
}: {
  tools: ToolDef[];
  formatState: SheetFormatState;
  onClick: (cmd: Command) => void;
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
            onClick={() => onClick(t.cmd)}
          />
        ))}
      </div>
      {divider && (
        <span
          aria-hidden
          style={{
            width: 1,
            height: 22,
            background: "var(--line)",
          }}
        />
      )}
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
      style={{
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
      }}
    >
      <tool.Icon size={15} strokeWidth={1.8} />
    </button>
  );
}
