/**
 * Settings → Members section (MU1 Phase 1c).
 *
 * Spec: [[workspace-invitations]] memory entry §"Phase 1c — Members tab".
 *
 * Two stacked cards:
 *   1. **Active members** — read-only list (owner badge + joined date).
 *      Per-member remove ships with MU2 role tiers.
 *   2. **Pending invitations** — every row from the workspace's
 *      `workspace_invitations` table with its status + a Revoke button
 *      for active ones. Owner clicks "Invite to <Workspace>" in the
 *      WorkspaceSwitcher footer to mint a fresh one (the dialog lives
 *      there, not here, to keep the management surface read-leaning).
 *
 * Personal workspaces show a friendly placeholder — there's no
 * "members" concept when the workspace is 1-to-1 with a user.
 */
import { useCallback, useEffect, useState } from "react";
import { Trash2, Users } from "lucide-react";
import { toast } from "sonner";

import {
  listInvitations,
  listWorkspaceMembers,
  listWorkspaces,
  revokeInvitation,
  type InvitationListEntry,
  type Workspace,
  type WorkspaceMember,
} from "../../api/client.ts";
import { useActiveWorkspaceId } from "../../state/WorkspaceContext.tsx";
import { ConfirmDialog } from "../../components/ConfirmDialog.tsx";

