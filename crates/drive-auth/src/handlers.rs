//! Sign-in + sign-out handlers.

use axum::{
    extract::State,
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use drive_db::{AuditRepo, DbError, NewAuditEvent, NewSession, NewUser, SessionRepo, UserRepo};
use serde::{Deserialize, Serialize};

use crate::{
    extractor::AuthSession,
    password::{hash_password, verify_password},
    state::AuthState,
    token::{generate_csrf_token, generate_session_id},
    AuthError,
};

const COOKIE_NAME_PROD: &str = "__Host-cd_sid";
const COOKIE_NAME_DEV: &str = "cd_sid";

#[derive(Deserialize)]
pub(crate) struct SignInBody {
    pub username: String,
    pub password: String,
}

#[derive(Serialize)]
pub(crate) struct SignInResp {
    pub csrf_token: String,
}

pub(crate) async fn sign_in(
    State(state): State<AuthState>,
    Json(body): Json<SignInBody>,
) -> Result<Response, AuthError> {
    // Phase 3 §12 — password sign-in can be disabled deployment-wide
    // once OIDC is the only sanctioned path. We return 404 (not 403) so
    // scrapers can't tell the difference between a hidden route and a
    // route that never existed.
    if !state.allow_password_auth {
        return Err(AuthError::PasswordAuthDisabled);
    }
    // Constant-time-ish: do the hash compare regardless of whether the user
    // exists, so timing doesn't disclose existence. We do this by always
    // verifying against a known-bad hash if the user lookup fails.
    let users = UserRepo::new(&state.db);
    let lookup = users.find_by_username(&body.username).await;

    let (user_id_opt, hash_for_verify) = match &lookup {
        Ok(u) => (Some(u.id.clone()), u.password_hash.clone()),
        Err(DbError::NotFound) => (None, ALWAYS_FAIL_HASH.to_string()),
        Err(e) => return Err(AuthError::Internal(e.to_string())),
    };

    let ok = verify_password(&hash_for_verify, &body.password).unwrap_or(false);
    let user_id = match (ok, user_id_opt) {
        (true, Some(id)) => id,
        _ => {
            AuditRepo::emit(
                &state.db,
                NewAuditEvent {
                    actor_id: None,
                    actor_username: None,
                    action: "auth.sign_in_failed".into(),
                    target_kind: Some("user".into()),
                    target_id: None,
                    target_name: Some(body.username.clone()),
                    ip_address: None,
                    metadata: None,
                },
            );
            return Err(AuthError::InvalidCredentials);
        }
    };

    // Mint session
    let sid = generate_session_id();
    let csrf = generate_csrf_token();
    let sessions = SessionRepo::new(&state.db);
    sessions
        .insert(
            &sid,
            &NewSession {
                user_id: user_id.clone(),
                csrf_token: csrf.clone(),
                ttl: state.session_ttl,
            },
        )
        .await
        .map_err(|e| AuthError::Internal(e.to_string()))?;

    AuditRepo::emit(
        &state.db,
        NewAuditEvent {
            actor_id: Some(user_id),
            actor_username: Some(body.username.clone()),
            action: "auth.sign_in".into(),
            target_kind: Some("session".into()),
            target_id: Some(sid.clone()),
            target_name: None,
            ip_address: None,
            metadata: None,
        },
    );

    let cookie = build_session_cookie(&sid, state.cookie_secure, state.session_ttl);
    let mut resp = (StatusCode::OK, Json(SignInResp { csrf_token: csrf })).into_response();
    resp.headers_mut()
        .insert(header::SET_COOKIE, HeaderValue::from_str(&cookie).unwrap());
    Ok(resp)
}

// ── First-run admin setup ──────────────────────────────────────────────

#[derive(Serialize)]
pub(crate) struct SetupStatus {
    pub needs_setup: bool,
}

/// Public — no auth, no session. Used by the SPA at boot to decide between
/// the sign-in card and the setup wizard.
pub(crate) async fn setup_status(
    State(state): State<AuthState>,
) -> Result<Json<SetupStatus>, AuthError> {
    let users = UserRepo::new(&state.db);
    let n = users
        .count()
        .await
        .map_err(|e| AuthError::Internal(e.to_string()))?;
    Ok(Json(SetupStatus {
        needs_setup: n == 0,
    }))
}

#[derive(Deserialize)]
pub(crate) struct SetupAdminBody {
    pub username: String,
    pub password: String,
}

/// Public — no auth, no CSRF. The zero-users invariant *is* the auth here.
/// Once the first admin exists, this endpoint returns 409 forever.
pub(crate) async fn setup_admin(
    State(state): State<AuthState>,
    Json(body): Json<SetupAdminBody>,
) -> Result<Response, AuthError> {
    let username = body.username.trim();
    if username.len() < 3 {
        return Err(AuthError::PasswordPolicy(
            "username must be at least 3 characters",
        ));
    }
    if body.password.chars().count() < 12 {
        return Err(AuthError::PasswordPolicy(
            "password must be at least 12 characters",
        ));
    }

    let users = UserRepo::new(&state.db);
    let n = users
        .count()
        .await
        .map_err(|e| AuthError::Internal(e.to_string()))?;
    if n > 0 {
        // 409 lives in AuthError as a dedicated variant so the handler can be
        // composed with the same IntoResponse machinery as the other auth flows.
        return Err(AuthError::AlreadyInitialized);
    }

    let password_hash = hash_password(&body.password)?;
    let user = users
        .insert(&NewUser {
            username: username.to_string(),
            password_hash,
            is_admin: true,
        })
        .await
        .map_err(|e| match e {
            // Lost a setup race — second caller sees 409.
            DbError::UniqueViolation(_) => AuthError::AlreadyInitialized,
            other => AuthError::Internal(other.to_string()),
        })?;

    // Auto-sign-in: mint a session for the freshly-created admin so the SPA
    // can transition straight from wizard → shell.
    let sid = generate_session_id();
    let csrf = generate_csrf_token();
    let sessions = SessionRepo::new(&state.db);
    sessions
        .insert(
            &sid,
            &NewSession {
                user_id: user.id.clone(),
                csrf_token: csrf.clone(),
                ttl: state.session_ttl,
            },
        )
        .await
        .map_err(|e| AuthError::Internal(e.to_string()))?;

    AuditRepo::emit(
        &state.db,
        NewAuditEvent {
            actor_id: Some(user.id.clone()),
            actor_username: Some(user.username.clone()),
            action: "setup.admin_created".into(),
            target_kind: Some("user".into()),
            target_id: Some(user.id.clone()),
            target_name: Some(user.username.clone()),
            ip_address: None,
            metadata: None,
        },
    );

    let cookie = build_session_cookie(&sid, state.cookie_secure, state.session_ttl);
    let mut resp = (StatusCode::OK, Json(SignInResp { csrf_token: csrf })).into_response();
    resp.headers_mut()
        .insert(header::SET_COOKIE, HeaderValue::from_str(&cookie).unwrap());
    Ok(resp)
}

#[derive(Deserialize)]
pub(crate) struct ChangePasswordBody {
    pub old_password: String,
    pub new_password: String,
}

/// Replace the caller's password and invalidate every *other* session for
/// the same user. The calling session stays alive, so the SPA doesn't get
/// kicked back to sign-in after a successful change.
///
/// Returns 204 on success, 401 on wrong old password, 422 on policy failure.
pub(crate) async fn change_password(
    State(state): State<AuthState>,
    session: AuthSession,
    Json(body): Json<ChangePasswordBody>,
) -> Result<Response, AuthError> {
    // Minimum-viable password policy. Tightening this to NIST 800-63 happens
    // alongside the haveibeenpwned check in Phase 2.
    if body.new_password.chars().count() < 12 {
        return Err(AuthError::PasswordPolicy(
            "new password must be at least 12 characters",
        ));
    }
    if body.new_password == body.old_password {
        return Err(AuthError::PasswordPolicy(
            "new password must differ from the old one",
        ));
    }

    let users = UserRepo::new(&state.db);
    let user = users
        .find_by_id(&session.user_id)
        .await
        .map_err(|e| AuthError::Internal(e.to_string()))?;

    if !verify_password(&user.password_hash, &body.old_password).unwrap_or(false) {
        return Err(AuthError::InvalidCredentials);
    }

    let new_hash = hash_password(&body.new_password)?;
    users
        .update_password(&user.id, &new_hash)
        .await
        .map_err(|e| AuthError::Internal(e.to_string()))?;

    let sessions = SessionRepo::new(&state.db);
    sessions
        .delete_for_user_except(&user.id, &session.session_id)
        .await
        .map_err(|e| AuthError::Internal(e.to_string()))?;

    AuditRepo::emit(
        &state.db,
        NewAuditEvent {
            actor_id: Some(user.id.clone()),
            actor_username: Some(user.username.clone()),
            action: "auth.password_changed".into(),
            target_kind: Some("user".into()),
            target_id: Some(user.id),
            target_name: Some(user.username),
            ip_address: None,
            metadata: None,
        },
    );

    Ok(StatusCode::NO_CONTENT.into_response())
}

pub(crate) async fn sign_out(
    State(state): State<AuthState>,
    session: AuthSession,
    headers: HeaderMap,
) -> Result<Response, AuthError> {
    let _ = &headers; // CSRF check is enforced by the router middleware in drive-http.
    let sessions = SessionRepo::new(&state.db);
    sessions
        .delete(&session.session_id)
        .await
        .map_err(|e| AuthError::Internal(e.to_string()))?;
    AuditRepo::emit(
        &state.db,
        NewAuditEvent {
            actor_id: Some(session.user_id.clone()),
            actor_username: Some(session.username.clone()),
            action: "auth.sign_out".into(),
            target_kind: Some("session".into()),
            target_id: Some(session.session_id),
            target_name: None,
            ip_address: None,
            metadata: None,
        },
    );
    let clear = clear_session_cookie(state.cookie_secure);
    let mut resp = StatusCode::NO_CONTENT.into_response();
    resp.headers_mut()
        .insert(header::SET_COOKIE, HeaderValue::from_str(&clear).unwrap());
    Ok(resp)
}

pub(crate) fn cookie_name(secure: bool) -> &'static str {
    // `__Host-` prefix requires Secure + Path=/ + no Domain — only valid over
    // HTTPS. In unencrypted dev we drop the prefix.
    if secure {
        COOKIE_NAME_PROD
    } else {
        COOKIE_NAME_DEV
    }
}

pub fn build_session_cookie(id: &str, secure: bool, ttl: time::Duration) -> String {
    let name = cookie_name(secure);
    let max_age = ttl.whole_seconds().max(0);
    let secure_part = if secure { "; Secure" } else { "" };
    format!("{name}={id}; Path=/; HttpOnly{secure_part}; SameSite=Lax; Max-Age={max_age}")
}

fn clear_session_cookie(secure: bool) -> String {
    let name = cookie_name(secure);
    let secure_part = if secure { "; Secure" } else { "" };
    format!("{name}=; Path=/; HttpOnly{secure_part}; SameSite=Lax; Max-Age=0")
}

/// A known-bad argon2id hash used to keep timing constant when the username
/// doesn't exist. Generated for password "always-fails" — verify against it
/// will never succeed for real passwords.
const ALWAYS_FAIL_HASH: &str =
    "$argon2id$v=19$m=19456,t=2,p=1$YWx3YXlzZmFpbHM$XKy+nb8s4mFcj2J3vYwS5QqXFL6jvVK0WkpHWfsxqJ8";
