//! Folders — hierarchical tree. `parent_id = NULL` ≡ root.

use serde::{Deserialize, Serialize};
use sqlx::Row;

use crate::{
    search::{placeholders, BindValue, SearchFilters, SearchPaging, TypeBucket},
    users::{parse_ts, ts},
    Db, DbError,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Folder {
    pub id: String,
    pub parent_id: Option<String>,
    pub name: String,
    pub owner_id: String,
    /// Workspace this folder lives in. Nullable on disk only because the
    /// 0006 ALTER TABLE migration backfills async; new rows always have one.
    pub workspace_id: Option<String>,
    pub trashed_at: Option<time::OffsetDateTime>,
    pub original_parent_id: Option<String>,
    pub created_at: time::OffsetDateTime,
    pub modified_at: time::OffsetDateTime,
}

#[derive(Debug, Clone)]
pub struct NewFolder {
    pub parent_id: Option<String>,
    pub name: String,
    pub owner_id: String,
    pub workspace_id: String,
}

#[derive(Debug, Clone)]
pub struct FolderRepo<'a> {
    db: &'a Db,
}

impl<'a> FolderRepo<'a> {
    #[must_use]
    pub fn new(db: &'a Db) -> Self {
        Self { db }
    }

    /// Create a new folder under `parent_id` (None = root).
    pub async fn insert(&self, new: &NewFolder) -> Result<Folder, DbError> {
        let id = ulid::Ulid::new().to_string();
        let now = time::OffsetDateTime::now_utc();
        let now_s = ts(now);
        sqlx::query(
            "INSERT INTO folders (id, parent_id, name, owner_id, created_at, modified_at, workspace_id) \
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(&new.parent_id)
        .bind(&new.name)
        .bind(&new.owner_id)
        .bind(&now_s)
        .bind(&now_s)
        .bind(&new.workspace_id)
        .execute(self.db.pool())
        .await?;
        Ok(Folder {
            id,
            parent_id: new.parent_id.clone(),
            name: new.name.clone(),
            owner_id: new.owner_id.clone(),
            workspace_id: Some(new.workspace_id.clone()),
            trashed_at: None,
            original_parent_id: None,
            created_at: now,
            modified_at: now,
        })
    }

    pub async fn find_by_id(&self, id: &str) -> Result<Folder, DbError> {
        let row = sqlx::query(
            "SELECT id, parent_id, name, owner_id, workspace_id, trashed_at, original_parent_id, \
                    created_at, modified_at \
             FROM folders WHERE id = ?",
        )
        .bind(id)
        .fetch_one(self.db.pool())
        .await
        .map_err(DbError::from_sqlx_no_rows)?;
        row_to_folder(&row)
    }

    /// List non-trashed folders directly under `parent_id` (None = root) for
    /// an owner. Sorted by name ascending.
    pub async fn list_children(
        &self,
        parent_id: Option<&str>,
        owner_id: &str,
    ) -> Result<Vec<Folder>, DbError> {
        let rows = match parent_id {
            Some(pid) => sqlx::query(
                "SELECT id, parent_id, name, owner_id, workspace_id, trashed_at, original_parent_id, \
                            created_at, modified_at \
                     FROM folders \
                     WHERE parent_id = ? AND owner_id = ? AND trashed_at IS NULL \
                     ORDER BY name ASC",
            )
            .bind(pid)
            .bind(owner_id),
            None => sqlx::query(
                "SELECT id, parent_id, name, owner_id, workspace_id, trashed_at, original_parent_id, \
                        created_at, modified_at \
                 FROM folders \
                 WHERE parent_id IS NULL AND owner_id = ? AND trashed_at IS NULL \
                 ORDER BY name ASC",
            )
            .bind(owner_id),
        }
        .fetch_all(self.db.pool())
        .await?;
        rows.iter().map(row_to_folder).collect()
    }

    /// Same as `list_children`, but scoped to a workspace instead of
    /// owner. Phase 2 path.
    pub async fn list_children_in_workspace(
        &self,
        parent_id: Option<&str>,
        workspace_id: &str,
    ) -> Result<Vec<Folder>, DbError> {
        let rows = match parent_id {
            Some(pid) => sqlx::query(
                "SELECT id, parent_id, name, owner_id, workspace_id, trashed_at, original_parent_id, \
                        created_at, modified_at \
                 FROM folders \
                 WHERE parent_id = ? AND workspace_id = ? AND trashed_at IS NULL \
                 ORDER BY name ASC",
            )
            .bind(pid)
            .bind(workspace_id),
            None => sqlx::query(
                "SELECT id, parent_id, name, owner_id, workspace_id, trashed_at, original_parent_id, \
                        created_at, modified_at \
                 FROM folders \
                 WHERE parent_id IS NULL AND workspace_id = ? AND trashed_at IS NULL \
                 ORDER BY name ASC",
            )
            .bind(workspace_id),
        }
        .fetch_all(self.db.pool())
        .await?;
        rows.iter().map(row_to_folder).collect()
    }

    /// Case-insensitive substring search by display `name`. Workspace-scoped,
    /// excludes trashed folders. Returns up to `limit` rows, name-sorted.
    /// Spec: docs/ux/12-search-surface.md.
    pub async fn search(
        &self,
        workspace_id: &str,
        query: &str,
        limit: i64,
    ) -> Result<Vec<Folder>, DbError> {
        let pattern = format!("%{}%", query.to_lowercase());
        let rows = sqlx::query(
            "SELECT id, parent_id, name, owner_id, workspace_id, trashed_at, original_parent_id, \
                    created_at, modified_at \
             FROM folders \
             WHERE workspace_id = ? AND trashed_at IS NULL AND LOWER(name) LIKE ? \
             ORDER BY name ASC LIMIT ?",
        )
        .bind(workspace_id)
        .bind(pattern)
        .bind(limit.clamp(1, 200))
        .fetch_all(self.db.pool())
        .await?;
        rows.iter().map(row_to_folder).collect()
    }

    /// Phase 3 search. Folders participate only when the type filter is
    /// empty OR explicitly includes the Folder bucket. Folder-specific
    /// filters (size, content_type, share-link) are no-ops here.
    pub async fn search_with(
        &self,
        filters: &SearchFilters,
        paging: &SearchPaging,
    ) -> Result<Vec<Folder>, DbError> {
        // Type-filter gate
        if !filters.types.is_empty() && !filters.types.contains(&TypeBucket::Folder) {
            return Ok(vec![]);
        }
        // Size + has_share_link don't apply to folders — if those were
        // the only meaningful filters, the result is empty by design.
        // (We still honour date / owner / workspace / q / trash.)
        if filters.has_share_link.is_some() {
            // Folders don't have share links in v0; presence-required → empty.
            if filters.has_share_link == Some(true) {
                return Ok(vec![]);
            }
            // Presence-absent → no extra constraint.
        }

        let mut sql = String::from(
            "SELECT id, parent_id, name, owner_id, workspace_id, trashed_at, \
                    original_parent_id, created_at, modified_at \
             FROM folders WHERE ",
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

        if let Some(folder) = &filters.folder_id {
            and(&mut sql, "parent_id = ?");
            binds.push(BindValue::Str(folder.clone()));
        }

        match filters.in_trash {
            None | Some(false) => and(&mut sql, "trashed_at IS NULL"),
            Some(true) => and(&mut sql, "trashed_at IS NOT NULL"),
        }

        if !filters.q.is_empty() {
            and(&mut sql, "LOWER(name) LIKE ?");
            binds.push(BindValue::Str(format!("%{}%", filters.q.to_lowercase())));
        }

        if !filters.owner_ids.is_empty() {
            and(
                &mut sql,
                &format!("owner_id IN ({})", placeholders(filters.owner_ids.len())),
            );
            for o in &filters.owner_ids {
                binds.push(BindValue::Str(o.clone()));
            }
        }

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

        if let Some((last_value, last_id)) = &paging.after {
            let cmp = match paging.sort_dir {
                crate::search::SortDir::Asc => ">",
                crate::search::SortDir::Desc => "<",
            };
            // Folders don't have a `size` column; if sort_by is Size we
            // fall back to modified for the column name.
            let col = if paging.order_column() == "size" {
                "modified_at"
            } else {
                paging.order_column()
            };
            and(
                &mut sql,
                &format!("({col} {cmp} ? OR ({col} = ? AND id > ?))"),
            );
            binds.push(BindValue::Str(last_value.clone()));
            binds.push(BindValue::Str(last_value.clone()));
            binds.push(BindValue::Str(last_id.clone()));
        }

        let col = if paging.order_column() == "size" {
            "modified_at"
        } else {
            paging.order_column()
        };
        use std::fmt::Write;
        let _ = write!(
            sql,
            " ORDER BY {col} {dir}, id ASC LIMIT ?",
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
        rows.iter().map(row_to_folder).collect()
    }

    pub async fn rename(&self, id: &str, new_name: &str) -> Result<(), DbError> {
        let now_s = ts(time::OffsetDateTime::now_utc());
        sqlx::query("UPDATE folders SET name = ?, modified_at = ? WHERE id = ?")
            .bind(new_name)
            .bind(&now_s)
            .bind(id)
            .execute(self.db.pool())
            .await?;
        Ok(())
    }

    pub async fn move_to(&self, id: &str, new_parent_id: Option<&str>) -> Result<(), DbError> {
        let now_s = ts(time::OffsetDateTime::now_utc());
        sqlx::query("UPDATE folders SET parent_id = ?, modified_at = ? WHERE id = ?")
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
            "UPDATE folders \
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
            "UPDATE folders \
             SET parent_id = original_parent_id, trashed_at = NULL, original_parent_id = NULL, modified_at = ? \
             WHERE id = ?",
        )
        .bind(&now_s)
        .bind(id)
        .execute(self.db.pool())
        .await?;
        Ok(())
    }
}

fn row_to_folder(row: &sqlx::any::AnyRow) -> Result<Folder, DbError> {
    Ok(Folder {
        id: row.get("id"),
        parent_id: row.get("parent_id"),
        name: row.get("name"),
        owner_id: row.get("owner_id"),
        workspace_id: row
            .try_get::<Option<String>, _>("workspace_id")
            .ok()
            .flatten(),
        trashed_at: row
            .try_get::<Option<String>, _>("trashed_at")?
            .map(parse_ts)
            .transpose()?,
        original_parent_id: row.get("original_parent_id"),
        created_at: parse_ts(row.get::<String, _>("created_at"))?,
        modified_at: parse_ts(row.get::<String, _>("modified_at"))?,
    })
}
