import { useState } from "react";
import {
  Activity,
  Clock,
  FileText,
  FolderClosed,
  Gauge,
  Home,
  NotebookPen,
  Plus,
  Settings,
  Share2,
  Sheet,
  ShieldCheck,
  Star,
  Trash2,
  Upload,
  Users,
} from "lucide-react";

import { Logo, Wordmark } from "./Logo.tsx";
import { AvatarStack } from "./AvatarStack.tsx";
import { ThemeToggle } from "./ThemeToggle.tsx";
import { WorkspaceSwitcher as RealWorkspaceSwitcher } from "./WorkspaceSwitcher.tsx";

export type NavId =
  | "home"
  | "notes"
  | "recent"
  | "starred"
  | "shared"
  | "trash"
  | "activity"
  | "settings"
  | "admin";

interface NavItem {
  id: NavId;
  label: string;
  icon: typeof Home;
  badge?: number;
  comingSoon?: boolean;
}

const LIBRARY: NavItem[] = [
  { id: "home", label: "My Drive", icon: Home },
  { id: "notes", label: "Notes", icon: NotebookPen },
  { id: "recent", label: "Recent", icon: Clock, comingSoon: true },
  { id: "starred", label: "Starred", icon: Star, comingSoon: true },
  { id: "shared", label: "Shared", icon: Share2, comingSoon: true },
];

const WORKSPACE: NavItem[] = [
  { id: "activity", label: "Activity", icon: Activity },
  { id: "admin", label: "Admin", icon: Gauge },
];

const SYSTEM: NavItem[] = [
  { id: "trash", label: "Trash", icon: Trash2 },
  { id: "settings", label: "Settings", icon: Settings },
];

