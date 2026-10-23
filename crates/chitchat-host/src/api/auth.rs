use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::Arc;

use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHash, SaltString};
use argon2::{Argon2, PasswordHasher, PasswordVerifier};
use axum::Json;
use axum::extract::ConnectInfo;
use axum::http::StatusCode;
use rand::RngCore;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{ApiError, ApiResult, Auth, Body, Srv};
use crate::db;
use crate::events::{CloseTarget, Event};
use crate::host::ServerCtx;
use crate::ids::next_id;
use crate::models::{UserOut, UserRow, now_ms};

// ---------------------------------------------------------------- helpers

pub fn token_hash(token: &str) -> String {
    blake3::hash(token.as_bytes()).to_hex().to_string()
}

fn new_token() -> String {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub async fn hash_password(password: String) -> ApiResult<String> {
    tokio::task::spawn_blocking(move || {
        let salt = SaltString::generate(&mut OsRng);
        Argon2::default().hash_password(password.as_bytes(), &salt).map(|h| h.to_string())
    })
    .await
    .map_err(|_| ApiError::internal())?
    .map_err(|_| ApiError::internal())
}

async fn verify_password(password: String, hash: String) -> bool {
    tokio::task::spawn_blocking(move || PasswordHash::new(&hash).is_ok_and(|h| Argon2::default().verify_password(password.as_bytes(), &h).is_ok())).await.unwrap_or(false)
}

/// Looks up a session token and builds the caller's permissions.
pub async fn authenticate(srv: Arc<ServerCtx>, token: &str) -> ApiResult<Auth> {
    let hash = token_hash(token);
    let user: Option<UserRow> = sqlx::query_as("SELECT u.* FROM sessions s JOIN users u ON u.id = s.user_id WHERE s.token_hash = ?").bind(&hash).fetch_optional(&srv.db).await?;
    let user = user.ok_or_else(ApiError::unauthorized)?;
    if super::moderation::user_banned(&srv, user.id).await? {
        return Err(ApiError::forbidden("banned", "You are banned from this server."));
    }
    sqlx::query("UPDATE sessions SET last_used_at = ? WHERE token_hash = ? AND last_used_at < ?").bind(now_ms()).bind(&hash).bind(now_ms() - 60_000).execute(&srv.db).await?;
    let (perms, is_owner) = db::user_perms(&srv.db, user.id).await?;
    Ok(Auth { srv, user, perms, is_owner, token_hash: hash })
}

pub(super) async fn create_session(srv: &ServerCtx, user_id: i64, device_id: &str, addr: SocketAddr) -> ApiResult<String> {
    let token = new_token();
    let now = now_ms();
    sqlx::query("INSERT INTO sessions (token_hash, user_id, device_id, ip, created_at, last_used_at) VALUES (?, ?, ?, ?, ?, ?)")
        .bind(token_hash(&token))
        .bind(user_id)
        .bind(device_id)
        .bind(addr.ip().to_string())
        .bind(now)
        .bind(now)
        .execute(&srv.db)
        .await?;
    Ok(token)
}

pub(super) fn clean_device_id(d: &str) -> String {
    let d: String = d.trim().chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').take(64).collect();
    if d.is_empty() { "unknown".into() } else { d }
}

// ---------------------------------------------------------------- validation

pub fn check_username(raw: &str) -> Result<String, &'static str> {
    let u = raw.trim().to_lowercase();
    if u.chars().count() < 2 {
        return Err("too_short");
    }
    if u.chars().count() > 32 {
        return Err("too_long");
    }
    if !u.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '.') {
        return Err("invalid_characters");
    }
    Ok(u)
}

pub fn check_display_name(raw: &str) -> Result<String, &'static str> {
    let d = raw.trim();
    if d.is_empty() {
        return Err("too_short");
    }
    if d.chars().count() > 32 {
        return Err("too_long");
    }
    if d.chars().any(char::is_control) {
        return Err("invalid_characters");
    }
    Ok(d.to_string())
}

pub fn check_password(p: &str) -> Result<(), &'static str> {
    match p.chars().count() {
        n if n < 8 => Err("too_short"),
        n if n > 128 => Err("too_long"),
        _ => Ok(()),
    }
}

// ---------------------------------------------------------------- handlers

#[derive(Deserialize)]
pub struct RegisterReq {
    username: String,
    password: String,
    display_name: String,
    server_password: Option<String>,
    invite_code: Option<String>,
    setup_code: Option<String>,
    #[serde(default)]
    device_id: String,
}

