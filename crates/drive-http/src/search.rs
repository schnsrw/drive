//! `GET /api/search` — Phase 3 search endpoint.
//!
//! Spec: `docs/ux/12-search-surface.md` + `docs/research/16-scale-infra.md`
//! §"Search backend wire contract".
//!
//! Sqlite path only in this pass; the response shape is the canonical
//! one (`total`, `next_cursor`, `notes` sibling, `sort_applied`) so
//! when OpenSearch lands behind the same trait, no client change is
//! needed.

use axum::{
    extract::{Query, State},
    Json,
};
use base64::Engine;
use drive_auth::AuthSession;
use drive_db::{
    FileRepo, FolderRepo, NotesRepo, SearchFilters, SearchPaging, SortBy, SortDir, TypeBucket,
    WorkspaceMemberRepo, WorkspaceRepo,
};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use time::format_description::well_known::Rfc3339;

use crate::HttpState;

type HmacSha256 = Hmac<Sha256>;

#[derive(Deserialize)]
pub(crate) struct SearchQuery {
    pub q: Option<String>,
    pub limit: Option<i64>,
    /// Opaque, HMAC-signed cursor from a previous response. `None` ⇒
    /// first page.
    pub after: Option<String>,
    /// `relevance` | `modified` | `created` | `name` | `size`. Default
    /// `relevance` (which the sqlite path renders as `modified`).
    pub sort: Option<String>,
    pub sort_dir: Option<String>,
    /// `folder` | `workspace` | `all`. Default `workspace`.
    pub scope: Option<String>,
    pub folder_id: Option<String>,

    // Filters
    /// CSV of canonical type buckets (folder,document,spreadsheet,…).
    #[serde(rename = "type")]
    pub type_csv: Option<String>,
    /// CSV of owner ids. Repeats (`?owner=a&owner=b`) collapse to the
    /// last value with axum's default `Query` deserializer, so we use
    /// CSV here for explicit multi-value support.
    #[serde(rename = "owner")]
    pub owners_csv: Option<String>,
    /// CSV of workspace ids. Same rationale as `owners_csv`.
    #[serde(rename = "workspace")]
    pub workspaces_csv: Option<String>,
    pub modified_after: Option<String>,
    pub modified_before: Option<String>,
    pub created_after: Option<String>,
    pub created_before: Option<String>,
    pub size_min: Option<u64>,
    pub size_max: Option<u64>,
    pub has_share_link: Option<bool>,
    pub include_trashed: Option<bool>,
}

#[derive(Serialize)]
pub(crate) struct FolderDto {
    id: String,
    parent_id: Option<String>,
    name: String,
    created_at: String,
    modified_at: String,
}

