//! Files — metadata for uploaded blobs. The bytes live in storage under the
//! key `files/{id}` (per `docs/ARCHITECTURE.md` §"Storage facade").

use serde::{Deserialize, Serialize};
use sqlx::Row;

use crate::{
    users::{parse_ts, ts},
    Db, DbError,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct File {
    pub id: String,
    pub parent_id: Option<String>,
    pub name: String,
    pub size: u64,
    pub content_type: Option<String>,
    pub etag: Option<String>,
    pub version: u32,
    pub owner_id: String,
    /// Workspace this file lives in. Pipeline §8.8. Null for legacy rows
    /// that predate the migration whose owner is also missing a Personal
    /// workspace; otherwise always set.
    pub workspace_id: Option<String>,
    /// Bring-your-own storage pointer (pipeline §8.9). NULL = server
    /// default; otherwise → `workspace_storage.id`. Permanent on the row
    /// so flipping a workspace's storage later doesn't orphan files.
    pub storage_id: Option<String>,
    pub trashed_at: Option<time::OffsetDateTime>,
    pub original_parent_id: Option<String>,
    pub created_at: time::OffsetDateTime,
    pub modified_at: time::OffsetDateTime,
    /// Client-generated preview, stored as a data URI. None for non-image
    /// uploads or files predating the v0.1 migration.
    pub thumbnail: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NewFile {
    pub id: String,
    pub parent_id: Option<String>,
    pub name: String,
    pub size: u64,
    pub content_type: Option<String>,
    pub etag: Option<String>,
    pub owner_id: String,
    pub workspace_id: String,
    /// Optional pointer to a `workspace_storage` row. None = server default.
    pub storage_id: Option<String>,
    pub thumbnail: Option<String>,
}

#[derive(Debug, Clone)]
pub struct FileRepo<'a> {
    db: &'a Db,
}

impl<'a> FileRepo<'a> {
    #[must_use]
    pub fn new(db: &'a Db) -> Self {
        Self { db }
    }

    /// Insert a file row. Caller picks the id (so the storage key can be
    /// computed before bytes land — see `docs/ARCHITECTURE.md` §"Storage facade").
    pub async fn insert(&self, new: &NewFile) -> Result<File, DbError> {
        let now = time::OffsetDateTime::now_utc();
        let now_s = ts(now);
        let size_i = i64::try_from(new.size).unwrap_or(i64::MAX);
        sqlx::query(
            "INSERT INTO files (id, parent_id, name, size, content_type, etag, owner_id, created_at, modified_at, thumbnail, workspace_id, storage_id) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&new.id)
        .bind(&new.parent_id)
        .bind(&new.name)
        .bind(size_i)
        .bind(&new.content_type)
        .bind(&new.etag)
        .bind(&new.owner_id)
        .bind(&now_s)
        .bind(&now_s)
        .bind(&new.thumbnail)
        .bind(&new.workspace_id)
        .bind(&new.storage_id)
        .execute(self.db.pool())
        .await?;
        Ok(File {
            id: new.id.clone(),
            parent_id: new.parent_id.clone(),
            name: new.name.clone(),
            size: new.size,
            content_type: new.content_type.clone(),
            etag: new.etag.clone(),
            version: 1,
            owner_id: new.owner_id.clone(),
            workspace_id: Some(new.workspace_id.clone()),
            storage_id: new.storage_id.clone(),
            trashed_at: None,
            original_parent_id: None,
            created_at: now,
            modified_at: now,
            thumbnail: new.thumbnail.clone(),
        })
    }

    pub async fn find_by_id(&self, id: &str) -> Result<File, DbError> {
        let row = sqlx::query(
            "SELECT id, parent_id, name, size, content_type, etag, version, owner_id, \
                    workspace_id, storage_id, trashed_at, original_parent_id, created_at, modified_at, thumbnail \
             FROM files WHERE id = ?",
        )
        .bind(id)
        .fetch_one(self.db.pool())
        .await
        .map_err(DbError::from_sqlx_no_rows)?;
        row_to_file(&row)
    }

    pub async fn list_children(
        &self,
        parent_id: Option<&str>,
        owner_id: &str,
    ) -> Result<Vec<File>, DbError> {
        let rows = match parent_id {
            Some(pid) => sqlx::query(
                "SELECT id, parent_id, name, size, content_type, etag, version, owner_id, \
                        workspace_id, storage_id, trashed_at, original_parent_id, created_at, modified_at, thumbnail \
                 FROM files \
                 WHERE parent_id = ? AND owner_id = ? AND trashed_at IS NULL \
                 ORDER BY name ASC",
            )
            .bind(pid)
            .bind(owner_id),
            None => sqlx::query(
                "SELECT id, parent_id, name, size, content_type, etag, version, owner_id, \
                        workspace_id, storage_id, trashed_at, original_parent_id, created_at, modified_at, thumbnail \
                 FROM files \
                 WHERE parent_id IS NULL AND owner_id = ? AND trashed_at IS NULL \
                 ORDER BY name ASC",
            )
            .bind(owner_id),
        }
        .fetch_all(self.db.pool())
        .await?;
        rows.iter().map(row_to_file).collect()
    }

    /// Same as `list_children`, but scoped to a specific workspace
    /// instead of owner. Backs the workspace-aware file browser
    /// (pipeline §8.8). Trashed rows excluded.
    pub async fn list_children_in_workspace(
        &self,
        parent_id: Option<&str>,
        workspace_id: &str,
    ) -> Result<Vec<File>, DbError> {
        let rows = match parent_id {
            Some(pid) => sqlx::query(
                "SELECT id, parent_id, name, size, content_type, etag, version, owner_id, \
                        workspace_id, storage_id, trashed_at, original_parent_id, created_at, modified_at, thumbnail \
                 FROM files \
                 WHERE parent_id = ? AND workspace_id = ? AND trashed_at IS NULL \
                 ORDER BY name ASC",
            )
            .bind(pid)
            .bind(workspace_id),
            None => sqlx::query(
                "SELECT id, parent_id, name, size, content_type, etag, version, owner_id, \
                        workspace_id, storage_id, trashed_at, original_parent_id, created_at, modified_at, thumbnail \
                 FROM files \
                 WHERE parent_id IS NULL AND workspace_id = ? AND trashed_at IS NULL \
                 ORDER BY name ASC",
            )
            .bind(workspace_id),
        }
        .fetch_all(self.db.pool())
        .await?;
        rows.iter().map(row_to_file).collect()
    }

    /// Sum of file sizes in one workspace. Phase 2 quota source.
    pub async fn workspace_used_bytes(&self, workspace_id: &str) -> Result<u64, DbError> {
        let n: Option<i64> = sqlx::query_scalar(
            "SELECT COALESCE(SUM(size), 0) FROM files \
             WHERE workspace_id = ? AND trashed_at IS NULL",
        )
        .bind(workspace_id)
        .fetch_one(self.db.pool())
        .await?;
        Ok(u64::try_from(n.unwrap_or(0)).unwrap_or(0))
    }

    /// Case-insensitive substring search by display `name`. Scoped to a
    /// workspace, excludes trashed files. Returns up to `limit` rows,
    /// name-sorted. Spec: docs/ux/12-search-surface.md.
    pub async fn search(
        &self,
        workspace_id: &str,
        query: &str,
        limit: i64,
    ) -> Result<Vec<File>, DbError> {
        let pattern = format!("%{}%", query.to_lowercase());
        let rows = sqlx::query(
            "SELECT id, parent_id, name, size, content_type, etag, version, owner_id, \
                    workspace_id, storage_id, trashed_at, original_parent_id, created_at, modified_at, thumbnail \
             FROM files \
             WHERE workspace_id = ? AND trashed_at IS NULL AND LOWER(name) LIKE ? \
             ORDER BY name ASC LIMIT ?",
        )
        .bind(workspace_id)
        .bind(pattern)
        .bind(limit.clamp(1, 200))
        .fetch_all(self.db.pool())
        .await?;
        rows.iter().map(row_to_file).collect()
    }

    pub async fn rename(&self, id: &str, new_name: &str) -> Result<(), DbError> {
        let now_s = ts(time::OffsetDateTime::now_utc());
        sqlx::query("UPDATE files SET name = ?, modified_at = ? WHERE id = ?")
            .bind(new_name)
            .bind(&now_s)
            .bind(id)
            .execute(self.db.pool())
            .await?;
        Ok(())
    }

    pub async fn move_to(&self, id: &str, new_parent_id: Option<&str>) -> Result<(), DbError> {
        let now_s = ts(time::OffsetDateTime::now_utc());
        sqlx::query("UPDATE files SET parent_id = ?, modified_at = ? WHERE id = ?")
            .bind(new_parent_id)
            .bind(&now_s)
            .bind(id)
            .execute(self.db.pool())
            .await?;
        Ok(())
    }

    pub async fn trash(&self, id: &str) -> Result<(), DbError> {
        let now = time::OffsetDateTime::now_utc();
        let now_s = ts(now);
        sqlx::query(
            "UPDATE files \
             SET trashed_at = ?, original_parent_id = parent_id, parent_id = NULL, modified_at = ? \
             WHERE id = ?",
        )
        .bind(&now_s)
        .bind(&now_s)
        .bind(id)
        .execute(self.db.pool())
        .await?;
        Ok(())
    }

    pub async fn restore(&self, id: &str) -> Result<(), DbError> {
        let now_s = ts(time::OffsetDateTime::now_utc());
        sqlx::query(
            "UPDATE files \
             SET parent_id = original_parent_id, trashed_at = NULL, original_parent_id = NULL, modified_at = ? \
             WHERE id = ?",
        )
        .bind(&now_s)
        .bind(id)
        .execute(self.db.pool())
        .await?;
        Ok(())
    }

    /// Bump version + update size/etag after a successful PutFile or upload.
    pub async fn record_overwrite(
        &self,
        id: &str,
        size: u64,
        etag: Option<&str>,
    ) -> Result<(), DbError> {
        let now_s = ts(time::OffsetDateTime::now_utc());
        let size_i = i64::try_from(size).unwrap_or(i64::MAX);
        sqlx::query(
            "UPDATE files SET size = ?, etag = ?, version = version + 1, modified_at = ? \
             WHERE id = ?",
        )
        .bind(size_i)
        .bind(etag)
        .bind(&now_s)
        .bind(id)
        .execute(self.db.pool())
        .await?;
        Ok(())
    }
}

fn row_to_file(row: &sqlx::any::AnyRow) -> Result<File, DbError> {
    Ok(File {
        id: row.get("id"),
        parent_id: row.get("parent_id"),
        name: row.get("name"),
        size: u64::try_from(row.get::<i64, _>("size")).unwrap_or(0),
        content_type: row.get("content_type"),
        etag: row.get("etag"),
        version: u32::try_from(row.get::<i64, _>("version")).unwrap_or(1),
        owner_id: row.get("owner_id"),
        workspace_id: row
            .try_get::<Option<String>, _>("workspace_id")
            .ok()
            .flatten(),
        storage_id: row
            .try_get::<Option<String>, _>("storage_id")
            .ok()
            .flatten(),
        trashed_at: row
            .try_get::<Option<String>, _>("trashed_at")?
            .map(parse_ts)
            .transpose()?,
        original_parent_id: row.get("original_parent_id"),
        created_at: parse_ts(row.get::<String, _>("created_at"))?,
        modified_at: parse_ts(row.get::<String, _>("modified_at"))?,
        thumbnail: row.try_get::<Option<String>, _>("thumbnail").ok().flatten(),
    })
}
