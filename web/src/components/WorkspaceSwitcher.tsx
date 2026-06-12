/**
 * Workspace switcher dropdown. Spec: docs/ux/13-workspaces-surface.md.
 *
 * Real Radix DropdownMenu that lists every workspace the user is a
 * member of, groups by kind (Personal / Team), and exposes a
 * "+ Create team workspace" footer that opens a tiny inline form.
 *
 * Active workspace state lives in WorkspaceContext (backed by
 * localStorage `cd-workspace-id-v1`). Picking a workspace pushes the
 * id into the context, which re-scopes the Files list, search, upload,
 * and folder-create requests in lock-step.
 */
import { useEffect, useRef, useState } from "react";
import { Building2, Check, ChevronDown, Plus, UserPlus } from "lucide-react";
import { DropdownMenu } from "radix-ui";
import { toast } from "sonner";

import {
  ApiError,
  createWorkspace,
  DEMO_MODE,
  listWorkspaces,
  type Workspace,
} from "../api/client.ts";
import { useActiveWorkspaceId, useWorkspaceMutator } from "../state/WorkspaceContext.tsx";
import { InviteDialog } from "./InviteDialog.tsx";

export function WorkspaceSwitcher({ onChange }: { onChange?: (w: Workspace) => void }) {
  const [list, setList] = useState<Workspace[] | null>(null);
  const currentId = useActiveWorkspaceId();
  const setActive = useWorkspaceMutator();
  const [open, setOpen] = useState(false);
  const [creating, setCreating] = useState(false);
  const [newName, setNewName] = useState("");
  const newRef = useRef<HTMLInputElement | null>(null);
  // MU1 Phase 1b — invite dialog state. Triggered by the footer
  // "Invite to <Workspace>" item. Personal workspaces don't get the
  // invite affordance (they're 1:1 with a user).
  const [inviting, setInviting] = useState<Workspace | null>(null);

  useEffect(() => {
    void refresh(true);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  async function refresh(initial = false) {
    try {
      const r = await listWorkspaces();
      setList(r.workspaces);
      if (!currentId || !r.workspaces.some((w) => w.id === currentId)) {
        const next = r.current_id || r.workspaces[0]?.id || null;
        setActive(next);
        if (initial && next && onChange) {
          const w = r.workspaces.find((x) => x.id === next);
          if (w) onChange(w);
        }
      }
    } catch (e) {
      const err = e as ApiError;
      if (err.status !== 401) {
        // Silent on 401 — the auth bootstrap handles that path.
        console.warn("workspace list failed", err.message);
      }
    }
  }

  function pick(w: Workspace) {
    setActive(w.id);
    setOpen(false);
    onChange?.(w);
  }

  async function submitCreate() {
    const name = newName.trim();
    if (name.length < 2) return;
    try {
      const w = await createWorkspace(name);
      setList((prev) => (prev ? [...prev, w] : [w]));
      setNewName("");
      setCreating(false);
      pick(w);
      toast.success(`Created workspace "${w.name}"`);
    } catch (err) {
      const e = err as ApiError;
      const body = e.body as { error?: string } | null;
      toast.error(body?.error ?? "Couldn't create workspace");
    }
  }

  const current = list?.find((w) => w.id === currentId) ?? list?.[0] ?? null;
  const personal = list?.filter((w) => w.kind === "personal") ?? [];
  const team = list?.filter((w) => w.kind === "team") ?? [];

  return (
    <DropdownMenu.Root open={open} onOpenChange={setOpen}>
      <DropdownMenu.Trigger asChild>
        <button type="button" style={triggerStyle()} aria-label="Switch workspace">
          <span style={iconBox()}>
            <Building2 size={14} strokeWidth={1.8} />
          </span>
          <span
            style={{
              flex: 1,
              minWidth: 0,
              overflow: "hidden",
              textOverflow: "ellipsis",
              whiteSpace: "nowrap",
            }}
          >
            {current?.name ?? "Personal"}
          </span>
          <ChevronDown size={14} style={{ color: "var(--rail-muted)", flexShrink: 0 }} />
        </button>
      </DropdownMenu.Trigger>
      <DropdownMenu.Portal>
        <DropdownMenu.Content align="start" sideOffset={6} style={menuStyle()}>
          {personal.length > 0 && (
            <>
              <Label>Personal</Label>
              {personal.map((w) => (
                <Item key={w.id} ws={w} active={w.id === currentId} onSelect={() => pick(w)} />
              ))}
            </>
          )}
          {team.length > 0 && (
            <>
              <Label>Team</Label>
              {team.map((w) => (
                <Item key={w.id} ws={w} active={w.id === currentId} onSelect={() => pick(w)} />
              ))}
            </>
          )}

          <Sep />

          {creating ? (
            <form
              onSubmit={(e) => {
                e.preventDefault();
                void submitCreate();
              }}
              style={{ padding: "8px 8px 4px" }}
            >
              <input
                ref={newRef}
                value={newName}
                onChange={(e) => setNewName(e.target.value)}
                placeholder="Workspace name"
                autoFocus
                style={{
                  width: "100%",
                  padding: "8px 10px",
                  fontFamily: "var(--font-sans)",
                  fontSize: "var(--text-sm)",
                  color: "var(--ink)",
                  background: "var(--paper)",
                  border: "1px solid var(--line-strong)",
                  borderRadius: 8,
                  outline: "none",
                }}
              />
              <div style={{ display: "flex", justifyContent: "flex-end", gap: 6, marginTop: 8 }}>
                <button
                  type="button"
                  onClick={() => setCreating(false)}
                  style={ghostBtn()}
                >
                  Cancel
                </button>
                <button type="submit" disabled={newName.trim().length < 2} style={primaryBtn()}>
                  Create
                </button>
              </div>
            </form>
          ) : (
            <DropdownMenu.Item
              onSelect={(e) => {
                e.preventDefault();
                setCreating(true);
                setTimeout(() => newRef.current?.focus(), 30);
              }}
              style={createItemStyle()}
              onMouseEnter={(ev) => (ev.currentTarget.style.background = "var(--bg-hover)")}
              onMouseLeave={(ev) => (ev.currentTarget.style.background = "transparent")}
            >
              <Plus size={14} strokeWidth={1.8} style={{ color: "var(--muted)" }} />
              Create team workspace
            </DropdownMenu.Item>
          )}

          {/* MU1 Phase 1b — invite-to-workspace footer entry. Hidden
              in DEMO_MODE (no backend) and on personal workspaces
              (1-to-1 with a user — invitations don't make sense). */}
          {!DEMO_MODE && current && current.kind === "team" && (
            <DropdownMenu.Item
              onSelect={(e) => {
                e.preventDefault();
                setInviting(current);
                setOpen(false);
              }}
              style={createItemStyle()}
              onMouseEnter={(ev) => (ev.currentTarget.style.background = "var(--bg-hover)")}
              onMouseLeave={(ev) => (ev.currentTarget.style.background = "transparent")}
            >
              <UserPlus size={14} strokeWidth={1.8} style={{ color: "var(--muted)" }} />
              Invite to {current.name}
            </DropdownMenu.Item>
          )}
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
      <InviteDialog
        workspace={inviting}
        open={inviting !== null}
        onClose={() => setInviting(null)}
      />
    </DropdownMenu.Root>
  );
}

function Item({
  ws,
  active,
  onSelect,
}: {
  ws: Workspace;
  active: boolean;
  onSelect: () => void;
}) {
  return (
    <DropdownMenu.Item
      onSelect={(e) => {
        e.preventDefault();
        onSelect();
      }}
      style={itemStyle()}
      onMouseEnter={(ev) => (ev.currentTarget.style.background = "var(--bg-hover)")}
      onMouseLeave={(ev) => (ev.currentTarget.style.background = "transparent")}
    >
      <span style={iconBox(true)}>
        <Building2 size={12} strokeWidth={1.8} />
      </span>
      <span style={{ flex: 1, minWidth: 0 }}>
        <span style={{ display: "block", fontSize: "var(--text-sm)", color: "var(--ink)" }}>
          {ws.name}
        </span>
        <span style={{ display: "block", fontSize: 11, color: "var(--muted)" }}>
          {ws.role === "owner" ? "Owner" : "Member"}
          {ws.member_count > 1 && ` · ${ws.member_count} members`}
        </span>
      </span>
      {active && <Check size={13} strokeWidth={2.2} style={{ color: "var(--accent)" }} />}
    </DropdownMenu.Item>
  );
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
    <DropdownMenu.Separator
      style={{ height: 1, background: "var(--line)", margin: "4px 6px" }}
    />
  );
}

function triggerStyle(): React.CSSProperties {
  // The switcher sits inside the dark sidebar rail, so it has its
  // own "raised" surface (`--rail-2`) instead of the bright `--card`.
  // The popover (menuStyle below) still uses `--card` because it
  // floats above the workspace, not inside the rail.
  return {
    display: "flex",
    alignItems: "center",
    gap: 10,
    width: "100%",
    padding: "10px 12px",
    background: "var(--rail-2)",
    border: "1px solid var(--rail-line)",
    borderRadius: 10,
    cursor: "pointer",
    fontFamily: "var(--font-sans)",
    fontSize: "var(--text-sm)",
    fontWeight: 500,
    color: "var(--rail-active-text)",
    textAlign: "left",
    transition: "background 150ms, border-color 150ms",
  };
}

function iconBox(small = false): React.CSSProperties {
  const sz = small ? 20 : 24;
  return {
    width: sz,
    height: sz,
    borderRadius: small ? 5 : 6,
    // Cyan-tinted square on the rail; switches back to `--ink` when
    // the same `<iconBox>` renders inside the dropdown popover (which
    // is on the bright `--card`).
    background: "var(--accent)",
    color: "var(--fg-onAccent)",
    display: "flex",
    alignItems: "center",
    justifyContent: "center",
    flexShrink: 0,
  };
}

function menuStyle(): React.CSSProperties {
  return {
    width: 248,
    background: "var(--card)",
    border: "1px solid var(--line)",
    borderRadius: 13,
    boxShadow: "var(--shadow-hover)",
    padding: 6,
    fontFamily: "var(--font-sans)",
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

function createItemStyle(): React.CSSProperties {
  return {
    display: "flex",
    alignItems: "center",
    gap: 9,
    padding: "9px 10px",
    borderRadius: 8,
    cursor: "pointer",
    fontSize: "var(--text-sm)",
    color: "var(--ink-soft)",
    userSelect: "none",
    outline: "none",
    transition: "background 120ms",
  };
}

function ghostBtn(): React.CSSProperties {
  return {
    padding: "5px 10px",
    fontSize: "var(--text-xs)",
    background: "transparent",
    border: "1px solid var(--line)",
    borderRadius: 7,
    cursor: "pointer",
    fontWeight: 500,
  };
}

function primaryBtn(): React.CSSProperties {
  return {
    padding: "5px 12px",
    fontSize: "var(--text-xs)",
    background: "var(--ink)",
    color: "var(--paper)",
    border: "none",
    borderRadius: 7,
    cursor: "pointer",
    fontWeight: 500,
  };
}
