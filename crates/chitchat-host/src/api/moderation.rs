//! Moderation (Phase 4): bans (optionally by IP and device, against ban evasion), timeouts, kicks,
//! the owner's anti-alt rules, and the audit log.

use std::collections::BTreeMap;
use std::net::IpAddr;

use axum::Json;
use axum::extract::{Path, Query};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::extract::parse_id;
use super::{ApiError, ApiResult, Auth, Body};
use crate::db;
use crate::events::{CloseTarget, Event};
use crate::host::ServerCtx;
use crate::ids::next_id;
use crate::models::{UserOut, UserRow, now_ms, ts};
use crate::perms::{self, BAN_MEMBERS, KICK_MEMBERS, MANAGE_MESSAGES, MANAGE_SERVER};

const HOUR_MS: i64 = 3_600_000;

// ---------------------------------------------------------------- audit log

/// Records an action. `actor` None = done automatically by the server (e.g. the spam filter).
pub async fn audit(srv: &ServerCtx, actor: Option<i64>, action: &str, target: Option<i64>, details: Value) {
    let r = sqlx::query("INSERT INTO audit_log (id, actor_id, action, target_id, details, created_at) VALUES (?, ?, ?, ?, ?, ?)")
        .bind(next_id())
        .bind(actor)
        .bind(action)
        .bind(target)
        .bind(details.to_string())
        .bind(now_ms())
        .execute(&srv.db)
        .await;
    if let Err(e) = r {
        tracing::warn!(error = %e, "couldn't write the audit log");
    }
}

#[derive(Deserialize)]
pub struct AuditQuery {
    before: Option<String>,
    limit: Option<i64>,
}

#[derive(sqlx::FromRow)]
struct AuditRow {
    id: i64,
    actor_id: Option<i64>,
    action: String,
    target_id: Option<i64>,
    details: String,
    created_at: i64,
}

pub async fn audit_log(auth: Auth, Query(q): Query<AuditQuery>) -> ApiResult<Json<Vec<Value>>> {
    if !perms::has(auth.perms, MANAGE_SERVER) && !perms::has(auth.perms, BAN_MEMBERS) && !perms::has(auth.perms, KICK_MEMBERS) {
        return Err(ApiError::missing_permission());
    }
    let limit = q.limit.unwrap_or(50).clamp(1, 100);
    let before = q.before.as_deref().and_then(crate::ids::parse).unwrap_or(i64::MAX);
    let rows: Vec<AuditRow> = sqlx::query_as("SELECT * FROM audit_log WHERE id < ? ORDER BY id DESC LIMIT ?").bind(before).bind(limit).fetch_all(&auth.srv.db).await?;
    Ok(Json(
        rows.into_iter()
            .map(|r| {
                json!({
                    "id": r.id.to_string(),
                    "actor_id": r.actor_id.map(|v| v.to_string()),
                    "action": r.action,
                    "target_id": r.target_id.map(|v| v.to_string()),
                    "details": serde_json::from_str::<Value>(&r.details).unwrap_or(Value::Null),
                    "created_at": ts(r.created_at),
                })
            })
            .collect(),
    ))
}

// ---------------------------------------------------------------- checks used elsewhere

/// True when this user has an active ban.
pub async fn user_banned(srv: &ServerCtx, user_id: i64) -> sqlx::Result<bool> {
    let n: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM bans WHERE user_id = ? AND (expires_at IS NULL OR expires_at > ?)").bind(user_id).bind(now_ms()).fetch_one(&srv.db).await?;
    Ok(n > 0)
}

fn banned_error() -> ApiError {
    ApiError::forbidden("banned", "You are banned from this server.")
}

