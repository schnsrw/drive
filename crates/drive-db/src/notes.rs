//! Notes / Wiki repos. Pipeline §8.11.
//! Spec: docs/research/09-notes-wiki.md.
//!
//! Two repos in one module:
//!   - [`NotesRepo`]  — the page tree + body.
//!   - [`NoteLinksRepo`] — the backlinks index (re-derived from body on save).
//!
//! Workspace gating is the caller's job; this layer trusts the workspace id
//! its handler passed in.

use serde::{Deserialize, Serialize};
use sqlx::Row;

use crate::{
    search::{placeholders, BindValue, SearchFilters, SearchPaging, TypeBucket},
    users::{parse_ts, ts},
    Db, DbError,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Note {
    pub id: String,
    pub workspace_id: String,
    pub parent_id: Option<String>,
    pub title: String,
    pub body: String,
    pub owner_id: String,
    pub order_key: String,
    pub trashed_at: Option<time::OffsetDateTime>,
    pub created_at: time::OffsetDateTime,
    pub modified_at: time::OffsetDateTime,
}

/// Slim shape for tree responses — no body, no timestamps. Keeps
/// `GET /api/notes/tree` cheap for workspaces with hundreds of notes.
#[derive(Debug, Clone, Serialize)]
pub struct NoteNode {
    pub id: String,
    pub parent_id: Option<String>,
    pub title: String,
    pub order_key: String,
}

#[derive(Debug, Clone)]
pub struct NewNote {
    pub workspace_id: String,
    pub parent_id: Option<String>,
    pub title: String,
    pub owner_id: String,
    pub order_key: String,
}

#[derive(Debug, Clone)]
pub struct NotesRepo<'a> {
    db: &'a Db,
}

impl<'a> NotesRepo<'a> {
    #[must_use]
    pub fn new(db: &'a Db) -> Self {
        Self { db }
    }

    pub async fn insert(&self, new: &NewNote) -> Result<Note, DbError> {
        let id = ulid::Ulid::new().to_string();
        let now = time::OffsetDateTime::now_utc();
        let now_s = ts(now);
        sqlx::query(
            "INSERT INTO notes \
             (id, workspace_id, parent_id, title, body, owner_id, order_key, \
              trashed_at, created_at, modified_at) \
             VALUES (?, ?, ?, ?, '', ?, ?, NULL, ?, ?)",
        )
        .bind(&id)
        .bind(&new.workspace_id)
        .bind(&new.parent_id)
        .bind(&new.title)
        .bind(&new.owner_id)
        .bind(&new.order_key)
        .bind(&now_s)
        .bind(&now_s)
        .execute(self.db.pool())
        .await?;
        Ok(Note {
            id,
            workspace_id: new.workspace_id.clone(),
            parent_id: new.parent_id.clone(),
            title: new.title.clone(),
            body: String::new(),
            owner_id: new.owner_id.clone(),
            order_key: new.order_key.clone(),
            trashed_at: None,
            created_at: now,
            modified_at: now,
        })
    }

    pub async fn find_by_id(&self, id: &str) -> Result<Note, DbError> {
        let row = sqlx::query(
            "SELECT id, workspace_id, parent_id, title, body, owner_id, \
                    order_key, trashed_at, created_at, modified_at \
             FROM notes WHERE id = ?",
        )
        .bind(id)
        .fetch_one(self.db.pool())
        .await
        .map_err(DbError::from_sqlx_no_rows)?;
        row_to_note(&row)
    }

    /// Returns every non-trashed node in the workspace as a flat list,
    /// already ordered by (parent_id, order_key). The caller assembles
    /// the tree in O(n) — keeps the SQL boring + portable.
    pub async fn list_tree(&self, workspace_id: &str) -> Result<Vec<NoteNode>, DbError> {
        let rows = sqlx::query(
            "SELECT id, parent_id, title, order_key FROM notes \
             WHERE workspace_id = ? AND trashed_at IS NULL \
             ORDER BY COALESCE(parent_id, ''), order_key, title",
        )
        .bind(workspace_id)
        .fetch_all(self.db.pool())
        .await?;
        rows.iter()
            .map(|r| {
                Ok(NoteNode {
                    id: r.get("id"),
                    parent_id: r.get("parent_id"),
                    title: r.get("title"),
                    order_key: r.get("order_key"),
                })
            })
            .collect()
    }

