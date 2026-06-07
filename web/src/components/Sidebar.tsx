import { useState } from "react";
import {
  Activity,
  Clock,
  FolderClosed,
  Gauge,
  Home,
  Plus,
  Settings,
  Share2,
  ShieldCheck,
  Star,
  Trash2,
  Upload,
  Users,
} from "lucide-react";

import { Logo, Wordmark } from "./Logo.tsx";
import { WorkspaceSwitcher as RealWorkspaceSwitcher } from "./WorkspaceSwitcher.tsx";

export type NavId =
  | "home"
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
  username,
  storage,
}: {
  current: NavId;
  onSelect: (id: NavId) => void;
  itemCount: number;
  onNewFolder: () => void;
  onUpload: () => void;
  username: string;
  storage?: { usedBytes: number; quotaBytes?: number };
}) {
  const [menuOpen, setMenuOpen] = useState(false);

  return (
    <aside
      style={{
        width: 248,
        flexShrink: 0,
        height: "100vh",
        padding: "22px 16px",
        display: "flex",
        flexDirection: "column",
        gap: 6,
        background: "var(--paper)",
        borderRight: "1px solid var(--line)",
      }}
    >
      <div style={{ display: "flex", alignItems: "center", gap: 12, padding: "4px 8px 16px" }}>
        <div style={{ color: "var(--ink)" }}>
          <Logo size={36} />
        </div>
        <Wordmark />
      </div>

      <RealWorkspaceSwitcher />

      <div style={{ position: "relative", marginTop: 12, marginBottom: 14 }}>
        <button
          type="button"
          onClick={() => setMenuOpen((v) => !v)}
          aria-expanded={menuOpen}
          style={{
            display: "flex",
            alignItems: "center",
            gap: 10,
            width: "100%",
            border: "none",
            cursor: "pointer",
            background: "var(--ink)",
            color: "var(--paper)",
            fontFamily: "var(--font-sans)",
            fontSize: "var(--text-md)",
            fontWeight: 500,
            padding: "13px 16px",
            borderRadius: 14,
            transition: "transform 250ms var(--ease), box-shadow 250ms",
          }}
          onMouseOver={(e) => {
            e.currentTarget.style.transform = "translateY(-1px)";
            e.currentTarget.style.boxShadow = "var(--shadow-button)";
          }}
          onMouseOut={(e) => {
            e.currentTarget.style.transform = "";
            e.currentTarget.style.boxShadow = "";
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

      <AvatarRow username={username} />
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
          color: "var(--muted-2)",
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
          display: "flex",
          alignItems: "center",
          gap: 12,
          width: "100%",
          padding: "9px 12px",
          borderRadius: 11,
          background: active ? "var(--ink)" : "transparent",
          color: active ? "var(--paper)" : "var(--ink-soft)",
          border: "none",
          cursor: "pointer",
          fontFamily: "var(--font-sans)",
          fontSize: "var(--text-md)",
          fontWeight: active ? 500 : 400,
          textAlign: "left",
          transition: "background 180ms, color 180ms",
        }}
        onMouseOver={(e) => {
          if (!active) e.currentTarget.style.background = "var(--bg-hover)";
        }}
        onMouseOut={(e) => {
          if (!active) e.currentTarget.style.background = "transparent";
        }}
      >
        <Icon size={17} strokeWidth={1.7} style={{ opacity: active ? 1 : 0.7 }} />
        <span style={{ flex: 1 }}>{item.label}</span>
        {item.comingSoon && (
          <span
            style={{
              fontSize: 10,
              letterSpacing: "1px",
              textTransform: "uppercase",
              color: active ? "rgba(242,240,234,.5)" : "var(--muted-2)",
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
              color: active ? "rgba(242,240,234,.6)" : "var(--muted)",
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
}: {
  onClose: () => void;
  onNewFolder: () => void;
  onUpload: () => void;
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
        background: "var(--card)",
        border: "1px solid var(--line)",
        borderRadius: "var(--radius)",
        padding: 14,
        marginBottom: 8,
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
        <span style={{ fontSize: "var(--text-sm)", fontWeight: 500 }}>Storage</span>
        {pct !== null && (
          <span
            className="tabular-nums"
            style={{
              fontFamily: "var(--font-display)",
              fontSize: "var(--text-sm)",
              color: "var(--muted)",
            }}
          >
            {pct}%
          </span>
        )}
      </div>
      <div style={{ height: 6, borderRadius: 6, background: "rgba(26,26,30,.08)", overflow: "hidden" }}>
        <div
          style={{
            display: "block",
            height: "100%",
            width: pct !== null ? `${pct}%` : 0,
            borderRadius: 6,
            background: "linear-gradient(90deg, var(--ink), #4a4a52)",
            transition: "width 1200ms var(--ease)",
          }}
        />
      </div>
      <div style={{ fontSize: "var(--text-xs)", color: "var(--muted)", marginTop: 9 }}>
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
        borderRadius: 11,
        cursor: "pointer",
        textAlign: "left",
        transition: "background 150ms",
      }}
      onMouseOver={(e) => (e.currentTarget.style.background = "var(--bg-hover)")}
      onMouseOut={(e) => (e.currentTarget.style.background = "transparent")}
    >
      <span
        style={{
          width: 32,
          height: 32,
          borderRadius: "50%",
          background: "linear-gradient(135deg, #2b2b32, #55555f)",
          color: "var(--paper)",
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          fontFamily: "var(--font-display)",
          fontWeight: 500,
          fontSize: "var(--text-sm)",
          flexShrink: 0,
          border: "2px solid var(--card)",
          boxShadow: "0 0 0 1px var(--line)",
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
            color: "var(--ink)",
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
            color: "var(--muted)",
          }}
        >
          <ShieldCheck size={11} strokeWidth={1.8} />
          Admin
        </span>
      </span>
      <Users size={14} style={{ color: "var(--muted-2)" }} />
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
