/**
 * Settings surface — two-column shell (section nav + content pane).
 *
 * Spec: docs/ux/03-settings-surface.md.
 * Section list locked by PIPELINE.md §9. v0 builds Account / Storage / About
 * for real; the other seven ship as polished ComingSoon panels.
 */
import { useState } from "react";
import {
  Activity,
  Bell,
  Building2,
  Database,
  Info,
  Key,
  Share2,
  ShieldCheck,
  Users,
  UserCircle,
} from "lucide-react";

import { ComingSoon } from "../components/ComingSoon.tsx";
import { AccountSection } from "./settings/AccountSection.tsx";
import { AboutSection } from "./settings/AboutSection.tsx";
import { MembersSection } from "./settings/MembersSection.tsx";
import { StorageSection } from "./settings/StorageSection.tsx";

type SectionId =
  | "account"
  | "workspace"
  | "members"
  | "roles"
  | "sharing"
  | "storage"
  | "notifications"
  | "tokens"
  | "audit"
  | "about";

interface SectionDef {
  id: SectionId;
  label: string;
  icon: typeof UserCircle;
  group: "you" | "team" | "system";
}

const SECTIONS: SectionDef[] = [
  { id: "account", label: "Account", icon: UserCircle, group: "you" },
  { id: "workspace", label: "Workspace", icon: Building2, group: "team" },
  { id: "members", label: "Members", icon: Users, group: "team" },
  { id: "roles", label: "Roles & permissions", icon: ShieldCheck, group: "team" },
  { id: "sharing", label: "Sharing", icon: Share2, group: "team" },
  { id: "storage", label: "Storage", icon: Database, group: "system" },
  { id: "notifications", label: "Notifications", icon: Bell, group: "system" },
  { id: "tokens", label: "API tokens", icon: Key, group: "system" },
  { id: "audit", label: "Audit log", icon: Activity, group: "system" },
  { id: "about", label: "About", icon: Info, group: "system" },
];

const GROUPS: { id: "you" | "team" | "system"; label: string }[] = [
  { id: "you", label: "You" },
  { id: "team", label: "Workspace" },
  { id: "system", label: "System" },
];

export function Settings() {
  const [current, setCurrent] = useState<SectionId>("account");
  const currentDef = SECTIONS.find((s) => s.id === current)!;

  return (
    <div
      style={{
        flex: 1,
        display: "grid",
        gridTemplateColumns: "240px 1fr",
        background: "var(--paper)",
        minHeight: 0,
      }}
    >
      <SectionNav current={current} onSelect={setCurrent} />
      <ContentPane>{renderSection(currentDef)}</ContentPane>
    </div>
  );
}

function SectionNav({
  current,
  onSelect,
}: {
  current: SectionId;
  onSelect: (id: SectionId) => void;
}) {
  return (
    <nav
      aria-label="Settings sections"
      style={{
        borderRight: "1px solid var(--line)",
        padding: "32px 14px 24px",
        overflowY: "auto",
        display: "flex",
        flexDirection: "column",
        gap: 4,
      }}
    >
      <h1
        style={{
          margin: "0 12px 22px",
          fontFamily: "var(--font-display)",
          fontWeight: 500,
          fontSize: "var(--text-2xl)",
          letterSpacing: "var(--tracking-tight)",
          color: "var(--ink)",
        }}
      >
        Settings
      </h1>

      {GROUPS.map((g, gi) => {
        const items = SECTIONS.filter((s) => s.group === g.id);
        if (items.length === 0) return null;
        return (
          <div key={g.id} style={{ marginTop: gi === 0 ? 0 : 18 }}>
            <span
              style={{
                display: "block",
                fontSize: 10,
                letterSpacing: "2.5px",
                textTransform: "uppercase",
                color: "var(--muted-2)",
                fontWeight: 600,
                padding: "0 12px 6px",
              }}
            >
              {g.label}
            </span>
            <ul style={{ listStyle: "none", margin: 0, padding: 0, display: "flex", flexDirection: "column", gap: 2 }}>
              {items.map((s) => (
                <li key={s.id}>
                  <NavItem def={s} active={current === s.id} onClick={() => onSelect(s.id)} />
                </li>
              ))}
            </ul>
          </div>
        );
      })}
    </nav>
  );
}

function NavItem({
  def,
  active,
  onClick,
}: {
  def: SectionDef;
  active: boolean;
  onClick: () => void;
}) {
  const Icon = def.icon;
  return (
    <button
      type="button"
      onClick={onClick}
      style={{
        display: "flex",
        alignItems: "center",
        gap: 11,
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
      <Icon size={16} strokeWidth={1.7} style={{ opacity: active ? 1 : 0.7 }} />
      <span>{def.label}</span>
    </button>
  );
}

function ContentPane({ children }: { children: React.ReactNode }) {
  return (
    <div
      style={{
        overflowY: "auto",
        padding: "40px 56px 80px",
      }}
    >
      <div style={{ maxWidth: 760, margin: "0 auto" }}>{children}</div>
    </div>
  );
}

function renderSection(def: SectionDef): React.ReactNode {
  switch (def.id) {
    case "account":
      return <AccountSection />;
    case "storage":
      return <StorageSection />;
    case "about":
      return <AboutSection />;
    case "workspace":
      return (
        <ComingSoon
          title="Workspace"
          description="Rename your workspace, change the icon, and set the default visibility for new files."
          bullets={[
            "Workspace name + monogram avatar",
            "Default file visibility (private / link-restricted / org)",
            "Workspace deletion + transfer-of-ownership",
          ]}
        />
      );
    case "members":
      return <MembersSection />;
    case "roles":
      return (
        <ComingSoon
          title="Roles & permissions"
          description="Define custom roles and the per-permission grid that backs them — beyond the four defaults."
          bullets={[
            "Built-in: Owner, Admin, Editor, Viewer",
            "Per-resource grants: file / folder / workspace",
            "Per-action grants: read / write / share / delete",
          ]}
        />
      );
    case "sharing":
      return (
        <ComingSoon
          title="Sharing defaults"
          description="Control the default expiry, default permission level, and password requirement for every new share link."
          bullets={[
            "Default expiry: 7 days / 30 days / never",
            "Default permission: view / comment / edit",
            "Require a password on every new link",
          ]}
        />
      );
    case "notifications":
      return (
        <ComingSoon
          title="Notifications"
          description="Decide what events Drive emails you about — and how often."
          bullets={[
            "Per-event toggle (share / mention / activity / system)",
            "Daily or weekly digest cadence",
            "Per-channel routing (email / webhook)",
          ]}
        />
      );
    case "tokens":
      return (
        <ComingSoon
          title="API tokens"
          description="Issue personal API tokens for scripts, sync clients, and CI. Each token is scoped + revocable."
          bullets={[
            "Per-token scope (read / write / admin)",
            "Per-token expiry + last-used timestamp",
            "Audit log entry on every issue / revoke",
          ]}
        />
      );
    case "audit":
      return (
        <ComingSoon
          title="Audit log"
          description="Tamper-evident event feed for every action — sign-ins, uploads, downloads, shares, deletions, permission changes."
          bullets={[
            "Grouped by day, type-tagged, owner-filterable",
            "Append-only audit_log table — required for compliance",
            "Per-action exportable JSON for downstream SIEMs",
          ]}
        />
      );
  }
}
