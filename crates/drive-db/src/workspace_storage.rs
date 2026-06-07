//! Per-workspace BYO storage rows. Pipeline §8.9.
//! Spec: docs/research/08-byo-storage.md, docs/ux/15-byo-storage-surface.md.
//!
//! The DB only sees the opaque `secret_ct` (a base64 envelope). Encryption
//! lives in `drive-storage::secret_box`; this crate stays free of crypto.

use serde::{Deserialize, Serialize};
use sqlx::Row;

use crate::{
    users::{parse_ts, ts},
    Db, DbError,
};

/// Storage provider supported by a workspace row. Wire format matches
/// `drive_storage::byo::Provider` byte-for-byte — keep the two in sync.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WorkspaceStorageProvider {
    S3,
    Minio,
    R2,
    B2,
}

impl WorkspaceStorageProvider {
    fn as_str(self) -> &'static str {
        match self {
            Self::S3 => "s3",
            Self::Minio => "minio",
            Self::R2 => "r2",
            Self::B2 => "b2",
        }
    }
    fn parse(s: &str) -> Self {
        match s {
            "minio" => Self::Minio,
            "r2" => Self::R2,
            "b2" => Self::B2,
            _ => Self::S3,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceStorage {
    pub id: String,
    pub workspace_id: String,
    pub provider: WorkspaceStorageProvider,
    pub bucket: String,
    pub region: String,
    pub endpoint: Option<String>,
    pub access_key_id: String,
    /// Base64 envelope; never plaintext. Decrypted via
    /// `drive_storage::open_secret` with AAD `<id>:<key_version>`.
    pub secret_ct: String,
    pub key_version: i64,
    pub tested_at: Option<time::OffsetDateTime>,
    pub tested_ok: bool,
    pub tested_error: Option<String>,
    pub created_at: time::OffsetDateTime,
    pub modified_at: time::OffsetDateTime,
}

impl WorkspaceStorage {
    /// AAD string passed to the seal/open helpers. Bumping `key_version`
    /// shifts the AAD so old envelopes fail decryption — that's how we
    /// invalidate the registry cache on credential rotation.
    #[must_use]
    pub fn aad(&self) -> String {
        format!("{}:{}", self.id, self.key_version)
    }
}

#[derive(Debug, Clone)]
pub struct NewWorkspaceStorage {
    pub workspace_id: String,
    pub provider: WorkspaceStorageProvider,
    pub bucket: String,
    pub region: String,
    pub endpoint: Option<String>,
    pub access_key_id: String,
    pub secret_ct: String,
}

#[derive(Debug, Clone)]
pub struct WorkspaceStorageRepo<'a> {
    db: &'a Db,
}

impl<'a> WorkspaceStorageRepo<'a> {
    #[must_use]
    pub fn new(db: &'a Db) -> Self {
        Self { db }
    }

    /// Inserts or replaces the workspace's BYO config. The UNIQUE constraint
    /// on `workspace_id` means at most one row per workspace — we delete
    /// any existing row first so the new row gets a fresh id (which is
    /// what the AAD binds to) + `key_version = 1`.
    pub async fn upsert(&self, new: &NewWorkspaceStorage) -> Result<WorkspaceStorage, DbError> {
        sqlx::query("DELETE FROM workspace_storage WHERE workspace_id = ?")
            .bind(&new.workspace_id)
            .execute(self.db.pool())
            .await?;
        let id = ulid::Ulid::new().to_string();
        let now = time::OffsetDateTime::now_utc();
        let now_s = ts(now);
        sqlx::query(
            "INSERT INTO workspace_storage \
             (id, workspace_id, provider, bucket, region, endpoint, access_key_id, secret_ct, \
              key_version, tested_at, tested_ok, tested_error, created_at, modified_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, 1, NULL, 0, NULL, ?, ?)",
        )
        .bind(&id)
        .bind(&new.workspace_id)
        .bind(new.provider.as_str())
        .bind(&new.bucket)
        .bind(&new.region)
        .bind(&new.endpoint)
        .bind(&new.access_key_id)
        .bind(&new.secret_ct)
        .bind(&now_s)
        .bind(&now_s)
        .execute(self.db.pool())
        .await?;
        Ok(WorkspaceStorage {
            id,
            workspace_id: new.workspace_id.clone(),
            provider: new.provider,
            bucket: new.bucket.clone(),
            region: new.region.clone(),
            endpoint: new.endpoint.clone(),
            access_key_id: new.access_key_id.clone(),
            secret_ct: new.secret_ct.clone(),
            key_version: 1,
            tested_at: None,
            tested_ok: false,
            tested_error: None,
            created_at: now,
            modified_at: now,
        })
    }

    pub async fn find_by_workspace(
        &self,
        workspace_id: &str,
    ) -> Result<Option<WorkspaceStorage>, DbError> {
        let row = sqlx::query(
            "SELECT id, workspace_id, provider, bucket, region, endpoint, access_key_id, \
                    secret_ct, key_version, tested_at, tested_ok, tested_error, \
                    created_at, modified_at \
             FROM workspace_storage WHERE workspace_id = ?",
        )
        .bind(workspace_id)
        .fetch_optional(self.db.pool())
        .await?;
        row.as_ref().map(row_to_ws).transpose()
    }

    pub async fn find_by_id(&self, id: &str) -> Result<Option<WorkspaceStorage>, DbError> {
        let row = sqlx::query(
            "SELECT id, workspace_id, provider, bucket, region, endpoint, access_key_id, \
                    secret_ct, key_version, tested_at, tested_ok, tested_error, \
                    created_at, modified_at \
             FROM workspace_storage WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(self.db.pool())
        .await?;
        row.as_ref().map(row_to_ws).transpose()
    }

    /// Replaces just the secret (and bumps `key_version`). Caller already
    /// re-encrypted with the new AAD.
    pub async fn replace_secret(
        &self,
        id: &str,
        new_access_key_id: &str,
        new_secret_ct: &str,
    ) -> Result<(), DbError> {
        let now_s = ts(time::OffsetDateTime::now_utc());
        sqlx::query(
            "UPDATE workspace_storage \
             SET access_key_id = ?, secret_ct = ?, key_version = key_version + 1, \
                 tested_at = NULL, tested_ok = 0, tested_error = NULL, modified_at = ? \
             WHERE id = ?",
        )
        .bind(new_access_key_id)
        .bind(new_secret_ct)
        .bind(&now_s)
        .bind(id)
        .execute(self.db.pool())
        .await?;
        Ok(())
    }

    pub async fn delete(&self, workspace_id: &str) -> Result<(), DbError> {
        sqlx::query("DELETE FROM workspace_storage WHERE workspace_id = ?")
            .bind(workspace_id)
            .execute(self.db.pool())
            .await?;
        Ok(())
    }

    /// Records the outcome of a test-connection round-trip. `error`
    /// should be `None` on success.
    pub async fn touch_test(&self, id: &str, ok: bool, error: Option<&str>) -> Result<(), DbError> {
        let now_s = ts(time::OffsetDateTime::now_utc());
        sqlx::query(
            "UPDATE workspace_storage \
             SET tested_at = ?, tested_ok = ?, tested_error = ?, modified_at = ? \
             WHERE id = ?",
        )
        .bind(&now_s)
        .bind(i64::from(ok))
        .bind(error)
        .bind(&now_s)
        .bind(id)
        .execute(self.db.pool())
        .await?;
        Ok(())
    }
}

fn row_to_ws(row: &sqlx::any::AnyRow) -> Result<WorkspaceStorage, DbError> {
    Ok(WorkspaceStorage {
        id: row.get("id"),
        workspace_id: row.get("workspace_id"),
        provider: WorkspaceStorageProvider::parse(row.get::<String, _>("provider").as_str()),
        bucket: row.get("bucket"),
        region: row.get("region"),
        endpoint: row.get("endpoint"),
        access_key_id: row.get("access_key_id"),
        secret_ct: row.get("secret_ct"),
        key_version: row.get("key_version"),
        tested_at: row
            .try_get::<Option<String>, _>("tested_at")?
            .map(parse_ts)
            .transpose()?,
        tested_ok: row.get::<i64, _>("tested_ok") != 0,
        tested_error: row.get("tested_error"),
        created_at: parse_ts(row.get::<String, _>("created_at"))?,
        modified_at: parse_ts(row.get::<String, _>("modified_at"))?,
    })
}