#[derive(Serialize)]
pub(crate) struct FileDto {
    id: String,
    parent_id: Option<String>,
    name: String,
    size: u64,
    content_type: Option<String>,
    version: u32,
    created_at: String,
    modified_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    thumbnail: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct NoteDto {
    id: String,
    parent_id: Option<String>,
    title: String,
}

#[derive(Serialize)]
pub(crate) struct Totals {
    pub files: u32,
    pub folders: u32,
    pub notes: u32,
    /// `true` when the count is exact; `false` when the backend
    /// stopped at a cap (sqlite path with > limit). Spec §"Pagination".
    pub exact: bool,
}

#[derive(Serialize)]
pub(crate) struct SearchResp {
    pub folders: Vec<FolderDto>,
    pub files: Vec<FileDto>,
    pub notes: Vec<NoteDto>,
    pub total: Totals,
    /// Opaque cursor for the next page; `null` at end-of-results.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    /// What the backend actually sorted by — relevant when the SPA
    /// requested `relevance` and the sqlite path fell back to
    /// `modified`. Spec §"Sort semantics".
    pub sort_applied: String,
}

pub(crate) async fn search(
    State(s): State<HttpState>,
    session: AuthSession,
    Query(q): Query<SearchQuery>,
) -> Result<Json<SearchResp>, axum::http::StatusCode> {
    let raw_q = q.q.as_deref().map_or("", str::trim).to_string();
    let limit = q.limit.unwrap_or(30).clamp(10, 100);

    // ── Scope + workspace resolution ─────────────────────────────────
    let scope = q.scope.as_deref().unwrap_or("workspace");
    let workspace_ids: Vec<String> = match scope {
        "all" => {
            // Every workspace the caller is a member of.
            let mine = WorkspaceRepo::new(&s.db)
                .list_for_user(&session.user_id)
                .await
                .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;
            mine.into_iter().map(|w| w.id).collect()
        }
        _ => {
            // Caller-provided ?workspace= ids must all be ones they
            // belong to. If none provided, fall back to active.
            let candidates: Vec<String> = split_csv(q.workspaces_csv.as_deref());
            if candidates.is_empty() {
                let active =
                    crate::workspaces::resolve_active_workspace(&s.db, &session.user_id, None)
                        .await
                        .map_err(|_| axum::http::StatusCode::FORBIDDEN)?;
                vec![active]
            } else {
                let members = WorkspaceMemberRepo::new(&s.db);
                let mut allowed = Vec::with_capacity(candidates.len());
                for c in candidates {
                    let role = members
                        .role_of(&c, &session.user_id)
                        .await
                        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;
                    if role.is_some() {
                        allowed.push(c);
                    }
                }
                if allowed.is_empty() {
                    return Err(axum::http::StatusCode::FORBIDDEN);
                }
                allowed
            }
        }
    };
    if workspace_ids.is_empty() {
        return Ok(Json(empty_resp(&q)));
    }

    // ── Filters ──────────────────────────────────────────────────────
    let types: Vec<TypeBucket> = q
        .type_csv
        .as_deref()
        .unwrap_or("")
        .split(',')
        .filter_map(|t| {
            let t = t.trim();
            if t.is_empty() {
                None
            } else {
                TypeBucket::from_name(t)
            }
        })
        .collect();

    // No query AND no filters ⇒ behaviour unchanged — empty result so
    // the SPA renders the current folder.
    let owner_ids = split_csv(q.owners_csv.as_deref());
    let any_filter_set = !types.is_empty()
        || !owner_ids.is_empty()
        || q.modified_after.is_some()
        || q.modified_before.is_some()
        || q.created_after.is_some()
        || q.created_before.is_some()
        || q.size_min.is_some()
        || q.size_max.is_some()
        || q.has_share_link.is_some()
        || q.include_trashed.is_some();
    if raw_q.is_empty() && !any_filter_set {
        return Ok(Json(empty_resp(&q)));
    }

    // Tri-state trash filter: `include_trashed=true` → return trashed
    // and non-trashed; default (and explicit false) → exclude trashed.
    // To request only-trashed Phase B can add a separate flag.
    let in_trash = match q.include_trashed {
        Some(true) => None,
        _ => Some(false),
    };

    let filters = SearchFilters {
        q: raw_q.clone(),
        workspace_ids: workspace_ids.clone(),
        folder_id: if scope == "folder" {
            q.folder_id.clone()
        } else {
            None
        },
        types: types.clone(),
        owner_ids: owner_ids.clone(),
        modified_after: q.modified_after.as_deref().and_then(parse_rfc3339),
        modified_before: q.modified_before.as_deref().and_then(parse_rfc3339),
        created_after: q.created_after.as_deref().and_then(parse_rfc3339),
        created_before: q.created_before.as_deref().and_then(parse_rfc3339),
        size_min: q.size_min,
        size_max: q.size_max,
        has_share_link: q.has_share_link,
        in_trash,
    };

    // ── Sort + paging ────────────────────────────────────────────────
    let sort_by = match q.sort.as_deref().unwrap_or("relevance") {
        "modified" => SortBy::Modified,
        "created" => SortBy::Created,
        "name" => SortBy::Name,
        "size" => SortBy::Size,
        _ => SortBy::Relevance,
    };
    // sqlite path can't compute BM25; fall back to modified, surface
    // the fallback in `sort_applied`.
    let sort_applied = if matches!(sort_by, SortBy::Relevance) {
        "modified".to_string()
    } else {
        match sort_by {
            SortBy::Modified => "modified",
            SortBy::Created => "created",
            SortBy::Name => "name",
            SortBy::Size => "size",
            SortBy::Relevance => "modified",
        }
        .to_string()
    };
    let sort_dir = match (q.sort_dir.as_deref(), sort_by) {
        (Some("asc"), _) => SortDir::Asc,
        (Some("desc"), _) => SortDir::Desc,
        (None, SortBy::Name) => SortDir::Asc, // Name defaults A→Z
        _ => SortDir::Desc,
    };

    let filter_hash = compute_filter_hash(&filters);
    let after_tuple = q
        .after
        .as_deref()
        .map(|c| decode_cursor(c, &filter_hash, &s.config.signed_url_hmac_secret))
        .transpose()
        .map_err(|_| axum::http::StatusCode::BAD_REQUEST)?;
    let paging = SearchPaging {
        sort_by,
        sort_dir,
        after: after_tuple,
        limit,
    };

    // ── Query each repo ──────────────────────────────────────────────
    let file_rows = FileRepo::new(&s.db)
        .search_with(&filters, &paging)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "file search failed");
            axum::http::StatusCode::INTERNAL_SERVER_ERROR
        })?;
    let folder_rows = FolderRepo::new(&s.db)
        .search_with(&filters, &paging)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "folder search failed");
            axum::http::StatusCode::INTERNAL_SERVER_ERROR
        })?;
    let note_rows = NotesRepo::new(&s.db)
        .search_with(&filters, &paging)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "note search failed");
            axum::http::StatusCode::INTERNAL_SERVER_ERROR
        })?;

    // Each repo returned up to limit+1 to signal has-more locally.
    // We compute the merged page first, then derive `next_cursor` from
    // the last merged row that came out of any kind.
    let file_count = file_rows.len() as u32;
    let folder_count = folder_rows.len() as u32;
    let note_count = note_rows.len() as u32;

    // For the sqlite path each kind paginates independently when the
    // SPA scrolls. We honour the merge by interleaving up to `limit`
    // rows total — taking the first `limit` from each kind. Phase A
    // ships per-kind cursors via one shared token; the cursor encodes
    // the merged tail position so the next page picks up.
    let take = limit as usize;
    let files: Vec<FileDto> = file_rows
        .iter()
        .take(take)
        .map(|f| FileDto {
            id: f.id.clone(),
            parent_id: f.parent_id.clone(),
            name: f.name.clone(),
            size: f.size,
            content_type: f.content_type.clone(),
            version: f.version,
            created_at: rfc3339(f.created_at),
            modified_at: rfc3339(f.modified_at),
            thumbnail: f.thumbnail.clone(),
        })
        .collect();
    let folders: Vec<FolderDto> = folder_rows
        .iter()
        .take(take)
        .map(|f| FolderDto {
            id: f.id.clone(),
            parent_id: f.parent_id.clone(),
            name: f.name.clone(),
            created_at: rfc3339(f.created_at),
            modified_at: rfc3339(f.modified_at),
        })
        .collect();
    let notes: Vec<NoteDto> = note_rows
        .iter()
        .take(take)
        .map(|n| NoteDto {
            id: n.id.clone(),
            parent_id: n.parent_id.clone(),
            title: n.title.clone(),
        })
        .collect();

    // has_more if any repo returned more than `take` rows.
    let has_more = file_rows.len() > take || folder_rows.len() > take || note_rows.len() > take;
    let next_cursor = if has_more {
        // Pick the last visible row across kinds — the one with the
        // largest sort_value seen (for DESC) / smallest (for ASC).
        let mut last: Option<(String, String)> = None;
        let cmp = |a: &str, b: &str| match paging.sort_dir {
            SortDir::Desc => a < b, // we keep the smaller as the "tail" boundary
            SortDir::Asc => a > b,
        };
        if let Some(f) = files.last() {
            let v = sort_value_for_file(f, paging.sort_by);
            last = Some((v, f.id.clone()));
        }
        if let Some(f) = folders.last() {
            let v = sort_value_for_folder(f, paging.sort_by);
            if last.as_ref().is_none_or(|(lv, _)| cmp(lv, &v)) {
                last = Some((v, f.id.clone()));
            }
        }
        if let Some(n) = notes.last() {
            // Notes don't carry modified/created/size in the DTO; use
            // their id as the tail boundary on (already sorted) rows.
            // This is a Phase A simplification — Phase B can lift the
            // sort fields into the DTO.
            let v = String::new();
            if last.as_ref().is_none_or(|(lv, _)| cmp(lv, &v)) {
                last = Some((v, n.id.clone()));
            }
        }
        last.map(|(v, id)| encode_cursor(&v, &id, &filter_hash, &s.config.signed_url_hmac_secret))
    } else {
        None
    };

    Ok(Json(SearchResp {
        folders,
        files,
        notes,
        total: Totals {
            files: file_count.min(take as u32),
            folders: folder_count.min(take as u32),
            notes: note_count.min(take as u32),
            exact: !has_more,
        },
        next_cursor,
        sort_applied,
    }))
}