/// Ban-evasion check on register/login: a banned device or address can't make or use accounts.
pub async fn check_join(srv: &ServerCtx, ip: IpAddr, device_id: &str) -> ApiResult<()> {
    let ip = crate::ipfilter::normalize(ip);
    let n: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM ban_marks m JOIN bans b ON b.id = m.ban_id \
         WHERE (b.expires_at IS NULL OR b.expires_at > ?) AND ((m.kind = 'ip' AND m.value = ?) OR (m.kind = 'device' AND m.value = ?))",
    )
    .bind(now_ms())
    .bind(ip.to_string())
    .bind(device_id)
    .fetch_one(&srv.db)
    .await?;
    if n > 0 { Err(banned_error()) } else { Ok(()) }
}

/// The owner's "max accounts per IP" rule, checked when registering. The host machine is exempt.
pub async fn check_accounts_per_ip(srv: &ServerCtx, ip: IpAddr, max: i64) -> ApiResult<()> {
    let ip = crate::ipfilter::normalize(ip);
    if max <= 0 || ip.is_loopback() {
        return Ok(());
    }
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE signup_ip = ?").bind(ip.to_string()).fetch_one(&srv.db).await?;
    if n >= max {
        return Err(ApiError::forbidden("too_many_accounts", "Too many accounts were already created from your network."));
    }
    Ok(())
}

/// Blocks posting while timed out.
pub fn check_not_timed_out(user: &UserRow) -> ApiResult<()> {
    match user.timed_out_until {
        Some(until) if until > now_ms() => Err(ApiError::forbidden("timed_out", "You are timed out and can't post right now.").with("until", json!(ts(until)))),
        _ => Ok(()),
    }
}

/// The owner's "new accounts can't post links or media for X hours" rule. Members who can manage
/// messages, and the owner, are exempt.
pub async fn check_new_account(auth: &Auth, wants_media_or_links: bool) -> ApiResult<()> {
    if !wants_media_or_links || auth.is_owner || perms::has(auth.perms, MANAGE_MESSAGES) {
        return Ok(());
    }
    let hours: i64 = sqlx::query_scalar("SELECT new_account_restrict_hours FROM server LIMIT 1").fetch_one(&auth.srv.db).await?;
    let until = auth.user.created_at + hours * HOUR_MS;
    if hours > 0 && until > now_ms() {
        return Err(ApiError::forbidden("account_too_new", "New accounts can't post links, images or videos yet.").with("until", json!(ts(until))));
    }
    Ok(())
}

pub fn has_link(text: &str) -> bool {
    let t = text.to_lowercase();
    t.contains("http://") || t.contains("https://") || t.contains("www.") || t.contains("discord.gg/")
}

/// Mutes a user until `until` (used by moderators and by the automatic spam filter).
pub async fn set_timeout(srv: &ServerCtx, user_id: i64, until: Option<i64>) -> ApiResult<()> {
    sqlx::query("UPDATE users SET timed_out_until = ? WHERE id = ?").bind(until).bind(user_id).execute(&srv.db).await?;
    if let Some(m) = db::member(srv, user_id).await? {
        // refresh: open sockets re-check voice suppression too.
        srv.emit(Event::new("member_update", m).refresh());
    }
    Ok(())
}

// ---------------------------------------------------------------- rank rule

/// Moderators can only act on members ranked below them, never on the owner or themselves.
async fn check_can_moderate(auth: &Auth, target: i64) -> ApiResult<UserRow> {
    let user = db::user_by_id(&auth.srv.db, target).await?.ok_or_else(|| ApiError::not_found("not_found", "No such member."))?;
    if target == auth.user.id {
        return Err(ApiError::bad_request("You can't do that to yourself."));
    }
    let (_, target_is_owner) = db::user_perms(&auth.srv.db, target).await?;
    if target_is_owner {
        return Err(ApiError::forbidden("forbidden", "The owner can't be moderated."));
    }
    if !auth.is_owner {
        let top = |uid: i64| {
            let db = auth.srv.db.clone();
            async move {
                sqlx::query_scalar::<_, Option<i64>>("SELECT MAX(r.position) FROM roles r JOIN member_roles m ON m.role_id = r.id WHERE m.user_id = ?")
                    .bind(uid)
                    .fetch_one(&db)
                    .await
                    .map(|v| v.unwrap_or(0))
            }
        };
        if top(target).await? >= top(auth.user.id).await? {
            return Err(ApiError::forbidden("forbidden", "That member is ranked the same as or higher than you."));
        }
    }
    Ok(user)
}