pub async fn register(Srv(srv): Srv, ConnectInfo(addr): ConnectInfo<SocketAddr>, Body(req): Body<RegisterReq>) -> ApiResult<(StatusCode, Json<Value>)> {
    let mut fields = BTreeMap::new();
    let username = check_username(&req.username).map_err(|e| fields.insert("username", e)).ok();
    let display_name = check_display_name(&req.display_name).map_err(|e| fields.insert("display_name", e)).ok();
    let _ = check_password(&req.password).map_err(|e| fields.insert("password", e));
    let (Some(username), Some(display_name), true) = (username, display_name, fields.is_empty()) else {
        return Err(ApiError::validation(fields));
    };

    let server = db::server_row(&srv.db).await?;
    let setup_code = req.setup_code.as_deref().map(|c| c.trim().to_uppercase()).filter(|c| !c.is_empty());
    let invite_code = req.invite_code.as_deref().map(str::trim).filter(|c| !c.is_empty()).map(str::to_string);
    let claiming_owner = match (&setup_code, &server.setup_code, server.owner_id) {
        (Some(given), Some(real), None) if given == real => true,
        (Some(_), _, _) => {
            return Err(ApiError::forbidden("invalid_setup_code", "That setup code is wrong or was already used."));
        }
        _ => false,
    };

    let device_id = clean_device_id(&req.device_id);
    if !claiming_owner {
        super::moderation::check_join(&srv, addr.ip(), &device_id).await?;
        if !server.registration_open {
            return Err(ApiError::forbidden("registration_closed", "This server isn't accepting new accounts."));
        }
        if let Some(hash) = &server.password_hash {
            let Some(given) = req.server_password.clone().filter(|p| !p.is_empty()) else {
                return Err(ApiError::forbidden("server_password_required", "This server needs a password to join."));
            };
            if !verify_password(given, hash.clone()).await {
                return Err(ApiError::forbidden("wrong_server_password", "Wrong server password."));
            }
        }
        if server.invite_only && invite_code.is_none() {
            return Err(ApiError::forbidden("invite_required", "You need an invite to join this server."));
        }
        super::moderation::check_accounts_per_ip(&srv, addr.ip(), server.max_accounts_per_ip).await?;
    }

    let password_hash = hash_password(req.password).await?;
    let user = UserRow {
        id: next_id(),
        username,
        display_name,
        password_hash,
        avatar_url: None,
        status: "online".into(),
        status_text: String::new(),
        created_at: now_ms(),
        timed_out_until: None,
        signup_ip: Some(crate::ipfilter::normalize(addr.ip()).to_string()),
        signup_device: Some(device_id.clone()),
    };

    let mut tx = srv.db.begin().await?;
    let inserted = sqlx::query("INSERT INTO users (id, username, display_name, password_hash, created_at, signup_ip, signup_device) VALUES (?, ?, ?, ?, ?, ?, ?)")
        .bind(user.id)
        .bind(&user.username)
        .bind(&user.display_name)
        .bind(&user.password_hash)
        .bind(user.created_at)
        .bind(&user.signup_ip)
        .bind(&user.signup_device)
        .execute(&mut *tx)
        .await;
    if let Err(e) = inserted {
        if e.as_database_error().is_some_and(|d| d.is_unique_violation()) {
            return Err(ApiError::new(StatusCode::CONFLICT, "username_taken", "That username is already taken."));
        }
        return Err(e.into());
    }
    if let (Some(code), false) = (&invite_code, claiming_owner) {
        super::admin::consume_invite(&mut tx, code).await?;
    }
    if claiming_owner {
        let claimed = sqlx::query("UPDATE server SET owner_id = ?, setup_code = NULL WHERE owner_id IS NULL AND setup_code = ?")
            .bind(user.id)
            .bind(setup_code.as_deref())
            .execute(&mut *tx)
            .await?;
        if claimed.rows_affected() != 1 {
            return Err(ApiError::forbidden("invalid_setup_code", "That setup code is wrong or was already used."));
        }
    }
    tx.commit().await?;

    if claiming_owner {
        tracing::info!(server = %srv.slug, user = %user.username, "owner claimed the server");
        if let Ok(row) = db::server_row(&srv.db).await {
            srv.emit(Event::new("server_update", crate::models::ServerOut::new(&srv.slug, &row)));
        }
    }
    if let Some(m) = db::member(&srv, user.id).await? {
        srv.emit(Event::new("member_join", m));
    }
    let token = create_session(&srv, user.id, &device_id, addr).await?;
    Ok((StatusCode::CREATED, Json(json!({ "token": token, "user": UserOut::from(&user) }))))
}

#[derive(Deserialize)]
pub struct LoginReq {
    username: String,
    password: String,
    #[serde(default)]
    device_id: String,
}

pub async fn login(Srv(srv): Srv, ConnectInfo(addr): ConnectInfo<SocketAddr>, Body(req): Body<LoginReq>) -> ApiResult<Json<Value>> {
    let invalid = || ApiError::new(StatusCode::UNAUTHORIZED, "invalid_credentials", "Wrong username or password.");
    let username = req.username.trim().to_lowercase();
    let user: Option<UserRow> = sqlx::query_as("SELECT * FROM users WHERE username = ?").bind(&username).fetch_optional(&srv.db).await?;
    let Some(user) = user else { return Err(invalid()) };
    if !verify_password(req.password, user.password_hash.clone()).await {
        return Err(invalid());
    }
    let device_id = clean_device_id(&req.device_id);
    let (_, is_owner) = db::user_perms(&srv.db, user.id).await?;
    if !is_owner {
        if super::moderation::user_banned(&srv, user.id).await? {
            return Err(ApiError::forbidden("banned", "You are banned from this server."));
        }
        super::moderation::check_join(&srv, addr.ip(), &device_id).await?;
    }
    let token = create_session(&srv, user.id, &device_id, addr).await?;
    Ok(Json(json!({ "token": token, "user": UserOut::from(&user) })))
}