fn empty_resp(q: &SearchQuery) -> SearchResp {
    SearchResp {
        folders: vec![],
        files: vec![],
        notes: vec![],
        total: Totals {
            files: 0,
            folders: 0,
            notes: 0,
            exact: true,
        },
        next_cursor: None,
        sort_applied: q.sort.clone().unwrap_or_else(|| "modified".into()),
    }
}

fn sort_value_for_file(f: &FileDto, sort_by: SortBy) -> String {
    match sort_by {
        SortBy::Relevance | SortBy::Modified => f.modified_at.clone(),
        SortBy::Created => f.created_at.clone(),
        SortBy::Name => f.name.clone(),
        SortBy::Size => format!("{:020}", f.size),
    }
}

fn sort_value_for_folder(f: &FolderDto, sort_by: SortBy) -> String {
    match sort_by {
        SortBy::Relevance | SortBy::Modified => f.modified_at.clone(),
        SortBy::Created => f.created_at.clone(),
        SortBy::Name => f.name.clone(),
        // Folders have no size; sort by modified as the safe fallback.
        SortBy::Size => f.modified_at.clone(),
    }
}

fn rfc3339(t: time::OffsetDateTime) -> String {
    t.format(&Rfc3339).unwrap_or_default()
}

fn parse_rfc3339(s: &str) -> Option<time::OffsetDateTime> {
    time::OffsetDateTime::parse(s, &Rfc3339).ok()
}

