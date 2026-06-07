-- Workspaces + memberships. Spec: docs/ux/13-workspaces-surface.md.
-- Phase 1: every user gets a Personal workspace auto-created on insert;
-- Team workspaces are created via the API. Files / folders stay
-- owner_id-scoped in Phase 1 — the workspace_id column on those tables
-- lands in Phase 2 alongside the permission overhaul.

CREATE TABLE workspaces (
  id          TEXT PRIMARY KEY,
  name        TEXT NOT NULL,
  -- "personal" | "team". Personal workspaces are 1-to-1 with users and
  -- can never be renamed, transferred, or deleted (enforced in app).
  kind        TEXT NOT NULL,
  owner_id    TEXT NOT NULL REFERENCES users(id),
  created_at  TEXT NOT NULL
);
CREATE INDEX workspaces_owner_id_idx ON workspaces(owner_id);

CREATE TABLE workspace_members (
  workspace_id  TEXT NOT NULL REFERENCES workspaces(id),
  user_id       TEXT NOT NULL REFERENCES users(id),
  -- "owner" | "member". v0.1.5 only ships these two roles; the
  -- Admin/Editor/Viewer split lands in v0.2 with invitations.
  role          TEXT NOT NULL,
  joined_at     TEXT NOT NULL,
  PRIMARY KEY (workspace_id, user_id)
);
CREATE INDEX workspace_members_user_id_idx ON workspace_members(user_id);