pub async fn logout(auth: Auth) -> ApiResult<StatusCode> {
    sqlx::query("DELETE FROM sessions WHERE token_hash = ?").bind(&auth.token_hash).execute(&auth.srv.db).await?;
    auth.srv.emit(Event::close(CloseTarget {
        token_hash: Some(auth.token_hash.clone()),
        user_id: None,
        except_token: None,
        close_code: super::ws::CLOSE_UNAUTHORIZED,
        error_code: "unauthorized",
        message: "You logged out.".into(),
    }));
    Ok(StatusCode::NO_CONTENT)
}

pub async fn me(auth: Auth) -> Json<Value> {
    Json(json!({
        "user": UserOut::from(&auth.user),
        "permissions": auth.perms.to_string(),
        "is_owner": auth.is_owner,
    }))
}

#[derive(Deserialize)]
pub struct PasswordChange {
    old: String,
    new: String,
}

#[derive(Deserialize)]
pub struct PatchMeReq {
    display_name: Option<String>,
    password: Option<PasswordChange>,
    status: Option<String>,
    status_text: Option<String>,
}

pub async fn patch_me(auth: Auth, Body(req): Body<PatchMeReq>) -> ApiResult<Json<UserOut>> {
    let mut fields = BTreeMap::new();
    let display_name = match &req.display_name {
        Some(d) => check_display_name(d).map_err(|e| fields.insert("display_name", e)).ok(),
        None => None,
    };
    if let Some(p) = &req.password {
        let _ = check_password(&p.new).map_err(|e| fields.insert("password", e));
    }
    if req.status.as_deref().is_some_and(|s| !matches!(s, "online" | "away")) {
        fields.insert("status", "invalid");
    }
    if let Some(text) = &req.status_text
        && (text.chars().count() > 128 || text.chars().any(char::is_control))
    {
        fields.insert("status_text", "invalid");
    }
    if !fields.is_empty() {
        return Err(ApiError::validation(fields));
    }

    if let Some(p) = req.password {
        if !verify_password(p.old, auth.user.password_hash.clone()).await {
            return Err(ApiError::new(StatusCode::UNAUTHORIZED, "invalid_credentials", "Your current password is wrong."));
        }
        let hash = hash_password(p.new).await?;
        sqlx::query("UPDATE users SET password_hash = ? WHERE id = ?").bind(hash).bind(auth.user.id).execute(&auth.srv.db).await?;
        // Log out every other device.
        sqlx::query("DELETE FROM sessions WHERE user_id = ? AND token_hash != ?").bind(auth.user.id).bind(&auth.token_hash).execute(&auth.srv.db).await?;
        auth.srv.emit(Event::close(CloseTarget {
            token_hash: None,
            user_id: Some(auth.user.id),
            except_token: Some(auth.token_hash.clone()),
            close_code: super::ws::CLOSE_UNAUTHORIZED,
            error_code: "unauthorized",
            message: "Your password was changed. Please log in again.".into(),
        }));
    }
    if let Some(d) = &display_name {
        sqlx::query("UPDATE users SET display_name = ? WHERE id = ?").bind(d).bind(auth.user.id).execute(&auth.srv.db).await?;
    }
    if let Some(status) = &req.status {
        sqlx::query("UPDATE users SET status = ? WHERE id = ?").bind(status).bind(auth.user.id).execute(&auth.srv.db).await?;
    }
    if let Some(text) = &req.status_text {
        sqlx::query("UPDATE users SET status_text = ? WHERE id = ?").bind(text.trim()).bind(auth.user.id).execute(&auth.srv.db).await?;
    }

    let user = db::user_by_id(&auth.srv.db, auth.user.id).await?.ok_or_else(ApiError::internal)?;
    if (display_name.is_some() || req.status.is_some() || req.status_text.is_some())
        && let Some(m) = db::member(&auth.srv, user.id).await?
    {
        auth.srv.emit(Event::new("member_update", m));
    }
    if req.status.is_some() {
        auth.srv.emit(super::profile::presence_event(&auth.srv, &user));
    }
    Ok(Json(UserOut::from(&user)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usernames() {
        assert_eq!(check_username(" Mario.B_1 "), Ok("mario.b_1".into()));
        assert_eq!(check_username("a"), Err("too_short"));
        assert_eq!(check_username("bad name"), Err("invalid_characters"));
        assert_eq!(check_display_name("  "), Err("too_short"));
        assert!(check_password("1234567").is_err());
        assert!(check_password("12345678").is_ok());
    }
}