fn kick_out(srv: &ServerCtx, user_id: i64, code: &'static str, message: &str) {
    srv.emit(Event::close(CloseTarget {
        token_hash: None,
        user_id: Some(user_id),
        except_token: None,
        close_code: super::ws::CLOSE_BANNED,
        error_code: code,
        message: message.to_string(),
    }));
}

fn clean_reason(r: Option<String>) -> ApiResult<String> {
    let r = r.unwrap_or_default().trim().to_string();
    if r.chars().count() > 500 {
        return Err(ApiError::validation(BTreeMap::from([("reason", "too_long")])));
    }
    Ok(r)
}

// ---------------------------------------------------------------- timeouts and kicks

#[derive(Deserialize)]
pub struct UserPath {
    user_id: String,
}

#[derive(Deserialize)]
pub struct TimeoutReq {
    minutes: i64,
    reason: Option<String>,
}

pub async fn timeout(auth: Auth, Path(p): Path<UserPath>, Body(req): Body<TimeoutReq>) -> ApiResult<StatusCode> {
    auth.require(KICK_MEMBERS)?;
    let target = check_can_moderate(&auth, parse_id(&p.user_id, "not_found")?).await?;
    if !(1..=40_320).contains(&req.minutes) {
        return Err(ApiError::validation(BTreeMap::from([("minutes", "out_of_range")])));
    }
    let reason = clean_reason(req.reason)?;
    let until = now_ms() + req.minutes * 60_000;
    set_timeout(&auth.srv, target.id, Some(until)).await?;
    audit(&auth.srv, Some(auth.user.id), "member_timeout", Some(target.id), json!({ "minutes": req.minutes, "reason": reason })).await;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn clear_timeout(auth: Auth, Path(p): Path<UserPath>) -> ApiResult<StatusCode> {
    auth.require(KICK_MEMBERS)?;
    let target = check_can_moderate(&auth, parse_id(&p.user_id, "not_found")?).await?;
    set_timeout(&auth.srv, target.id, None).await?;
    auth.srv.spam.lock().unwrap().forgive(target.id);
    audit(&auth.srv, Some(auth.user.id), "member_timeout_clear", Some(target.id), json!({})).await;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct ReasonReq {
    reason: Option<String>,
}

/// Logs the member out everywhere and removes their roles. To keep someone out, ban them.
pub async fn kick(auth: Auth, Path(p): Path<UserPath>, Body(req): Body<ReasonReq>) -> ApiResult<StatusCode> {
    auth.require(KICK_MEMBERS)?;
    let target = check_can_moderate(&auth, parse_id(&p.user_id, "not_found")?).await?;
    let reason = clean_reason(req.reason)?;
    sqlx::query("DELETE FROM sessions WHERE user_id = ?").bind(target.id).execute(&auth.srv.db).await?;
    sqlx::query("DELETE FROM member_roles WHERE user_id = ?").bind(target.id).execute(&auth.srv.db).await?;
    sqlx::query("UPDATE users SET kicked = 1 WHERE id = ?").bind(target.id).execute(&auth.srv.db).await?;
    kick_out(&auth.srv, target.id, "kicked", &if reason.is_empty() { "You were kicked.".to_string() } else { format!("You were kicked: {reason}") });
    auth.srv.emit(Event::new("member_leave", json!({ "user_id": target.id.to_string() })));
    audit(&auth.srv, Some(auth.user.id), "member_kick", Some(target.id), json!({ "reason": reason })).await;
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------- bans

#[derive(Deserialize)]
pub struct BanReq {
    user_id: String,
    reason: Option<String>,
    /// Omitted or null = permanent.
    duration_hours: Option<i64>,
    #[serde(default)]
    ban_ip: bool,
    #[serde(default)]
    ban_device: bool,
    /// Also delete their messages from the last N hours (0–168).
    #[serde(default)]
    delete_messages_hours: i64,
}

#[derive(sqlx::FromRow)]
struct BanRow {
    id: i64,
    user_id: Option<i64>,
    reason: String,
    created_by: Option<i64>,
    created_at: i64,
    expires_at: Option<i64>,
}

#[derive(Serialize)]
pub struct BanOut {
    id: String,
    user: Option<UserOut>,
    reason: String,
    ip_banned: bool,
    device_banned: bool,
    /// Only shown to the owner.
    #[serde(skip_serializing_if = "Option::is_none")]
    ips: Option<Vec<String>>,
    created_by: Option<String>,
    created_at: String,
    expires_at: Option<String>,
}

async fn ban_out(auth: &Auth, b: &BanRow) -> ApiResult<BanOut> {
    let marks: Vec<(String, String)> = sqlx::query_as("SELECT kind, value FROM ban_marks WHERE ban_id = ?").bind(b.id).fetch_all(&auth.srv.db).await?;
    let user = match b.user_id {
        Some(uid) => db::user_by_id(&auth.srv.db, uid).await?.as_ref().map(UserOut::from),
        None => None,
    };
    let ips: Vec<String> = marks.iter().filter(|(k, _)| k == "ip").map(|(_, v)| v.clone()).collect();
    Ok(BanOut {
        id: b.id.to_string(),
        user,
        reason: b.reason.clone(),
        ip_banned: !ips.is_empty(),
        device_banned: marks.iter().any(|(k, _)| k == "device"),
        ips: auth.is_owner.then_some(ips),
        created_by: b.created_by.map(|v| v.to_string()),
        created_at: ts(b.created_at),
        expires_at: b.expires_at.map(ts),
    })
}

pub async fn list_bans(auth: Auth) -> ApiResult<Json<Vec<BanOut>>> {
    auth.require(BAN_MEMBERS)?;
    let rows: Vec<BanRow> = sqlx::query_as(
        "SELECT id, user_id, reason, created_by, created_at, expires_at FROM bans \
         WHERE expires_at IS NULL OR expires_at > ? ORDER BY id DESC",
    )
    .bind(now_ms())
    .fetch_all(&auth.srv.db)
    .await?;
    let mut out = Vec::new();
    for b in &rows {
        out.push(ban_out(&auth, b).await?);
    }
    Ok(Json(out))
}

pub async fn ban(auth: Auth, Body(req): Body<BanReq>) -> ApiResult<(StatusCode, Json<BanOut>)> {
    auth.require(BAN_MEMBERS)?;
    let target = check_can_moderate(&auth, parse_id(&req.user_id, "not_found")?).await?;
    let mut fields = BTreeMap::new();
    if req.duration_hours.is_some_and(|h| !(1..=87_600).contains(&h)) {
        fields.insert("duration_hours", "out_of_range");
    }
    if !(0..=168).contains(&req.delete_messages_hours) {
        fields.insert("delete_messages_hours", "out_of_range");
    }
    if !fields.is_empty() {
        return Err(ApiError::validation(fields));
    }
    let reason = clean_reason(req.reason)?;
    let now = now_ms();
    let row =
        BanRow { id: next_id(), user_id: Some(target.id), reason, created_by: Some(auth.user.id), created_at: now, expires_at: req.duration_hours.map(|h| now + h * HOUR_MS) };

    // Collect the addresses and devices this account has used.
    let mut marks: Vec<(&str, String)> = Vec::new();
    let used: Vec<(String, String)> = sqlx::query_as("SELECT ip, device_id FROM sessions WHERE user_id = ?").bind(target.id).fetch_all(&auth.srv.db).await?;
    let ips = used.iter().map(|(ip, _)| ip.clone()).chain(target.signup_ip.clone());
    let devices = used.iter().map(|(_, d)| d.clone()).chain(target.signup_device.clone());
    if req.ban_ip {
        for ip in ips {
            // Never ban the host machine itself.
            let loopback = ip.parse::<IpAddr>().map(|a| crate::ipfilter::normalize(a).is_loopback()).unwrap_or(true);
            if !loopback && !marks.iter().any(|(k, v)| *k == "ip" && *v == ip) {
                marks.push(("ip", ip));
            }
        }
    }
    if req.ban_device {
        for d in devices {
            if d != "unknown" && !d.is_empty() && !marks.iter().any(|(k, v)| *k == "device" && *v == d) {
                marks.push(("device", d));
            }
        }
    }

    let mut tx = auth.srv.db.begin().await?;
    sqlx::query("INSERT INTO bans (id, user_id, reason, created_by, created_at, expires_at) VALUES (?, ?, ?, ?, ?, ?)")
        .bind(row.id)
        .bind(row.user_id)
        .bind(&row.reason)
        .bind(row.created_by)
        .bind(row.created_at)
        .bind(row.expires_at)
        .execute(&mut *tx)
        .await?;
    for (kind, value) in &marks {
        sqlx::query("INSERT INTO ban_marks (ban_id, kind, value) VALUES (?, ?, ?)").bind(row.id).bind(kind).bind(value).execute(&mut *tx).await?;
    }
    sqlx::query("DELETE FROM sessions WHERE user_id = ?").bind(target.id).execute(&mut *tx).await?;
    let mut deleted: Vec<(i64, i64)> = Vec::new();
    if req.delete_messages_hours > 0 {
        deleted = sqlx::query_as("SELECT id, channel_id FROM messages WHERE author_id = ? AND created_at >= ?")
            .bind(target.id)
            .bind(now - req.delete_messages_hours * HOUR_MS)
            .fetch_all(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM messages WHERE author_id = ? AND created_at >= ?").bind(target.id).bind(now - req.delete_messages_hours * HOUR_MS).execute(&mut *tx).await?;
    }
    tx.commit().await?;

    kick_out(&auth.srv, target.id, "banned", &if row.reason.is_empty() { "You were banned.".to_string() } else { format!("You were banned: {}", row.reason) });
    for (mid, cid) in deleted {
        super::messages::emit_in(&auth.srv, Event::new("message_delete", json!({ "id": mid.to_string(), "channel_id": cid.to_string() })), cid).await?;
    }
    auth.srv.emit(Event::new("member_leave", json!({ "user_id": target.id.to_string() })));
    if req.ban_ip {
        auth.srv.reload_ip_rules().await?;
    }
    audit(
        &auth.srv,
        Some(auth.user.id),
        "member_ban",
        Some(target.id),
        json!({ "reason": row.reason, "duration_hours": req.duration_hours, "ban_ip": req.ban_ip, "ban_device": req.ban_device, "delete_messages_hours": req.delete_messages_hours }),
    )
    .await;
    Ok((StatusCode::CREATED, Json(ban_out(&auth, &row).await?)))
}

#[derive(Deserialize)]
pub struct IdPath {
    id: String,
}

pub async fn unban(auth: Auth, Path(p): Path<IdPath>) -> ApiResult<StatusCode> {
    auth.require(BAN_MEMBERS)?;
    let id = parse_id(&p.id, "not_found")?;
    let user: Option<Option<i64>> = sqlx::query_scalar("SELECT user_id FROM bans WHERE id = ?").bind(id).fetch_optional(&auth.srv.db).await?;
    let Some(user) = user else { return Err(ApiError::not_found("not_found", "No such ban.")) };
    sqlx::query("DELETE FROM bans WHERE id = ?").bind(id).execute(&auth.srv.db).await?;
    auth.srv.reload_ip_rules().await?;
    if let Some(uid) = user
        && let Some(m) = db::member(&auth.srv, uid).await?
    {
        auth.srv.emit(Event::new("member_join", m));
    }
    audit(&auth.srv, Some(auth.user.id), "member_unban", user, json!({ "ban_id": id.to_string() })).await;
    Ok(StatusCode::NO_CONTENT)
}
