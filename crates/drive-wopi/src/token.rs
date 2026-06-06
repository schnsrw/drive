//! WOPI access-token mint + verify. HMAC-SHA256 over
//! `{user_id, file_id, perms, exp, jti}` per ARCHITECTURE.md §"Three-token
//! identity model".

use std::sync::Arc;

use drive_core::FileId;
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};

use crate::WopiError;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct WopiClaims {
    pub user_id: String,
    pub file_id: FileId,
    pub perms: WopiPerms,
    pub exp: i64,
    pub jti: String,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum WopiPerms {
    #[serde(rename = "read")]
    Read,
    #[serde(rename = "write")]
    Write,
}

impl WopiPerms {
    #[must_use]
    pub fn can_write(self) -> bool {
        matches!(self, Self::Write)
    }
}

pub fn mint_token(secret: &Arc<[u8; 32]>, claims: &WopiClaims) -> String {
    encode(
        &Header::new(jsonwebtoken::Algorithm::HS256),
        claims,
        &EncodingKey::from_secret(secret.as_ref()),
    )
    .expect("HS256 encode")
}

/// Verify and parse a WOPI access token. Also enforces the URL `file_id`
/// matches the claim — the most important check in the whole WOPI layer.
pub fn verify_token(
    secret: &Arc<[u8; 32]>,
    token: &str,
    url_file_id: FileId,
) -> Result<WopiClaims, WopiError> {
    let mut v = Validation::new(jsonwebtoken::Algorithm::HS256);
    v.validate_exp = true;
    v.leeway = 0;
    let data = decode::<WopiClaims>(token, &DecodingKey::from_secret(secret.as_ref()), &v)
        .map_err(|_| WopiError::Unauthorized)?;
    if data.claims.file_id != url_file_id {
        return Err(WopiError::Unauthorized);
    }
    Ok(data.claims)
}