    /// Trashed siblings of a workspace — feeds the Notes → Trash view.
    pub async fn list_trashed(&self, workspace_id: &str) -> Result<Vec<NoteNode>, DbError> {
        let rows = sqlx::query(
            "SELECT id, parent_id, title, order_key FROM notes \
             WHERE workspace_id = ? AND trashed_at IS NOT NULL \
             ORDER BY trashed_at DESC, title",
        )
        .bind(workspace_id)
        .fetch_all(self.db.pool())
        .await?;
        rows.iter()
            .map(|r| {
                Ok(NoteNode {
                    id: r.get("id"),
                    parent_id: r.get("parent_id"),
                    title: r.get("title"),
                    order_key: r.get("order_key"),
                })
            })
            .collect()
    }

    /// Partial update — every field is `Option`, only `Some` values are
    /// written. `modified_at` always bumps. Returns the updated row.
    pub async fn update(
        &self,
        id: &str,
        title: Option<&str>,
        body: Option<&str>,
        parent_id: Option<Option<&str>>,
        order_key: Option<&str>,
    ) -> Result<Note, DbError> {
        let now_s = ts(time::OffsetDateTime::now_utc());
        // Build the SET clause dynamically; sqlx::Any doesn't have a
        // first-class query builder, so we string-build the SQL and bind
        // values in order. Every column name is a literal — no injection
        // surface — but be careful adding more fields here later.
        let mut sets: Vec<&'static str> = vec!["modified_at = ?"];
        if title.is_some() {
            sets.push("title = ?");
        }
        if body.is_some() {
            sets.push("body = ?");
        }
        if parent_id.is_some() {
            sets.push("parent_id = ?");
        }
        if order_key.is_some() {
            sets.push("order_key = ?");
        }
        let sql = format!("UPDATE notes SET {} WHERE id = ?", sets.join(", "));
        let mut q = sqlx::query(&sql).bind(&now_s);
        if let Some(v) = title {
            q = q.bind(v);
        }
        if let Some(v) = body {
            q = q.bind(v);
        }
        if let Some(v) = parent_id {
            q = q.bind(v);
        }
        if let Some(v) = order_key {
            q = q.bind(v);
        }
        q.bind(id).execute(self.db.pool()).await?;
        self.find_by_id(id).await
    }

    pub async fn trash(&self, id: &str) -> Result<(), DbError> {
        let now_s = ts(time::OffsetDateTime::now_utc());
        sqlx::query("UPDATE notes SET trashed_at = ?, modified_at = ? WHERE id = ?")
            .bind(&now_s)
            .bind(&now_s)
            .bind(id)
            .execute(self.db.pool())
            .await?;
        Ok(())
    }

    pub async fn restore(&self, id: &str) -> Result<(), DbError> {
        let now_s = ts(time::OffsetDateTime::now_utc());
        sqlx::query("UPDATE notes SET trashed_at = NULL, modified_at = ? WHERE id = ?")
            .bind(&now_s)
            .bind(id)
            .execute(self.db.pool())
            .await?;
        Ok(())
    }

    pub async fn delete(&self, id: &str) -> Result<(), DbError> {
        sqlx::query("DELETE FROM notes WHERE id = ?")
            .bind(id)
            .execute(self.db.pool())
            .await?;
        Ok(())
    }

    /// Workspace-scoped substring search across title + body. Returns
    /// `NoteNode` for the result list (title only, no body slice).
    pub async fn search(
        &self,
        workspace_id: &str,
        query: &str,
        limit: i64,
    ) -> Result<Vec<NoteNode>, DbError> {
        let pattern = format!("%{}%", query.to_lowercase());
        let rows = sqlx::query(
            "SELECT id, parent_id, title, order_key FROM notes \
             WHERE workspace_id = ? AND trashed_at IS NULL \
               AND (LOWER(title) LIKE ? OR LOWER(body) LIKE ?) \
             ORDER BY \
               CASE WHEN LOWER(title) LIKE ? THEN 0 ELSE 1 END, \
               title \
             LIMIT ?",
        )
        .bind(workspace_id)
        .bind(&pattern)
        .bind(&pattern)
        .bind(&pattern)
        .bind(limit.clamp(1, 200))
        .fetch_all(self.db.pool())
        .await?;
        rows.iter()
            .map(|r| {
                Ok(NoteNode {
                    id: r.get("id"),
                    parent_id: r.get("parent_id"),
                    title: r.get("title"),
                    order_key: r.get("order_key"),
                })
            })
            .collect()
    }