fn split_csv(s: Option<&str>) -> Vec<String> {
    s.map(|raw| {
        raw.split(',')
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .map(str::to_string)
            .collect()
    })
    .unwrap_or_default()
}

// ── Cursor encoding ──────────────────────────────────────────────────
//
// Plaintext payload: `<last_value>\n<last_id>\n<filter_hash>`
// Encoded: base64url(payload_bytes || hmac_tag)
//
// `filter_hash` binds the cursor to the exact filter combination —
// a cursor from search X can't drift into search Y because the hash
// won't match. Tampering with the body fails the HMAC check.

fn encode_cursor(last_value: &str, last_id: &str, filter_hash: &str, key: &[u8; 32]) -> String {
    let payload = format!("{last_value}\n{last_id}\n{filter_hash}");
    let mut mac = HmacSha256::new_from_slice(key).unwrap();
    mac.update(payload.as_bytes());
    let tag = mac.finalize().into_bytes();
    let mut combined = Vec::with_capacity(payload.len() + 32);
    combined.extend_from_slice(payload.as_bytes());
    combined.extend_from_slice(&tag);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(combined)
}

fn decode_cursor(
    cursor: &str,
    expected_filter_hash: &str,
    key: &[u8; 32],
) -> Result<(String, String), &'static str> {
    let raw = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(cursor.as_bytes())
        .map_err(|_| "invalid base64")?;
    if raw.len() < 32 {
        return Err("cursor too short");
    }
    let (payload_bytes, tag) = raw.split_at(raw.len() - 32);
    let payload = std::str::from_utf8(payload_bytes).map_err(|_| "invalid utf-8 payload")?;

    let mut mac = HmacSha256::new_from_slice(key).map_err(|_| "hmac key")?;
    mac.update(payload_bytes);
    let expected_tag = mac.finalize().into_bytes();
    if expected_tag.ct_eq(tag).unwrap_u8() != 1 {
        return Err("hmac mismatch");
    }

    let mut parts = payload.splitn(3, '\n');
    let last_value = parts.next().ok_or("missing last_value")?.to_string();
    let last_id = parts.next().ok_or("missing last_id")?.to_string();
    let filter_hash = parts.next().ok_or("missing filter_hash")?;
    if filter_hash != expected_filter_hash {
        // The filter set changed since this cursor was issued — refuse
        // the pagination jump rather than silently returning the wrong
        // page. The SPA's response is to clear pagination + restart.
        return Err("filter set changed");
    }
    Ok((last_value, last_id))
}

