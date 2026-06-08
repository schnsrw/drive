//! Users table — single-tenant v0 holds exactly one row (the admin), but the
//! shape grows directly into multi-user without a migration.

use serde::{Deserialize, Serialize};
use sqlx::Row;

use crate::{workspaces::WorkspaceRepo, Db, DbError, WorkspaceKind};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: String,
    pub username: String,
    pub password_hash: String,
    pub is_admin: bool,
    pub created_at: time::OffsetDateTime,
    /// Per-user storage cap in bytes. None = unlimited.
    pub quota_bytes: Option<u64>,
    /// Phase 3 §12 — OIDC anchor fields. `oidc_subject` is `None` for
    /// users created via the local-password / first-run-setup paths.
    /// When set, the (`oidc_provider_id`, `oidc_subject`) tuple uniquely
    /// identifies this user across sign-ins.
    pub oidc_provider_id: Option<String>,
    pub oidc_subject: Option<String>,
    pub oidc_email_verified: bool,
}

#[derive(Debug, Clone)]
pub struct NewUser {
    pub username: String,
    pub password_hash: String,
    pub is_admin: bool,
}

#[derive(Debug, Clone)]
pub struct UserRepo<'a> {
    db: &'a Db,
}

impl<'a> UserRepo<'a> {
    #[must_use]
    pub fn new(db: &'a Db) -> Self {
        Self { db }
    }