export function Sidebar({
  current,
  onSelect,
  itemCount,
  onNewFolder,
  onUpload,
  onNewDocument,
  onNewSpreadsheet,
  username,
  storage,
}: {
  current: NavId;
  onSelect: (id: NavId) => void;
  itemCount: number;
  onNewFolder: () => void;
  onUpload: () => void;
  onNewDocument: () => void;
  onNewSpreadsheet: () => void;
  username: string;
  storage?: { usedBytes: number; quotaBytes?: number };
}) {
  const [menuOpen, setMenuOpen] = useState(false);

  return (
    <aside
      style={{
        // Slate Console (re-skin Phase B) — the rail stays dark even
        // when the rest of the app is in light mode. Tokens shipped
        // in Phase A's tokens.css.
        width: 248,
        flexShrink: 0,
        height: "100vh",
        padding: "22px 16px",
        display: "flex",
        flexDirection: "column",
        gap: 6,
        background: "var(--rail)",
        color: "var(--rail-text)",
        borderRight: "1px solid var(--rail-line)",
      }}
    >
      <div style={{ display: "flex", alignItems: "center", gap: 12, padding: "4px 8px 16px" }}>
        {/* Logo's square fills with `currentColor` and the cloud
            paints with `var(--mark-fg)` so this single wrapper flips
            both. On the rail we want a cyan square + paper-coloured
            cloud — readable, brand-locked. */}
        <div style={{ color: "var(--accent)", ["--mark-fg" as string]: "#FFFFFF" }}>
          <Logo size={36} />
        </div>
        {/* Wordmark inherits via currentColor on the wrapping span. */}
        <div style={{ color: "var(--rail-active-text)" }}>
          <Wordmark tone="rail" />
        </div>
      </div>

      <RealWorkspaceSwitcher />
      <AvatarStack />

      <div style={{ position: "relative", marginTop: 12, marginBottom: 14 }}>
        <button
          type="button"
          onClick={() => setMenuOpen((v) => !v)}
          aria-expanded={menuOpen}
          style={{
            // Slate Console — the New button is the primary cyan
            // fill (formerly ink-on-paper). Its hover lift uses the
            // cyan-tinted `--shadow-button` from Phase A tokens.
            display: "flex",
            alignItems: "center",
            gap: 10,
            width: "100%",
            border: "none",
            cursor: "pointer",
            background: "var(--accent)",
            color: "var(--fg-onAccent)",
            fontFamily: "var(--font-sans)",
            fontSize: "var(--text-md)",
            fontWeight: 500,
            padding: "13px 16px",
            borderRadius: 10,
            transition: "transform 250ms var(--ease), box-shadow 250ms, background 200ms",
          }}
          onMouseOver={(e) => {
            e.currentTarget.style.transform = "translateY(-1px)";
            e.currentTarget.style.boxShadow = "var(--shadow-button)";
            e.currentTarget.style.background = "var(--accent-hover)";
          }}
          onMouseOut={(e) => {
            e.currentTarget.style.transform = "";
            e.currentTarget.style.boxShadow = "";
            e.currentTarget.style.background = "var(--accent)";
          }}
        >
          <Plus size={16} strokeWidth={2} />
          <span>New</span>
        </button>
        {menuOpen && (
          <NewMenu
            onClose={() => setMenuOpen(false)}
            onNewFolder={() => {
              setMenuOpen(false);
              onNewFolder();
            }}
            onUpload={() => {
              setMenuOpen(false);
              onUpload();
            }}
            onNewDocument={() => {
              setMenuOpen(false);
              onNewDocument();
            }}
            onNewSpreadsheet={() => {
              setMenuOpen(false);
              onNewSpreadsheet();
            }}
          />
        )}
      </div>

      <Section label="Library">
        {LIBRARY.map((item) => (
          <NavRow
            key={item.id}
            item={item}
            active={current === item.id}
            badge={item.id === "home" ? itemCount : undefined}
            onClick={() => onSelect(item.id)}
          />
        ))}
      </Section>

      <Section label="Workspace">
        {WORKSPACE.map((item) => (
          <NavRow
            key={item.id}
            item={item}
            active={current === item.id}
            onClick={() => onSelect(item.id)}
          />
        ))}
      </Section>

      <Section label="System">
        {SYSTEM.map((item) => (
          <NavRow
            key={item.id}
            item={item}
            active={current === item.id}
            onClick={() => onSelect(item.id)}
          />
        ))}
      </Section>

      <div style={{ flex: 1 }} />

      {storage && <StorageCard {...storage} />}

      <div style={{ display: "flex", alignItems: "center", gap: 4 }}>
        <div style={{ flex: 1, minWidth: 0 }}>
          <AvatarRow username={username} />
        </div>
        <ThemeToggle />
      </div>
    </aside>
  );
}

// The real switcher lives in WorkspaceSwitcher.tsx; this stub was
// removed once the workspaces API landed.

function Section({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <>
      <span
        style={{
          fontSize: 10,
          letterSpacing: "2.5px",
          textTransform: "uppercase",
          color: "var(--rail-muted)",
          fontWeight: 600,
          padding: "10px 12px 4px",
        }}
      >
        {label}
      </span>
      <ul
        style={{
          listStyle: "none",
          display: "flex",
          flexDirection: "column",
          gap: 2,
          margin: 0,
          padding: 0,
        }}
      >
        {children}
      </ul>
    </>
  );
}