fn compute_filter_hash(f: &SearchFilters) -> String {
    let mut h = Sha256::new();
    h.update(f.q.as_bytes());
    h.update(b"\0");
    for w in &f.workspace_ids {
        h.update(w.as_bytes());
        h.update(b",");
    }
    h.update(b"\0");
    h.update(f.folder_id.as_deref().unwrap_or("").as_bytes());
    h.update(b"\0");
    let mut t = f.types.iter().map(|t| format!("{t:?}")).collect::<Vec<_>>();
    t.sort();
    h.update(t.join(",").as_bytes());
    h.update(b"\0");
    let mut o = f.owner_ids.clone();
    o.sort();
    h.update(o.join(",").as_bytes());
    h.update(b"\0");
    for opt in [
        f.modified_after,
        f.modified_before,
        f.created_after,
        f.created_before,
    ] {
        h.update(opt.map(rfc3339).unwrap_or_default().as_bytes());
        h.update(b",");
    }
    h.update(b"\0");
    h.update(f.size_min.unwrap_or(0).to_string().as_bytes());
    h.update(b",");
    h.update(f.size_max.unwrap_or(0).to_string().as_bytes());
    h.update(b"\0");
    h.update(format!("{:?}", f.has_share_link).as_bytes());
    h.update(b"\0");
    h.update(format!("{:?}", f.in_trash).as_bytes());

    let digest = h.finalize();
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&digest[..12])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_round_trips_and_rejects_tampering() {
        let key = [7u8; 32];
        let fh = "abc123";
        let c = encode_cursor("2026-06-08T10:00:00Z", "01HXYZ", fh, &key);
        let (v, id) = decode_cursor(&c, fh, &key).unwrap();
        assert_eq!(v, "2026-06-08T10:00:00Z");
        assert_eq!(id, "01HXYZ");

        // Tamper with the cursor.
        let mut bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(&c)
            .unwrap();
        let mid = bytes.len() / 2;
        bytes[mid] ^= 0x01;
        let bad = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&bytes);
        assert!(decode_cursor(&bad, fh, &key).is_err());
    }

    #[test]
    fn cursor_rejects_wrong_filter_hash() {
        let key = [7u8; 32];
        let c = encode_cursor("v", "i", "filter-A", &key);
        assert!(decode_cursor(&c, "filter-B", &key).is_err());
    }

    #[test]
    fn filter_hash_is_deterministic_and_filter_sensitive() {
        let a = SearchFilters {
            q: "kickoff".into(),
            workspace_ids: vec!["ws1".into()],
            types: vec![TypeBucket::Pdf],
            ..Default::default()
        };
        let mut b = a.clone();
        b.types = vec![TypeBucket::Image];
        assert_ne!(compute_filter_hash(&a), compute_filter_hash(&b));
        assert_eq!(compute_filter_hash(&a), compute_filter_hash(&a.clone()));
    }
}