    /// Phase 3 search. Notes participate when the type filter is empty
    /// OR explicitly includes Note. Note-irrelevant filters (size,
    /// content_type, has_share_link) gate the result set to empty.
    pub async fn search_with(
        &self,
        filters: &SearchFilters,
        paging: &SearchPaging,
    ) -> Result<Vec<NoteNode>, DbError> {
        if !filters.types.is_empty() && !filters.types.contains(&TypeBucket::Note) {
            return Ok(vec![]);
        }
        if filters.has_share_link == Some(true)
            || filters.size_min.is_some()
            || filters.size_max.is_some()
        {
            return Ok(vec![]);
        }

        let mut sql = String::from(
            "SELECT id, parent_id, title, order_key, modified_at, created_at, owner_id \
             FROM notes WHERE ",
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

        match filters.in_trash {
            None | Some(false) => and(&mut sql, "trashed_at IS NULL"),
            Some(true) => and(&mut sql, "trashed_at IS NOT NULL"),
        }

        if !filters.q.is_empty() {
            and(&mut sql, "(LOWER(title) LIKE ? OR LOWER(body) LIKE ?)");
            let pat = format!("%{}%", filters.q.to_lowercase());
            binds.push(BindValue::Str(pat.clone()));
            binds.push(BindValue::Str(pat));
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

        // Notes don't have a `name` or `size` column — `name` maps to
        // `title`, `size` falls back to `modified_at`. Apply the remap
        // BEFORE building the cursor predicate so it never references a
        // non-existent column.
        let col = match paging.order_column() {
            "name" => "title",
            "size" => "modified_at",
            other => other,
        };

        if let Some((last_value, last_id)) = &paging.after {
            let cmp = match paging.sort_dir {
                crate::search::SortDir::Asc => ">",
                crate::search::SortDir::Desc => "<",
            };
            and(
                &mut sql,
                &format!("({col} {cmp} ? OR ({col} = ? AND id > ?))"),
            );
            binds.push(BindValue::Str(last_value.clone()));
            binds.push(BindValue::Str(last_value.clone()));
            binds.push(BindValue::Str(last_id.clone()));
        }

        let order_col = col;
        use std::fmt::Write;
        let _ = write!(
            sql,
            " ORDER BY {order_col} {dir}, id ASC LIMIT ?",
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
        rows.iter()
            .map(|r| {
                Ok(NoteNode {
                    id: r.get("id"),
                    parent_id: r.get("parent_id"),
                    title: r.get("title"),
                    order_key: r.get("order_key"),
                })
            })
            .collect()
    }

    /// Resolves a set of `[[Title]]` strings (already lowercased) to
    /// existing note ids in the SAME workspace. Returns a map title →
    /// id; titles that don't resolve aren't in the map.
    pub async fn resolve_titles(
        &self,
        workspace_id: &str,
        titles_lower: &[String],
    ) -> Result<std::collections::HashMap<String, String>, DbError> {
        if titles_lower.is_empty() {
            return Ok(std::collections::HashMap::new());
        }
        let placeholders = std::iter::repeat_n("?", titles_lower.len())
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "SELECT id, LOWER(title) AS t FROM notes \
             WHERE workspace_id = ? AND trashed_at IS NULL \
               AND LOWER(title) IN ({placeholders})"
        );
        let mut q = sqlx::query(&sql).bind(workspace_id);
        for t in titles_lower {
            q = q.bind(t);
        }
        let rows = q.fetch_all(self.db.pool()).await?;
        let mut out = std::collections::HashMap::with_capacity(rows.len());
        for r in rows {
            let id: String = r.get("id");
            let t: String = r.get("t");
            // First match wins. v0 doesn't disambiguate duplicate titles
            // beyond "first row from the index" — spec calls this out.
            out.entry(t).or_insert(id);
        }
        Ok(out)
    }
}

// ── Note links ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct NoteBacklink {
    pub note_id: String,
    pub title: String,
}

#[derive(Debug, Clone)]
pub struct NoteLinksRepo<'a> {
    db: &'a Db,
}

impl<'a> NoteLinksRepo<'a> {
    #[must_use]
    pub fn new(db: &'a Db) -> Self {
        Self { db }
    }