function NavRow({
  item,
  active,
  badge,
  onClick,
}: {
  item: NavItem;
  active: boolean;
  badge?: number;
  onClick: () => void;
}) {
  const Icon = item.icon;
  return (
    <li>
      <button
        type="button"
        onClick={onClick}
        style={{
          // Slate Console rail row. Active = cyan-wash fill +
          // bright-cyan text (`--rail-active-text`); idle = transparent
          // + `--rail-text`. Hover gets a thin white wash so the row
          // affords without flashing colour.
          display: "flex",
          alignItems: "center",
          gap: 12,
          width: "100%",
          padding: "9px 12px",
          borderRadius: 10,
          background: active ? "var(--rail-active)" : "transparent",
          color: active ? "var(--rail-active-text)" : "var(--rail-text)",
          border: "none",
          cursor: "pointer",
          fontFamily: "var(--font-sans)",
          fontSize: "var(--text-md)",
          fontWeight: active ? 500 : 400,
          textAlign: "left",
          transition: "background 180ms, color 180ms",
        }}
        onMouseOver={(e) => {
          if (!active) e.currentTarget.style.background = "rgba(255,255,255,0.05)";
        }}
        onMouseOut={(e) => {
          if (!active) e.currentTarget.style.background = "transparent";
        }}
      >
        <Icon size={17} strokeWidth={1.7} style={{ opacity: active ? 1 : 0.85 }} />
        <span style={{ flex: 1 }}>{item.label}</span>
        {item.comingSoon && (
          <span
            style={{
              fontSize: 10,
              letterSpacing: "1px",
              textTransform: "uppercase",
              color: active ? "var(--rail-muted)" : "var(--rail-muted)",
              fontWeight: 600,
            }}
          >
            soon
          </span>
        )}
        {badge !== undefined && badge > 0 && !item.comingSoon && (
          <span
            className="tabular-nums"
            style={{
              fontSize: "var(--text-sm)",
              color: active ? "var(--rail-active-text)" : "var(--rail-muted)",
              opacity: active ? 0.7 : 1,
            }}
          >
            {badge}
          </span>
        )}
      </button>
    </li>
  );
}

function NewMenu({
  onClose,
  onNewFolder,
  onUpload,
  onNewDocument,
  onNewSpreadsheet,
}: {
  onClose: () => void;
  onNewFolder: () => void;
  onUpload: () => void;
  onNewDocument: () => void;
  onNewSpreadsheet: () => void;
}) {
  return (
    <div
      role="menu"
      onMouseLeave={onClose}
      style={{
        position: "absolute",
        top: "calc(100% + 6px)",
        left: 0,
        width: "100%",
        background: "var(--card)",
        border: "1px solid var(--line)",
        borderRadius: 13,
        boxShadow: "var(--shadow-hover)",
        padding: 6,
        zIndex: 20,
        animation: "cd-menu-in 200ms var(--ease)",
      }}
    >
      <MenuItem icon={<FolderClosed size={16} />} label="New folder" onClick={onNewFolder} />
      <MenuItem icon={<FileText size={16} />} label="New document" onClick={onNewDocument} />
      <MenuItem icon={<Sheet size={16} />} label="New spreadsheet" onClick={onNewSpreadsheet} />
      <MenuItem icon={<Upload size={16} />} label="Upload files" onClick={onUpload} />
      <style>{`
        @keyframes cd-menu-in {
          from { opacity: 0; transform: translateY(-6px); }
          to   { opacity: 1; transform: translateY(0); }
        }
      `}</style>
    </div>
  );
}

function MenuItem({
  icon,
  label,
  onClick,
}: {
  icon: React.ReactNode;
  label: string;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      role="menuitem"
      onClick={onClick}
      style={{
        display: "flex",
        alignItems: "center",
        gap: 11,
        width: "100%",
        border: "none",
        background: "transparent",
        cursor: "pointer",
        padding: "10px 12px",
        borderRadius: 9,
        fontFamily: "var(--font-sans)",
        fontSize: "var(--text-sm)",
        color: "var(--ink)",
        textAlign: "left",
        transition: "background 150ms",
      }}
      onMouseOver={(e) => (e.currentTarget.style.background = "var(--bg-hover)")}
      onMouseOut={(e) => (e.currentTarget.style.background = "transparent")}
    >
      <span style={{ color: "var(--muted)" }}>{icon}</span>
      <span>{label}</span>
    </button>
  );
}

