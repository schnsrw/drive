//! Files — metadata for uploaded blobs. The bytes live in storage under the
//! key `files/{id}` (per `docs/ARCHITECTURE.md` §"Storage facade").

use serde::{Deserialize, Serialize};
use sqlx::Row;

use crate::{
    search::{placeholders, BindValue, SearchFilters, SearchPaging, TypeBucket},
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
    /// Lifecycle (pipeline §13.6). `ready` is the default for the proxy
    /// multipart path + every pre-direct-upload row.
    pub status: FileStatus,
    /// Bytes the SPA promised at presign time. Counted against the
    /// workspace quota until status flips to `ready`. NULL when irrelevant.
    pub expected_size: Option<u64>,
    /// Server-side multi-size thumbnail generation state (pipeline §5.4).
    pub thumbs_state: ThumbsState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum FileStatus {
    Uploading,
    #[default]
    Ready,
    Failed,
}

impl FileStatus {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Uploading => "uploading",
            Self::Ready => "ready",
            Self::Failed => "failed",
        }
    }
    fn parse(s: &str) -> Self {
        match s {
            "uploading" => Self::Uploading,
            "failed" => Self::Failed,
            _ => Self::Ready,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ThumbsState {
    #[default]
    Pending,
    Ready,
    Unsupported,
    Failed,
}

impl ThumbsState {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Ready => "ready",
            Self::Unsupported => "unsupported",
            Self::Failed => "failed",
        }
    }
    fn parse(s: &str) -> Self {
        match s {
            "ready" => Self::Ready,
            "unsupported" => Self::Unsupported,
            "failed" => Self::Failed,
            _ => Self::Pending,
        }
    }
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
    /// Lifecycle (pipeline §13.6). Defaults to `Ready` for proxy uploads.
    /// Direct-upload presigners pass `Uploading` + an `expected_size`.
    pub status: FileStatus,
    pub expected_size: Option<u64>,
}

