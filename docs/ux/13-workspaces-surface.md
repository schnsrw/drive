# 13 — Workspaces + ownership

Companion to `02-surface-v2.md` + `03-settings-surface.md`. Closes pipeline §8 (multi-user / teams) **phase 1** — every user gets a Personal workspace plus zero-or-more Team workspaces, and Team workspace owners can transfer ownership.

> **Phase 1 scope** (this session): model + sidebar switcher + Settings → Workspace card + ownership transfer between known users. Files stay `owner_id`-scoped in queries; full workspace-scoped permission checks + the `Personal/Team` filter on every list ship in Phase 2.

## Tiers

1. **Personal workspace** — auto-created on user creation, 1-to-1 with the user, can never be renamed away from "Personal", deleted, or transferred. Always the default after sign-in. Holds the user's "My Drive" today.
2. **Team workspace** — created explicitly. Exactly one **Owner**, optional **Members**. v0.1.5 ships Owner+Member roles only; Owner/Admin/Editor/Viewer split lands in v0.2 alongside invitations.

## Backend contract

### `GET /api/workspaces` (authed)

```json
{
  "current_id": "wsp_personal_admin",
  "workspaces": [
    { "id": "wsp_personal_admin", "name": "Personal", "kind": "personal",
      "owner_id": "usr_admin", "role": "owner", "member_count": 1,
      "created_at": "…" },
    { "id": "wsp_eng",            "name": "Engineering", "kind": "team",
      "owner_id": "usr_admin", "role": "owner", "member_count": 1,
      "created_at": "…" }
  ]
}
```

- Returns every workspace the caller is a member of, plus a `current_id` hint (server-side just returns the Personal id; the SPA persists the user's chosen workspace in `localStorage` and overrides this).
- `role` is the caller's role *in that workspace*: `owner` or `member`.

### `POST /api/workspaces` (authed)

```json
{ "name": "Engineering" }
```

→ **201** with the created workspace. Caller is auto-inserted as Owner. Name must be 2–60 chars, trimmed. Always `kind: "team"`.

### `POST /api/workspaces/{id}/transfer` (authed, owner-only)

```json
{ "new_owner_id": "usr_alice" }
```

- **204** — transfer is an atomic DB transaction: existing Owner becomes Member, new Owner becomes Owner. Audit-emit `workspace.transfer_owner` with both ids in metadata.
- **403** — caller isn't the current Owner.
- **404** — workspace missing.
- **422** — the target user isn't currently a Member of the workspace (you can only transfer to someone already in).
- **409** — refused on Personal workspaces (which can never be transferred).

### `/api/me` adds

```json
{
  "...existing fields": "...",
  "personal_workspace_id": "wsp_personal_admin"
}
```

## Sidebar switcher

The existing "Personal" placeholder in `Sidebar.tsx` becomes a real Radix DropdownMenu:

```
┌─ Sidebar switcher ────────────────────────────┐
│  [W] Personal               ▾                 │
└───────────────────────────────────────────────┘
            ↓ click
┌─ Workspaces ──────────────────────────────────┐
│  PERSONAL                                      │
│  [W] Personal               · Owner            │
│  TEAM                                          │
│  [W] Engineering            · Owner            │
│  [W] Marketing              · Member           │
│  ──────────                                    │
│  ＋ Create team workspace                       │
└───────────────────────────────────────────────┘
```

- Trigger: the existing pill in the sidebar (currently shows "Personal" hard-coded).
- Selection persists in `localStorage` as `cd-current-workspace-v1`. The SPA's bootstrap reads it and falls back to `personal_workspace_id` from `/api/me`.
- "+ Create team workspace" opens a small dialog: name + Create. On success the new workspace becomes current and the dropdown closes.

## Settings → Workspace card

Replaces the "Coming in v0.2" panel under Settings → Workspace. For the currently-selected workspace:

- Header: name, kind pill, member count.
- **Rename** (Owner only). Inline edit, Save / Cancel.
- **Member list** (Owner only). v0.1.5 just shows the seeded membership; v0.2 lights up the add/remove UI.
- **Transfer ownership** (Owner only, Team workspaces only). Picker of *other Members*. v0.1.5 shows "Need at least one other Member to transfer" until invitations land.
- **Leave workspace** (Members on Team workspaces). Personal workspaces don't show this.
- **Delete workspace** (Owner only, Team workspaces only). Confirms with the workspace name. Deletes the workspace row + memberships; files are NOT deleted in Phase 1 (Phase 2 wires `workspace_id` on files and handles re-homing).

## Audit events added

| Action | Actor | Target |
|---|---|---|
| `workspace.create` | user | workspace |
| `workspace.rename` | user | workspace |
| `workspace.transfer_owner` | user | workspace (metadata: from_user_id, to_user_id) |
| `workspace.delete` | user | workspace |

## Out of scope (Phase 2 / v0.2)

- File / folder rows scoped by `workspace_id` in queries (and the migration that backfills + adds the column).
- Invitations + magic-link onboarding for new members.
- Admin / Editor / Viewer roles (v0.1.5 is just Owner / Member).
- Personal-workspace quotas separate from team quotas.
- Sharing files **across** workspaces (today share-links are token-scoped; that stays the cross-workspace primitive).