    /// Insert a new user. Returns `UniqueViolation` if the username clashes.
    pub async fn insert(&self, new: &NewUser) -> Result<User, DbError> {
        let id = ulid::Ulid::new().to_string();
        let created_at = time::OffsetDateTime::now_utc();
        let created_at_str = ts(created_at);
        let is_admin_i = i64::from(new.is_admin);

        sqlx::query(
            "INSERT INTO users (id, username, password_hash, is_admin, created_at) \
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(&new.username)
        .bind(&new.password_hash)
        .bind(is_admin_i)
        .bind(&created_at_str)
        .execute(self.db.pool())
        .await
        .map_err(map_unique_violation)?;

        // Auto-create the Personal workspace (1-to-1 with the user). Spec:
        // docs/ux/13-workspaces-surface.md. We refuse to ship a user
        // without a workspace, so this is `?`, not `let _`.
        WorkspaceRepo::new(self.db)
            .insert("Personal", WorkspaceKind::Personal, &id)
            .await?;

        Ok(User {
            id,
            username: new.username.clone(),
            password_hash: new.password_hash.clone(),
            is_admin: new.is_admin,
            created_at,
            quota_bytes: None,
            oidc_provider_id: None,
            oidc_subject: None,
            oidc_email_verified: false,
        })
    }

    /// Phase 3 §12 — look up by `(provider_id, subject)`. Returns
    /// `NotFound` if no row.
    pub async fn find_by_oidc(&self, provider_id: &str, subject: &str) -> Result<User, DbError> {
        let row = sqlx::query(
            "SELECT id, username, password_hash, is_admin, created_at, quota_bytes, \
                    oidc_provider_id, oidc_subject, oidc_email_verified \
             FROM users \
             WHERE oidc_provider_id = ? AND oidc_subject = ?",
        )
        .bind(provider_id)
        .bind(subject)
        .fetch_one(self.db.pool())
        .await
        .map_err(DbError::from_sqlx_no_rows)?;
        row_to_user(&row)
    }

    /// Insert a user whose anchor is OIDC, not a password. Uses a sentinel
    /// `oidc:` prefix for `password_hash` so the local sign-in path can
    /// never accept it (Argon2id hashes start with `$argon2`).
    pub async fn insert_oidc(
        &self,
        username: &str,
        is_admin: bool,
        provider_id: &str,
        subject: &str,
        email_verified: bool,
    ) -> Result<User, DbError> {
        let id = ulid::Ulid::new().to_string();
        let created_at = time::OffsetDateTime::now_utc();
        let created_at_str = ts(created_at);
        let is_admin_i = i64::from(is_admin);
        let email_verified_i = i64::from(email_verified);
        let sentinel = "oidc:no-password";

        sqlx::query(
            "INSERT INTO users \
             (id, username, password_hash, is_admin, created_at, \
              oidc_provider_id, oidc_subject, oidc_email_verified) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(username)
        .bind(sentinel)
        .bind(is_admin_i)
        .bind(&created_at_str)
        .bind(provider_id)
        .bind(subject)
        .bind(email_verified_i)
        .execute(self.db.pool())
        .await
        .map_err(map_unique_violation)?;

        WorkspaceRepo::new(self.db)
            .insert("Personal", WorkspaceKind::Personal, &id)
            .await?;

        Ok(User {
            id,
            username: username.to_string(),
            password_hash: sentinel.to_string(),
            is_admin,
            created_at,
            quota_bytes: None,
            oidc_provider_id: Some(provider_id.to_string()),
            oidc_subject: Some(subject.to_string()),
            oidc_email_verified: email_verified,
        })
    }

    /// Attach an OIDC anchor to an existing (local-password) user.
    /// Idempotent — runs the same UPDATE either way.
    pub async fn link_oidc(
        &self,
        id: &str,
        provider_id: &str,
        subject: &str,
        email_verified: bool,
    ) -> Result<(), DbError> {
        sqlx::query(
            "UPDATE users SET oidc_provider_id = ?, oidc_subject = ?, \
                              oidc_email_verified = ? WHERE id = ?",
        )
        .bind(provider_id)
        .bind(subject)
        .bind(i64::from(email_verified))
        .bind(id)
        .execute(self.db.pool())
        .await?;
        Ok(())
    }

    /// Toggle admin. Used by the admin elevation path when the OIDC
    /// `admin_group` claim flips.
    pub async fn set_admin(&self, id: &str, is_admin: bool) -> Result<(), DbError> {
        sqlx::query("UPDATE users SET is_admin = ? WHERE id = ?")
            .bind(i64::from(is_admin))
            .bind(id)
            .execute(self.db.pool())
            .await?;
        Ok(())
    }

    /// Look up a user by username. Returns `NotFound` if no row.
    pub async fn find_by_username(&self, username: &str) -> Result<User, DbError> {
        let row = sqlx::query(
            "SELECT id, username, password_hash, is_admin, created_at, quota_bytes, \
                    oidc_provider_id, oidc_subject, oidc_email_verified \
             FROM users WHERE username = ?",
        )
        .bind(username)
        .fetch_one(self.db.pool())
        .await
        .map_err(DbError::from_sqlx_no_rows)?;
        row_to_user(&row)
    }

    /// Look up a user by id. Returns `NotFound` if no row.
    pub async fn find_by_id(&self, id: &str) -> Result<User, DbError> {
        let row = sqlx::query(
            "SELECT id, username, password_hash, is_admin, created_at, quota_bytes, \
                    oidc_provider_id, oidc_subject, oidc_email_verified \
             FROM users WHERE id = ?",
        )
        .bind(id)
        .fetch_one(self.db.pool())
        .await
        .map_err(DbError::from_sqlx_no_rows)?;
        row_to_user(&row)
    }

    /// All users in the table, newest first. Backs the Admin → Users
    /// card.
    pub async fn list_all(&self) -> Result<Vec<User>, DbError> {
        let rows = sqlx::query(
            "SELECT id, username, password_hash, is_admin, created_at, quota_bytes, \
                    oidc_provider_id, oidc_subject, oidc_email_verified \
             FROM users ORDER BY created_at DESC",
        )
        .fetch_all(self.db.pool())
        .await?;
        rows.iter().map(row_to_user).collect()
    }

    /// Sum of non-trashed file sizes owned by `user_id`. Drives the
    /// quota check on upload (pipeline §6.4) and the Settings →
    /// Storage card. Returns 0 when the user owns no files.
    pub async fn used_bytes(&self, user_id: &str) -> Result<u64, DbError> {
        let n: Option<i64> = sqlx::query_scalar(
            "SELECT COALESCE(SUM(size), 0) FROM files \
             WHERE owner_id = ? AND trashed_at IS NULL",
        )
        .bind(user_id)
        .fetch_one(self.db.pool())
        .await?;
        Ok(u64::try_from(n.unwrap_or(0)).unwrap_or(0))
    }

    /// Set or clear the per-user storage quota.
    pub async fn set_quota(&self, id: &str, quota_bytes: Option<u64>) -> Result<(), DbError> {
        let n = quota_bytes.and_then(|q| i64::try_from(q).ok());
        sqlx::query("UPDATE users SET quota_bytes = ? WHERE id = ?")
            .bind(n)
            .bind(id)
            .execute(self.db.pool())
            .await?;
        Ok(())
    }

    /// Count rows in `users`. Backs the first-run admin-setup gate —
    /// the wizard runs only when this is zero.
    pub async fn count(&self) -> Result<i64, DbError> {
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
            .fetch_one(self.db.pool())
            .await?;
        Ok(n)
    }

    /// Replace the stored password hash for an existing user. Returns
    /// `NotFound` if the user does not exist.
    pub async fn update_password(&self, id: &str, new_hash: &str) -> Result<(), DbError> {
        let res = sqlx::query("UPDATE users SET password_hash = ? WHERE id = ?")
            .bind(new_hash)
            .bind(id)
            .execute(self.db.pool())
            .await?;
        if res.rows_affected() == 0 {
            return Err(DbError::NotFound);
        }
        Ok(())
    }
}

fn row_to_user(row: &sqlx::any::AnyRow) -> Result<User, DbError> {
    Ok(User {
        id: row.get("id"),
        username: row.get("username"),
        password_hash: row.get("password_hash"),
        is_admin: row.get::<i64, _>("is_admin") != 0,
        created_at: parse_ts(row.get::<String, _>("created_at"))?,
        quota_bytes: row
            .try_get::<Option<i64>, _>("quota_bytes")
            .ok()
            .flatten()
            .and_then(|n| u64::try_from(n).ok()),
        oidc_provider_id: row
            .try_get::<Option<String>, _>("oidc_provider_id")
            .ok()
            .flatten(),
        oidc_subject: row
            .try_get::<Option<String>, _>("oidc_subject")
            .ok()
            .flatten(),
        oidc_email_verified: row
            .try_get::<Option<i64>, _>("oidc_email_verified")
            .ok()
            .flatten()
            .is_some_and(|n| n != 0),
    })
}

fn map_unique_violation(e: sqlx::Error) -> DbError {
    if let sqlx::Error::Database(dbe) = &e {
        if dbe.is_unique_violation() {
            return DbError::UniqueViolation(dbe.message().to_string());
        }
    }
    DbError::Sqlx(e)
}

pub(crate) fn ts(t: time::OffsetDateTime) -> String {
    t.format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

pub(crate) fn parse_ts(s: String) -> Result<time::OffsetDateTime, DbError> {
    time::OffsetDateTime::parse(&s, &time::format_description::well_known::Rfc3339)
        .map_err(|e| DbError::InvalidUrl(format!("bad timestamp {s:?}: {e}")))
}