impl Default for NewFile {
    fn default() -> Self {
        Self {
            id: String::new(),
            parent_id: None,
            name: String::new(),
            size: 0,
            content_type: None,
            etag: None,
            owner_id: String::new(),
            workspace_id: String::new(),
            storage_id: None,
            thumbnail: None,
            status: FileStatus::Ready,
            expected_size: None,
        }
    }
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
        let expected_i = new
            .expected_size
            .map(|e| i64::try_from(e).unwrap_or(i64::MAX));
        sqlx::query(
            "INSERT INTO files (id, parent_id, name, size, content_type, etag, owner_id, created_at, modified_at, thumbnail, workspace_id, storage_id, status, expected_size) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
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
        .bind(new.status.as_str())
        .bind(expected_i)
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
            status: new.status,
            expected_size: new.expected_size,
            thumbs_state: ThumbsState::Pending,
        })
    }

    pub async fn find_by_id(&self, id: &str) -> Result<File, DbError> {
        let row = sqlx::query(
            "SELECT id, parent_id, name, size, content_type, etag, version, owner_id, \
                    workspace_id, storage_id, trashed_at, original_parent_id, created_at, modified_at, thumbnail, status, expected_size, thumbs_state, thumbs_generated_at \
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
                        workspace_id, storage_id, trashed_at, original_parent_id, created_at, modified_at, thumbnail, status, expected_size, thumbs_state, thumbs_generated_at \
                 FROM files \
                 WHERE parent_id = ? AND owner_id = ? AND trashed_at IS NULL \
                 ORDER BY name ASC",
            )
            .bind(pid)
            .bind(owner_id),
            None => sqlx::query(
                "SELECT id, parent_id, name, size, content_type, etag, version, owner_id, \
                        workspace_id, storage_id, trashed_at, original_parent_id, created_at, modified_at, thumbnail, status, expected_size, thumbs_state, thumbs_generated_at \
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
                        workspace_id, storage_id, trashed_at, original_parent_id, created_at, modified_at, thumbnail, status, expected_size, thumbs_state, thumbs_generated_at \
                 FROM files \
                 WHERE parent_id = ? AND workspace_id = ? AND trashed_at IS NULL \
                 ORDER BY name ASC",
            )
            .bind(pid)
            .bind(workspace_id),
            None => sqlx::query(
                "SELECT id, parent_id, name, size, content_type, etag, version, owner_id, \
                        workspace_id, storage_id, trashed_at, original_parent_id, created_at, modified_at, thumbnail, status, expected_size, thumbs_state, thumbs_generated_at \
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

    /// Sum of file sizes in one workspace, counting in-flight presigns
    /// against the quota too. Phase 2 quota source.
    ///
    /// `ready` rows use their committed `size`; `uploading` rows use
    /// their `expected_size` (capped at i64::MAX). Without this clamp a
    /// pair of parallel presigns could both squeeze under the cap and
    /// then together exceed it — the spec calls this out explicitly.
    pub async fn workspace_used_bytes(&self, workspace_id: &str) -> Result<u64, DbError> {
        let n: Option<i64> = sqlx::query_scalar(
            "SELECT COALESCE(SUM( \
                  CASE WHEN status = 'uploading' \
                       THEN COALESCE(expected_size, 0) \
                       ELSE size END \
                ), 0) \
             FROM files \
             WHERE workspace_id = ? AND trashed_at IS NULL",
        )
        .bind(workspace_id)
        .fetch_one(self.db.pool())
        .await?;
        Ok(u64::try_from(n.unwrap_or(0)).unwrap_or(0))
    }

    /// Direct upload finalize. Stamps the real size + etag (the values
    /// the bucket returned via stat) onto an `uploading` row and flips
    /// it to `ready`. Returns the updated row.
    ///
    /// Pipeline §13.6 spec: clears `expected_size` (NULL) once we have
    /// the real `size` so the column doesn't carry stale numbers around.
    pub async fn mark_uploaded(
        &self,
        id: &str,
        size: u64,
        etag: Option<&str>,
        content_type: Option<&str>,
    ) -> Result<File, DbError> {
        let now_s = ts(time::OffsetDateTime::now_utc());
        let size_i = i64::try_from(size).unwrap_or(i64::MAX);
        sqlx::query(
            "UPDATE files \
             SET size = ?, etag = ?, \
                 content_type = COALESCE(?, content_type), \
                 status = 'ready', expected_size = NULL, \
                 modified_at = ?, thumbs_state = 'pending' \
             WHERE id = ?",
        )
        .bind(size_i)
        .bind(etag)
        .bind(content_type)
        .bind(&now_s)
        .bind(id)
        .execute(self.db.pool())
        .await?;
        self.find_by_id(id).await
    }

    /// Direct upload abort / janitor sweep. Hard-deletes the row.
    /// Caller is expected to best-effort delete the storage object too.
    pub async fn delete_by_id(&self, id: &str) -> Result<(), DbError> {
        sqlx::query("DELETE FROM files WHERE id = ?")
            .bind(id)
            .execute(self.db.pool())
            .await?;
        Ok(())
    }

    /// Bulk-update the thumbnail generation state. Pipeline §5.4.
    pub async fn set_thumbs_state(
        &self,
        id: &str,
        state: ThumbsState,
        mark_generated: bool,
    ) -> Result<(), DbError> {
        let now_s = ts(time::OffsetDateTime::now_utc());
        if mark_generated {
            sqlx::query("UPDATE files SET thumbs_state = ?, thumbs_generated_at = ? WHERE id = ?")
                .bind(state.as_str())
                .bind(&now_s)
                .bind(id)
                .execute(self.db.pool())
                .await?;
        } else {
            sqlx::query("UPDATE files SET thumbs_state = ? WHERE id = ?")
                .bind(state.as_str())
                .bind(id)
                .execute(self.db.pool())
                .await?;
        }
        Ok(())
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
                    workspace_id, storage_id, trashed_at, original_parent_id, created_at, modified_at, thumbnail, status, expected_size, thumbs_state, thumbs_generated_at \
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

    /// Phase 3 search: filters + sort + cursor pagination.
    /// Spec: `docs/ux/12-search-surface.md` + `docs/research/16-scale-infra.md`.
    ///
    /// Returns up to `paging.limit + 1` rows — the caller uses the +1 to
    /// detect "is there another page" and trims it off before responding.
    pub async fn search_with(
        &self,
        filters: &SearchFilters,
        paging: &SearchPaging,
    ) -> Result<Vec<File>, DbError> {
        // Files participate only when the type filter is empty OR
        // includes at least one bucket other than Folder/Note.
        let file_buckets: Vec<TypeBucket> = filters
            .types
            .iter()
            .copied()
            .filter(|t| !matches!(t, TypeBucket::Folder | TypeBucket::Note))
            .collect();
        if !filters.types.is_empty() && file_buckets.is_empty() {
            return Ok(vec![]);
        }

        // SQL builder. We construct the WHERE / ORDER BY / LIMIT clause
        // dynamically because the filter set is dynamic; bind values are
        // collected in parallel so the order matches the `?` placeholders.
        let mut sql = String::from(
            "SELECT id, parent_id, name, size, content_type, etag, version, owner_id, \
                    workspace_id, storage_id, trashed_at, original_parent_id, created_at, \
                    modified_at, thumbnail, status, expected_size, thumbs_state, \
                    thumbs_generated_at \
             FROM files WHERE ",
        );
        let mut binds: Vec<BindValue> = Vec::new();
        let mut first = true;
        let mut and = |sql: &mut String, frag: &str| {
            if first {
                first = false;
            } else {
                sql.push_str(" AND ");
            }
            sql.push_str(frag);
        };

        // Workspaces (required, always non-empty)
        and(
            &mut sql,
            &format!(
                "workspace_id IN ({})",
                placeholders(filters.workspace_ids.len())
            ),
        );
        for w in &filters.workspace_ids {
            binds.push(BindValue::Str(w.clone()));
        }

        // Optional folder scope
        if let Some(folder) = &filters.folder_id {
            and(&mut sql, "parent_id = ?");
            binds.push(BindValue::Str(folder.clone()));
        }

        // Trash semantics
        match filters.in_trash {
            None | Some(false) => and(&mut sql, "trashed_at IS NULL"),
            Some(true) => and(&mut sql, "trashed_at IS NOT NULL"),
        }

        // Query text (case-insensitive LIKE)
        if !filters.q.is_empty() {
            and(&mut sql, "LOWER(name) LIKE ?");
            binds.push(BindValue::Str(format!("%{}%", filters.q.to_lowercase())));
        }

        // Type buckets
        if !file_buckets.is_empty() {
            let mut type_parts: Vec<String> = Vec::new();
            for bucket in &file_buckets {
                for pred in bucket.content_type_predicates() {
                    if pred.ends_with('/') {
                        // prefix match
                        type_parts.push("content_type LIKE ?".into());
                        binds.push(BindValue::Str(format!("{pred}%")));
                    } else {
                        type_parts.push("content_type = ?".into());
                        binds.push(BindValue::Str((*pred).to_string()));
                    }
                }
            }
            if !type_parts.is_empty() {
                and(&mut sql, &format!("({})", type_parts.join(" OR ")));
            }
        }

        // Owner
        if !filters.owner_ids.is_empty() {
            and(
                &mut sql,
                &format!("owner_id IN ({})", placeholders(filters.owner_ids.len())),
            );
            for o in &filters.owner_ids {
                binds.push(BindValue::Str(o.clone()));
            }
        }

        // Date ranges
        if let Some(t) = filters.modified_after {
            and(&mut sql, "modified_at >= ?");
            binds.push(BindValue::Str(ts(t)));
        }
        if let Some(t) = filters.modified_before {
            and(&mut sql, "modified_at <= ?");
            binds.push(BindValue::Str(ts(t)));
        }
        if let Some(t) = filters.created_after {
            and(&mut sql, "created_at >= ?");
            binds.push(BindValue::Str(ts(t)));
        }
        if let Some(t) = filters.created_before {
            and(&mut sql, "created_at <= ?");
            binds.push(BindValue::Str(ts(t)));
        }

        // Size
        if let Some(min) = filters.size_min {
            and(&mut sql, "size >= ?");
            binds.push(BindValue::I64(min as i64));
        }
        if let Some(max) = filters.size_max {
            and(&mut sql, "size <= ?");
            binds.push(BindValue::I64(max as i64));
        }

        // Share-link presence (subquery on share_links table)
        if let Some(want) = filters.has_share_link {
            let frag = if want {
                "EXISTS (SELECT 1 FROM share_links WHERE share_links.file_id = files.id)"
            } else {
                "NOT EXISTS (SELECT 1 FROM share_links WHERE share_links.file_id = files.id)"
            };
            and(&mut sql, frag);
        }

        // Cursor: skip past the previously-seen page.
        // Predicate is `(sort_col, id) <after>` where the comparator is
        // > or < depending on sort direction. Tie-broken by id ASC so
        // pagination is deterministic.
        if let Some((last_value, last_id)) = &paging.after {
            let cmp = match paging.sort_dir {
                crate::search::SortDir::Asc => ">",
                crate::search::SortDir::Desc => "<",
            };
            and(
                &mut sql,
                &format!(
                    "({col} {cmp} ? OR ({col} = ? AND id > ?))",
                    col = paging.order_column(),
                    cmp = cmp
                ),
            );
            binds.push(BindValue::Str(last_value.clone()));
            binds.push(BindValue::Str(last_value.clone()));
            binds.push(BindValue::Str(last_id.clone()));
        }

        // Order + limit. +1 to detect has-more.
        use std::fmt::Write;
        let _ = write!(
            sql,
            " ORDER BY {col} {dir}, id ASC LIMIT ?",
            col = paging.order_column(),
            dir = paging.order_sql(),
        );
        let fetch_limit = paging.limit.clamp(1, 200) + 1;
        binds.push(BindValue::I64(fetch_limit));

        let mut q = sqlx::query(&sql);
        for b in &binds {
            q = match b {
                BindValue::Str(s) => q.bind(s.as_str()),
                BindValue::I64(n) => q.bind(*n),
            };
        }
        let rows = q.fetch_all(self.db.pool()).await?;
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
        status: row
            .try_get::<Option<String>, _>("status")
            .ok()
            .flatten()
            .as_deref()
            .map_or(FileStatus::Ready, FileStatus::parse),
        expected_size: row
            .try_get::<Option<i64>, _>("expected_size")
            .ok()
            .flatten()
            .and_then(|v| u64::try_from(v).ok()),
        thumbs_state: row
            .try_get::<Option<String>, _>("thumbs_state")
            .ok()
            .flatten()
            .as_deref()
            .map_or(ThumbsState::Pending, ThumbsState::parse),
    })
}