    /// Replace the entire set of outgoing links for one note. Bulk-insert
    /// with the resolved `target_id` (or NULL when dangling).
    ///
    /// Each entry is `(target_title_lower, resolved_id_opt)`. Caller
    /// resolves via `NotesRepo::resolve_titles`.
    pub async fn replace_for_note(
        &self,
        note_id: &str,
        entries: &[(String, Option<String>)],
    ) -> Result<(), DbError> {
        sqlx::query("DELETE FROM note_links WHERE note_id = ?")
            .bind(note_id)
            .execute(self.db.pool())
            .await?;
        if entries.is_empty() {
            return Ok(());
        }
        let now_s = ts(time::OffsetDateTime::now_utc());
        // Bulk insert via repeated single-row inserts. Volume is small
        // (most notes have under a dozen wiki-links) and sqlx::Any
        // doesn't expose multi-row VALUES portably.
        for (title, target_id) in entries {
            sqlx::query(
                "INSERT INTO note_links \
                 (note_id, target_title, target_id, created_at) \
                 VALUES (?, ?, ?, ?)",
            )
            .bind(note_id)
            .bind(title)
            .bind(target_id)
            .bind(&now_s)
            .execute(self.db.pool())
            .await?;
        }
        Ok(())
    }

    /// Notes that contain `[[Title]]` resolving to `note_id`, plus any
    /// dangling links whose `target_title` matches the same title even
    /// before resolution.
    pub async fn backlinks_for(
        &self,
        note_id: &str,
        note_title: &str,
    ) -> Result<Vec<NoteBacklink>, DbError> {
        let title_lower = note_title.to_lowercase();
        let rows = sqlx::query(
            "SELECT DISTINCT n.id AS id, n.title AS title \
             FROM note_links l \
             JOIN notes n ON n.id = l.note_id \
             WHERE (l.target_id = ? OR l.target_title = ?) \
               AND n.trashed_at IS NULL \
               AND n.id != ? \
             ORDER BY n.modified_at DESC \
             LIMIT 50",
        )
        .bind(note_id)
        .bind(&title_lower)
        .bind(note_id)
        .fetch_all(self.db.pool())
        .await?;
        Ok(rows
            .iter()
            .map(|r| NoteBacklink {
                note_id: r.get("id"),
                title: r.get("title"),
            })
            .collect())
    }

    /// Called when a note's title changes: re-resolve every dangling link
    /// whose target_title matches the new title, pointing them at this
    /// note. Cheap (indexed lookup + targeted UPDATE) and keeps the
    /// dangling-link surface from going stale.
    pub async fn reresolve_dangling(
        &self,
        new_title: &str,
        target_id: &str,
        _workspace_id: &str,
    ) -> Result<(), DbError> {
        let title_lower = new_title.to_lowercase();
        sqlx::query(
            "UPDATE note_links SET target_id = ? \
             WHERE target_title = ? AND target_id IS NULL",
        )
        .bind(target_id)
        .bind(&title_lower)
        .execute(self.db.pool())
        .await?;
        Ok(())
    }
}

// ── Helpers ───────────────────────────────────────────────────────────

fn row_to_note(row: &sqlx::any::AnyRow) -> Result<Note, DbError> {
    Ok(Note {
        id: row.get("id"),
        workspace_id: row.get("workspace_id"),
        parent_id: row.get("parent_id"),
        title: row.get("title"),
        body: row.get("body"),
        owner_id: row.get("owner_id"),
        order_key: row.get("order_key"),
        trashed_at: row
            .try_get::<Option<String>, _>("trashed_at")?
            .map(parse_ts)
            .transpose()?,
        created_at: parse_ts(row.get::<String, _>("created_at"))?,
        modified_at: parse_ts(row.get::<String, _>("modified_at"))?,
    })
}

/// Parse `[[…]]` tokens out of a markdown body. Returns the unique set
/// of lowercased, trimmed titles. Bounded at 256 distinct links per
/// note — beyond that, indexing isn't useful and the spam would dwarf
/// the rest of the schema.
#[must_use]
pub fn parse_wiki_links(body: &str) -> Vec<String> {
    use std::collections::BTreeSet;
    let mut out = BTreeSet::new();
    let bytes = body.as_bytes();
    let mut i = 0;
    while i + 3 < bytes.len() {
        if bytes[i] == b'[' && bytes[i + 1] == b'[' {
            // Find the closing ]]
            let start = i + 2;
            let mut j = start;
            while j + 1 < bytes.len() && !(bytes[j] == b']' && bytes[j + 1] == b']') {
                // Don't cross newlines — wiki links are inline.
                if bytes[j] == b'\n' {
                    break;
                }
                j += 1;
            }
            if j + 1 < bytes.len() && bytes[j] == b']' && bytes[j + 1] == b']' && j > start {
                // Safe slice: we matched on byte boundaries that are
                // ASCII '[' / ']' which are always at char boundaries.
                let title = body[start..j].trim();
                if !title.is_empty() && title.chars().count() <= 200 {
                    out.insert(title.to_lowercase());
                    if out.len() >= 256 {
                        break;
                    }
                }
                i = j + 2;
                continue;
            }
        }
        i += 1;
    }
    out.into_iter().collect()
}