function StorageCard({ usedBytes, quotaBytes }: { usedBytes: number; quotaBytes?: number }) {
  const pct =
    quotaBytes && quotaBytes > 0 ? Math.min(100, Math.round((usedBytes / quotaBytes) * 100)) : null;
  return (
    <div
      style={{
        // Raised-on-rail card: `--rail-2` is slightly lighter than
        // `--rail` so the card lifts off the sidebar background
        // without breaking the dark frame.
        background: "var(--rail-2)",
        border: "1px solid var(--rail-line)",
        borderRadius: "var(--radius)",
        padding: 14,
        marginBottom: 8,
        color: "var(--rail-text)",
      }}
    >
      <div
        style={{
          display: "flex",
          justifyContent: "space-between",
          alignItems: "baseline",
          marginBottom: 10,
        }}
      >
        <span
          style={{
            fontSize: "var(--text-sm)",
            fontWeight: 500,
            color: "var(--rail-active-text)",
          }}
        >
          Storage
        </span>
        {pct !== null && (
          <span
            className="tabular-nums"
            style={{
              fontFamily: "var(--font-display)",
              fontSize: "var(--text-sm)",
              color: "var(--rail-muted)",
            }}
          >
            {pct}%
          </span>
        )}
      </div>
      <div
        style={{
          height: 6,
          borderRadius: 6,
          background: "rgba(255, 255, 255, 0.08)",
          overflow: "hidden",
        }}
      >
        <div
          style={{
            display: "block",
            height: "100%",
            width: pct !== null ? `${pct}%` : 0,
            borderRadius: 6,
            background: "linear-gradient(90deg, var(--accent), var(--accent-bright))",
            transition: "width 1200ms var(--ease)",
          }}
        />
      </div>
      <div style={{ fontSize: "var(--text-xs)", color: "var(--rail-muted)", marginTop: 9 }}>
        {pct !== null
          ? `${formatBytes(usedBytes)} of ${formatBytes(quotaBytes!)} used`
          : `${formatBytes(usedBytes)} used`}
      </div>
    </div>
  );
}

function AvatarRow({ username }: { username: string }) {
  const monogram = username.charAt(0).toUpperCase();
  return (
    <button
      type="button"
      style={{
        display: "flex",
        alignItems: "center",
        gap: 11,
        width: "100%",
        padding: "8px 12px",
        background: "transparent",
        border: "none",
        borderRadius: 10,
        cursor: "pointer",
        textAlign: "left",
        transition: "background 150ms",
      }}
      onMouseOver={(e) => (e.currentTarget.style.background = "rgba(255,255,255,0.05)")}
      onMouseOut={(e) => (e.currentTarget.style.background = "transparent")}
    >
      <span
        style={{
          // Cyan monogram chip on the dark rail — same accent that
          // fills the New button so the user's avatar reads as "yours
          // in this product" rather than a generic stock chip.
          width: 32,
          height: 32,
          borderRadius: "50%",
          background: "linear-gradient(135deg, var(--accent), var(--accent-bright))",
          color: "var(--fg-onAccent)",
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          fontFamily: "var(--font-display)",
          fontWeight: 600,
          fontSize: "var(--text-sm)",
          flexShrink: 0,
          border: "2px solid var(--rail-2)",
          boxShadow: "0 0 0 1px var(--rail-line)",
        }}
      >
        {monogram}
      </span>
      <span style={{ flex: 1, minWidth: 0 }}>
        <span
          style={{
            display: "block",
            fontSize: "var(--text-sm)",
            fontWeight: 500,
            color: "var(--rail-active-text)",
            overflow: "hidden",
            textOverflow: "ellipsis",
            whiteSpace: "nowrap",
          }}
        >
          {username}
        </span>
        <span
          style={{
            display: "flex",
            alignItems: "center",
            gap: 4,
            fontSize: "var(--text-xs)",
            color: "var(--rail-muted)",
          }}
        >
          <ShieldCheck size={11} strokeWidth={1.8} />
          Admin
        </span>
      </span>
      <Users size={14} style={{ color: "var(--rail-muted)" }} />
    </button>
  );
}

function formatBytes(b: number) {
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