export function MembersSection() {
  const workspaceId = useActiveWorkspaceId();
  const [workspace, setWorkspace] = useState<Workspace | null>(null);
  const [members, setMembers] = useState<WorkspaceMember[] | null>(null);
  const [invitations, setInvitations] = useState<InvitationListEntry[] | null>(null);
  const [confirming, setConfirming] = useState<InvitationListEntry | null>(null);

  const refresh = useCallback(async () => {
    if (!workspaceId) return;
    try {
      const [wsList, m, invs] = await Promise.all([
        listWorkspaces(),
        listWorkspaceMembers(workspaceId),
        listInvitations(workspaceId),
      ]);
      const ws = wsList.workspaces.find((w) => w.id === workspaceId) ?? null;
      setWorkspace(ws);
      setMembers(m.members);
      setInvitations(invs);
    } catch (err) {
      const message = err instanceof Error ? err.message : "Couldn't load workspace members";
      toast.error(message);
    }
  }, [workspaceId]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  async function doRevoke(inv: InvitationListEntry) {
    if (!workspaceId) return;
    try {
      await revokeInvitation(workspaceId, inv.id);
      toast.success("Invitation revoked");
      await refresh();
    } catch (err) {
      const message = err instanceof Error ? err.message : "Revoke failed";
      toast.error(message);
    }
  }

  const isPersonal = workspace?.kind === "personal";

  return (
    <div>
      <Header />

      {isPersonal ? (
        <Card>
          <p
            style={{
              margin: 0,
              fontSize: "var(--text-sm)",
              color: "var(--muted)",
              lineHeight: 1.6,
            }}
          >
            This is your personal workspace — it's just for you. To collaborate, switch to a team
            workspace or create one from the workspace switcher in the sidebar.
          </p>
        </Card>
      ) : (
        <>
          <ActiveMembersCard members={members} workspace={workspace} />
          <PendingInvitationsCard
            invitations={invitations}
            onRevoke={(inv) => setConfirming(inv)}
          />
        </>
      )}

      <ConfirmDialog
        open={confirming !== null}
        title="Revoke this invitation?"
        body="The link will stop admitting new members. Anyone who already accepted stays in the workspace."
        variant="destructive"
        confirmLabel="Revoke"
        onClose={() => setConfirming(null)}
        onConfirm={async () => {
          const target = confirming;
          setConfirming(null);
          if (target) await doRevoke(target);
        }}
      />
    </div>
  );
}

function Header() {
  return (
    <div style={{ marginBottom: 22 }}>
      <h1
        style={{
          margin: 0,
          fontFamily: "var(--font-display)",
          fontSize: "var(--text-2xl)",
          fontWeight: 600,
          color: "var(--ink)",
          letterSpacing: "-0.01em",
        }}
      >
        Members
      </h1>
      <p
        style={{
          margin: "6px 0 0",
          fontSize: "var(--text-sm)",
          color: "var(--muted)",
          lineHeight: 1.55,
        }}
      >
        Who has access to this workspace and which invitations are still live.
      </p>
    </div>
  );
}

function Card({ children }: { children: React.ReactNode }) {
  return (
    <section
      style={{
        marginBottom: 18,
        padding: "18px 20px",
        border: "1px solid var(--line)",
        borderRadius: 14,
        background: "var(--card)",
        boxShadow: "var(--shadow)",
      }}
    >
      {children}
    </section>
  );
}

function CardTitle({
  icon,
  label,
  count,
}: {
  icon: React.ReactNode;
  label: string;
  count: number | null;
}) {
  return (
    <div
      style={{
        display: "flex",
        alignItems: "center",
        gap: 10,
        marginBottom: 14,
        color: "var(--ink)",
      }}
    >
      <span
        aria-hidden="true"
        style={{
          width: 28,
          height: 28,
          borderRadius: 8,
          background: "var(--bg-subtle)",
          display: "inline-flex",
          alignItems: "center",
          justifyContent: "center",
          color: "var(--muted)",
        }}
      >
        {icon}
      </span>
      <span
        style={{
          fontFamily: "var(--font-sans)",
          fontSize: "var(--text-md)",
          fontWeight: 600,
        }}
      >
        {label}
      </span>
      {count !== null && (
        <span
          style={{
            marginLeft: "auto",
            fontSize: "var(--text-xs)",
            color: "var(--muted)",
            background: "var(--bg-subtle)",
            border: "1px solid var(--line)",
            borderRadius: 6,
            padding: "2px 8px",
          }}
        >
          {count}
        </span>
      )}
    </div>
  );
}

function ActiveMembersCard({
  members,
  workspace,
}: {
  members: WorkspaceMember[] | null;
  workspace: Workspace | null;
}) {
  return (
    <Card>
      <CardTitle
        icon={<Users size={14} strokeWidth={1.8} />}
        label="Active"
        count={members?.length ?? null}
      />
      {members === null ? (
        <p style={{ margin: 0, color: "var(--muted)", fontSize: "var(--text-sm)" }}>Loading…</p>
      ) : members.length === 0 ? (
        <p style={{ margin: 0, color: "var(--muted)", fontSize: "var(--text-sm)" }}>
          No members yet.
        </p>
      ) : (
        <ul style={{ listStyle: "none", margin: 0, padding: 0 }}>
          {members.map((m, i) => (
            <MemberRow
              key={m.user_id}
              member={m}
              isOwner={workspace?.owner_id === m.user_id}
              last={i === members.length - 1}
            />
          ))}
        </ul>
      )}
    </Card>
  );
}

function MemberRow({
  member,
  isOwner,
  last,
}: {
  member: WorkspaceMember;
  isOwner: boolean;
  last: boolean;
}) {
  return (
    <li
      style={{
        display: "flex",
        alignItems: "center",
        gap: 12,
        padding: "10px 4px",
        borderBottom: last ? "none" : "1px solid var(--line)",
      }}
    >
      <span
        aria-hidden="true"
        style={{
          width: 28,
          height: 28,
          borderRadius: "50%",
          background: "linear-gradient(135deg, #2b2b32, #55555f)",
          color: "var(--paper)",
          display: "inline-flex",
          alignItems: "center",
          justifyContent: "center",
          fontFamily: "var(--font-display)",
          fontWeight: 500,
          fontSize: "var(--text-sm)",
          flexShrink: 0,
        }}
      >
        {member.username.charAt(0).toUpperCase()}
      </span>
      <span
        style={{
          fontSize: "var(--text-sm)",
          fontWeight: 500,
          color: "var(--ink)",
          flex: 1,
          minWidth: 0,
          overflow: "hidden",
          textOverflow: "ellipsis",
          whiteSpace: "nowrap",
        }}
      >
        {member.username}
      </span>
      <span
        style={{
          fontSize: "var(--text-xs)",
          color: isOwner ? "var(--accent-strong)" : "var(--muted)",
          background: isOwner ? "var(--accent-muted)" : "var(--bg-subtle)",
          border: "1px solid var(--line)",
          borderRadius: 6,
          padding: "2px 8px",
          textTransform: "capitalize",
        }}
      >
        {isOwner ? "Owner" : member.role}
      </span>
    </li>
  );
}

function PendingInvitationsCard({
  invitations,
  onRevoke,
}: {
  invitations: InvitationListEntry[] | null;
  onRevoke: (inv: InvitationListEntry) => void;
}) {
  const active = invitations?.filter((i) => !i.revoked) ?? [];
  return (
    <Card>
      <CardTitle
        icon={<Users size={14} strokeWidth={1.8} />}
        label="Pending invitations"
        count={active.length}
      />
      {invitations === null ? (
        <p style={{ margin: 0, color: "var(--muted)", fontSize: "var(--text-sm)" }}>Loading…</p>
      ) : invitations.length === 0 ? (
        <p style={{ margin: 0, color: "var(--muted)", fontSize: "var(--text-sm)" }}>
          No invitations yet. Generate one from the workspace switcher in the sidebar.
        </p>
      ) : (
        <ul style={{ listStyle: "none", margin: 0, padding: 0 }}>
          {invitations.map((inv, i) => (
            <InvitationRow
              key={inv.id}
              inv={inv}
              last={i === invitations.length - 1}
              onRevoke={() => onRevoke(inv)}
            />
          ))}
        </ul>
      )}
    </Card>
  );
}

function InvitationRow({
  inv,
  last,
  onRevoke,
}: {
  inv: InvitationListEntry;
  last: boolean;
  onRevoke: () => void;
}) {
  const status = computeStatus(inv);
  return (
    <li
      style={{
        display: "flex",
        alignItems: "center",
        gap: 12,
        padding: "10px 4px",
        borderBottom: last ? "none" : "1px solid var(--line)",
      }}
    >
      <div style={{ flex: 1, minWidth: 0 }}>
        <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
          <span
            style={{
              fontSize: "var(--text-sm)",
              fontWeight: 500,
              color: "var(--ink)",
              textTransform: "capitalize",
            }}
          >
            {inv.role}
          </span>
          <StatusBadge status={status} />
        </div>
        <div
          style={{
            fontSize: "var(--text-xs)",
            color: "var(--muted)",
            marginTop: 4,
            display: "flex",
            gap: 8,
            flexWrap: "wrap",
          }}
        >
          <span>
            {inv.used_count} / {inv.max_uses} used
          </span>
          <span aria-hidden>·</span>
          <span>{inv.expires_at ? `Expires ${formatRelative(inv.expires_at)}` : "Never expires"}</span>
          <span aria-hidden>·</span>
          <span>Created {formatRelative(inv.created_at)}</span>
        </div>
      </div>
      {status === "active" && (
        <button
          type="button"
          onClick={onRevoke}
          aria-label="Revoke invitation"
          title="Revoke invitation"
          style={{
            display: "inline-flex",
            alignItems: "center",
            gap: 6,
            padding: "6px 10px",
            borderRadius: 8,
            border: "1px solid var(--line)",
            background: "transparent",
            color: "var(--danger)",
            fontFamily: "var(--font-sans)",
            fontSize: "var(--text-xs)",
            cursor: "pointer",
            transition: "background 120ms, border-color 120ms",
          }}
          onMouseEnter={(e) => {
            e.currentTarget.style.background = "var(--bg-hover)";
            e.currentTarget.style.borderColor = "var(--danger)";
          }}
          onMouseLeave={(e) => {
            e.currentTarget.style.background = "transparent";
            e.currentTarget.style.borderColor = "var(--line)";
          }}
        >
          <Trash2 size={12} strokeWidth={1.8} />
          Revoke
        </button>
      )}
    </li>
  );
}

type Status = "active" | "exhausted" | "expired" | "revoked";

function computeStatus(inv: InvitationListEntry): Status {
  if (inv.revoked) return "revoked";
  if (inv.expires_at && new Date(inv.expires_at).getTime() < Date.now()) return "expired";
  if (inv.used_count >= inv.max_uses) return "exhausted";
  return "active";
}

function StatusBadge({ status }: { status: Status }) {
  const palette: Record<Status, { fg: string; bg: string; label: string }> = {
    active: { fg: "var(--success)", bg: "var(--bg-subtle)", label: "Active" },
    exhausted: { fg: "var(--muted)", bg: "var(--bg-subtle)", label: "Exhausted" },
    expired: { fg: "var(--muted)", bg: "var(--bg-subtle)", label: "Expired" },
    revoked: { fg: "var(--danger)", bg: "var(--bg-subtle)", label: "Revoked" },
  };
  const p = palette[status];
  return (
    <span
      style={{
        fontSize: "var(--text-xs)",
        color: p.fg,
        background: p.bg,
        border: "1px solid var(--line)",
        borderRadius: 6,
        padding: "1px 7px",
        fontWeight: 600,
        letterSpacing: "0.02em",
      }}
    >
      {p.label}
    </span>
  );
}

function formatRelative(iso: string): string {
  const t = new Date(iso).getTime();
  if (Number.isNaN(t)) return "soon";
  const diffMs = t - Date.now();
  const abs = Math.abs(diffMs);
  const past = diffMs < 0;
  const day = 1000 * 60 * 60 * 24;
  const hour = 1000 * 60 * 60;
  const min = 1000 * 60;
  if (abs >= 2 * day) {
    const days = Math.round(abs / day);
    return past ? `${days}d ago` : `in ${days}d`;
  }
  if (abs >= 2 * hour) {
    const hours = Math.round(abs / hour);
    return past ? `${hours}h ago` : `in ${hours}h`;
  }
  if (abs >= 2 * min) {
    const mins = Math.round(abs / min);
    return past ? `${mins}m ago` : `in ${mins}m`;
  }
  return past ? "just now" : "any moment";
}