/// Lexicographic midpoint between two strings. Used to generate
/// `order_key`s for new + moved nodes without renumbering siblings.
/// Returns a key strictly between `lo` and `hi`. Inputs use
/// alphanumeric chars only; the alphabet is `0-9a-z` (36 chars).
///
/// - `between(None, None)` → "m" (middle of the alphabet)
/// - `between(None, Some("h"))` → "d"
/// - `between(Some("h"), None)` → "u"
/// - `between(Some("a"), Some("b"))` → "am"
#[must_use]
pub fn order_key_between(lo: Option<&str>, hi: Option<&str>) -> String {
    let alphabet: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    fn idx(c: u8) -> i32 {
        // Map char to index; non-alphabet defaults to 18 (~'m').
        match c {
            b'0'..=b'9' => (c - b'0') as i32,
            b'a'..=b'z' => 10 + (c - b'a') as i32,
            _ => 18,
        }
    }
    let lo_bytes = lo.unwrap_or("").as_bytes();
    let hi_bytes = hi.unwrap_or("").as_bytes();
    let mut out: Vec<u8> = Vec::new();
    let mut i = 0;
    loop {
        let a = lo_bytes.get(i).copied().map_or(0, idx);
        let b = hi_bytes.get(i).copied().map_or(36, idx);
        // If hi is unbounded at this position, the upper bound is 36.
        let mid = (a + b) / 2;
        if mid != a {
            out.push(alphabet[mid as usize]);
            return String::from_utf8(out).unwrap_or_else(|_| "m".into());
        }
        // a == mid → need to recurse one position deeper.
        out.push(alphabet[a as usize]);
        i += 1;
        if i > 32 {
            // Pathological: extremely deep tie-breaking. Cap to avoid
            // unbounded growth; collisions become possible but rare.
            return String::from_utf8(out).unwrap_or_else(|_| "m".into());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_links() {
        let body = "Hello [[Foo]] and [[Bar]] and [[Foo]] again.";
        assert_eq!(parse_wiki_links(body), vec!["bar", "foo"]);
    }

    #[test]
    fn parse_ignores_newline_inside_brackets() {
        let body = "[[broken\nlink]] but [[good one]]";
        assert_eq!(parse_wiki_links(body), vec!["good one"]);
    }

    #[test]
    fn parse_lowercases_and_trims() {
        let body = "[[  Mixed Case  ]] [[mixed case]]";
        assert_eq!(parse_wiki_links(body), vec!["mixed case"]);
    }

    #[test]
    fn parse_ignores_empty_brackets() {
        assert!(parse_wiki_links("[[]] [[  ]] [[real]]").contains(&"real".to_string()));
        assert_eq!(parse_wiki_links("[[]] [[  ]]").len(), 0);
    }

    #[test]
    fn parse_caps_at_256() {
        let body = (0..400).fold(String::new(), |mut acc, i| {
            use std::fmt::Write;
            let _ = write!(&mut acc, "[[link{i}]] ");
            acc
        });
        assert_eq!(parse_wiki_links(&body).len(), 256);
    }

    #[test]
    fn order_keys_strictly_between() {
        let mid = order_key_between(None, None);
        let lo = order_key_between(None, Some(&mid));
        let hi = order_key_between(Some(&mid), None);
        assert!(lo.as_str() < mid.as_str());
        assert!(mid.as_str() < hi.as_str());
    }

    #[test]
    fn order_keys_chain_inserts() {
        let mut keys: Vec<String> = vec![order_key_between(None, None)];
        for _ in 0..50 {
            let new = order_key_between(None, Some(&keys[0]));
            assert!(new.as_str() < keys[0].as_str());
            keys.insert(0, new);
        }
    }
}
