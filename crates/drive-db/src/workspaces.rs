//! Workspaces + memberships. Spec: docs/ux/13-workspaces-surface.md.
//!
//! Phase 1: every user gets a Personal workspace auto-created on
//! `UserRepo::insert`. Team workspaces are created via the API. Roles
//! are just `owner` | `member` for now; v0.2 introduces Admin / Editor /
//! Viewer alongside invitations.

use serde::{Deserialize, Serialize};
use sqlx::Row;

use crate::{
    users::{parse_ts, ts},
    Db, DbError,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WorkspaceKind {
    Personal,
    Team,
}

impl WorkspaceKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Personal => "personal",
            Self::Team => "team",
        }
    }
    fn parse(s: &str) -> Self {
        match s {
            "personal" => Self::Personal,
            _ => Self::Team,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WorkspaceRole {
    Owner,
    Member,
}

impl WorkspaceRole {
    fn as_str(self) -> &'static str {
        match self {
            Self::Owner => "owner",
            Self::Member => "member",
        }
    }
    fn parse(s: &str) -> Self {
        match s {
            "owner" => Self::Owner,
            _ => Self::Member,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workspace {
    pub id: String,
    pub name: String,
    pub kind: WorkspaceKind,
    pub owner_id: String,
    pub created_at: time::OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceWithRole {
    pub id: String,
    pub name: String,
    pub kind: WorkspaceKind,
    pub owner_id: String,
    pub role: WorkspaceRole,
    pub member_count: i64,
    pub created_at: time::OffsetDateTime,
}

#[derive(Debug, Clone)]
pub struct WorkspaceRepo<'a> {
    db: &'a Db,
}

impl<'a> WorkspaceRepo<'a> {
    #[must_use]
    pub fn new(db: &'a Db) -> Self {
        Self { db }
    }

    /// Create a workspace + insert the owner as a member in one shot.
    /// Caller picks the kind. Use this rather than two separate calls so
    /// the invariant "every workspace has at least one Owner row" is
    /// guaranteed even if the second insert raced (single-tenant doesn't
    /// race, but the shape is right).
    pub async fn insert(
        &self,
        name: &str,
        kind: WorkspaceKind,
        owner_id: &str,
    ) -> Result<Workspace, DbError> {
        let id = ulid::Ulid::new().to_string();
        let created_at = time::OffsetDateTime::now_utc();
        let now_s = ts(created_at);
        sqlx::query(
            "INSERT INTO workspaces (id, name, kind, owner_id, created_at) \
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(name)
        .bind(kind.as_str())
        .bind(owner_id)
        .bind(&now_s)
        .execute(self.db.pool())
        .await?;
        sqlx::query(
            "INSERT INTO workspace_members (workspace_id, user_id, role, joined_at) \
             VALUES (?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(owner_id)
        .bind(WorkspaceRole::Owner.as_str())
        .bind(&now_s)
        .execute(self.db.pool())
        .await?;
        Ok(Workspace {
            id,
            name: name.to_string(),
            kind,
            owner_id: owner_id.to_string(),
            created_at,
        })
    }

    pub async fn find_by_id(&self, id: &str) -> Result<Workspace, DbError> {
        let row =
            sqlx::query("SELECT id, name, kind, owner_id, created_at FROM workspaces WHERE id = ?")
                .bind(id)
                .fetch_one(self.db.pool())
                .await
                .map_err(DbError::from_sqlx_no_rows)?;
        Ok(Workspace {
            id: row.get("id"),
            name: row.get("name"),
            kind: WorkspaceKind::parse(row.get::<String, _>("kind").as_str()),
            owner_id: row.get("owner_id"),
            created_at: parse_ts(row.get::<String, _>("created_at"))?,
        })
    }

    /// Every workspace the user is a member of, with their role + a
    /// member count. Newest first.
    pub async fn list_for_user(&self, user_id: &str) -> Result<Vec<WorkspaceWithRole>, DbError> {
        let rows = sqlx::query(
            "SELECT w.id, w.name, w.kind, w.owner_id, w.created_at, m.role, \
                    (SELECT COUNT(*) FROM workspace_members WHERE workspace_id = w.id) AS member_count \
             FROM workspaces w \
             JOIN workspace_members m ON m.workspace_id = w.id \
             WHERE m.user_id = ? \
             ORDER BY w.created_at ASC",
        )
        .bind(user_id)
        .fetch_all(self.db.pool())
        .await?;
        rows.iter()
            .map(|row| {
                Ok(WorkspaceWithRole {
                    id: row.get("id"),
                    name: row.get("name"),
                    kind: WorkspaceKind::parse(row.get::<String, _>("kind").as_str()),
                    owner_id: row.get("owner_id"),
                    role: WorkspaceRole::parse(row.get::<String, _>("role").as_str()),
                    member_count: row.get("member_count"),
                    created_at: parse_ts(row.get::<String, _>("created_at"))?,
                })
            })
            .collect()
    }

    pub async fn rename(&self, id: &str, new_name: &str) -> Result<(), DbError> {
        sqlx::query("UPDATE workspaces SET name = ? WHERE id = ?")
            .bind(new_name)
            .bind(id)
            .execute(self.db.pool())
            .await?;
        Ok(())
    }

    pub async fn delete(&self, id: &str) -> Result<(), DbError> {
        // Memberships first to respect the FK.
        sqlx::query("DELETE FROM workspace_members WHERE workspace_id = ?")
            .bind(id)
            .execute(self.db.pool())
            .await?;
        sqlx::query("DELETE FROM workspaces WHERE id = ?")
            .bind(id)
            .execute(self.db.pool())
            .await?;
        Ok(())
    }

    /// Atomic ownership transfer: existing owner → member, new owner →
    /// owner. Caller must verify `new_owner_id` is already a member and
    /// that the workspace isn't Personal — the repo just runs the swap.
    pub async fn transfer_owner(
        &self,
        id: &str,
        old_owner_id: &str,
        new_owner_id: &str,
    ) -> Result<(), DbError> {
        let mut tx = self.db.pool().begin().await?;
        sqlx::query("UPDATE workspaces SET owner_id = ? WHERE id = ?")
            .bind(new_owner_id)
            .bind(id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE workspace_members SET role = ? WHERE workspace_id = ? AND user_id = ?")
            .bind(WorkspaceRole::Member.as_str())
            .bind(id)
            .bind(old_owner_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE workspace_members SET role = ? WHERE workspace_id = ? AND user_id = ?")
            .bind(WorkspaceRole::Owner.as_str())
            .bind(id)
            .bind(new_owner_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceMembership {
    pub workspace_id: String,
    pub user_id: String,
    pub role: WorkspaceRole,
    pub joined_at: time::OffsetDateTime,
}

#[derive(Debug, Clone)]
pub struct WorkspaceMemberRepo<'a> {
    db: &'a Db,
}

impl<'a> WorkspaceMemberRepo<'a> {
    #[must_use]
    pub fn new(db: &'a Db) -> Self {
        Self { db }
    }

    pub async fn insert(
        &self,
        workspace_id: &str,
        user_id: &str,
        role: WorkspaceRole,
    ) -> Result<WorkspaceMembership, DbError> {
        let joined_at = time::OffsetDateTime::now_utc();
        sqlx::query(
            "INSERT INTO workspace_members (workspace_id, user_id, role, joined_at) \
             VALUES (?, ?, ?, ?)",
        )
        .bind(workspace_id)
        .bind(user_id)
        .bind(role.as_str())
        .bind(ts(joined_at))
        .execute(self.db.pool())
        .await?;
        Ok(WorkspaceMembership {
            workspace_id: workspace_id.to_string(),
            user_id: user_id.to_string(),
            role,
            joined_at,
        })
    }

    pub async fn delete(&self, workspace_id: &str, user_id: &str) -> Result<(), DbError> {
        sqlx::query("DELETE FROM workspace_members WHERE workspace_id = ? AND user_id = ?")
            .bind(workspace_id)
            .bind(user_id)
            .execute(self.db.pool())
            .await?;
        Ok(())
    }

    pub async fn role_of(
        &self,
        workspace_id: &str,
        user_id: &str,
    ) -> Result<Option<WorkspaceRole>, DbError> {
        let row = sqlx::query(
            "SELECT role FROM workspace_members WHERE workspace_id = ? AND user_id = ?",
        )
        .bind(workspace_id)
        .bind(user_id)
        .fetch_optional(self.db.pool())
        .await?;
        Ok(row.map(|r| WorkspaceRole::parse(r.get::<String, _>("role").as_str())))
    }

    pub async fn list(&self, workspace_id: &str) -> Result<Vec<WorkspaceMembership>, DbError> {
        let rows = sqlx::query(
            "SELECT workspace_id, user_id, role, joined_at \
             FROM workspace_members WHERE workspace_id = ? ORDER BY joined_at ASC",
        )
        .bind(workspace_id)
        .fetch_all(self.db.pool())
        .await?;
        rows.iter()
            .map(|row| {
                Ok(WorkspaceMembership {
                    workspace_id: row.get("workspace_id"),
                    user_id: row.get("user_id"),
                    role: WorkspaceRole::parse(row.get::<String, _>("role").as_str()),
                    joined_at: parse_ts(row.get::<String, _>("joined_at"))?,
                })
            })
            .collect()
    }
}
