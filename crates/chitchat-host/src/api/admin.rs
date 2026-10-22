//! Owner controls: server settings, server password, invites and IP rules.

use std::collections::BTreeMap;

use axum::Json;
use axum::extract::Path;
use axum::http::StatusCode;
use rand::Rng;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::extract::parse_id;
use super::moderation::audit;
use super::{ApiError, ApiResult, Auth, Body, Srv};
use crate::db;
use crate::events::Event;
use crate::ids::next_id;
use crate::ipfilter::Cidr;
use crate::models::{ServerOut, ServerRow, ServerSummary, now_ms, ts};
use crate::perms::{CREATE_INVITES, MANAGE_SERVER};

fn one_field(field: &'static str, err: &'static str) -> ApiError {
    ApiError::validation(BTreeMap::from([(field, err)]))
}

fn owner_only(auth: &Auth) -> ApiResult<()> {
    if auth.is_owner { Ok(()) } else { Err(ApiError::forbidden("forbidden", "Only the server owner can do that.")) }
}

// ---------------------------------------------------------------- settings

#[derive(Serialize)]
pub struct Settings {
    name: String,
    description: String,
    has_password: bool,
    invite_only: bool,
    registration_open: bool,
    listed: bool,
    discoverable: bool,
    voice_messages: bool,
    ip_mode: String,
    max_file_mb: i64,
    user_daily_mb: i64,
    user_total_mb: i64,
    server_total_mb: i64,
    storage_full_action: String,
    media_retention_days: i64,
    storage_used_bytes: i64,
    new_account_restrict_hours: i64,
    max_accounts_per_ip: i64,
    spam_messages_per_5s: i64,
    spam_chars_per_minute: i64,
    spam_max_repeats: i64,
    spam_timeout_minutes: i64,
}

impl From<&ServerRow> for Settings {
    fn from(r: &ServerRow) -> Self {
        Settings {
            name: r.name.clone(),
            description: r.description.clone(),
            has_password: r.password_hash.is_some(),
            invite_only: r.invite_only,
            registration_open: r.registration_open,
            listed: r.listed,
            discoverable: r.discoverable,
            voice_messages: r.voice_messages,
            ip_mode: r.ip_mode.clone(),
            max_file_mb: r.max_file_mb,
            user_daily_mb: r.user_daily_mb,
            user_total_mb: r.user_total_mb,
            server_total_mb: r.server_total_mb,
            storage_full_action: r.storage_full_action.clone(),
            media_retention_days: r.media_retention_days,
            storage_used_bytes: 0,
            new_account_restrict_hours: r.new_account_restrict_hours,
            max_accounts_per_ip: r.max_accounts_per_ip,
            spam_messages_per_5s: r.spam_messages_per_5s,
            spam_chars_per_minute: r.spam_chars_per_minute,
            spam_max_repeats: r.spam_max_repeats,
            spam_timeout_minutes: r.spam_timeout_minutes,
        }
    }
}

async fn settings_out(auth: &Auth, row: &ServerRow) -> ApiResult<Settings> {
    let mut s = Settings::from(row);
    s.storage_used_bytes = super::storage::usage(&auth.srv).await?.used();
    Ok(s)
}

pub async fn get_settings(auth: Auth) -> ApiResult<Json<Settings>> {
    auth.require(MANAGE_SERVER)?;
    Ok(Json(settings_out(&auth, &db::server_row(&auth.srv.db).await?).await?))
}

#[derive(Deserialize)]
pub struct SettingsReq {
    name: Option<String>,
    description: Option<String>,
    invite_only: Option<bool>,
    registration_open: Option<bool>,
    listed: Option<bool>,
    discoverable: Option<bool>,
    voice_messages: Option<bool>,
    ip_mode: Option<String>,
    max_file_mb: Option<i64>,
    user_daily_mb: Option<i64>,
    user_total_mb: Option<i64>,
    server_total_mb: Option<i64>,
    storage_full_action: Option<String>,
    media_retention_days: Option<i64>,
    new_account_restrict_hours: Option<i64>,
    max_accounts_per_ip: Option<i64>,
    spam_messages_per_5s: Option<i64>,
    spam_chars_per_minute: Option<i64>,
    spam_max_repeats: Option<i64>,
    spam_timeout_minutes: Option<i64>,
}

pub async fn patch_settings(auth: Auth, Body(req): Body<SettingsReq>) -> ApiResult<Json<Settings>> {
    auth.require(MANAGE_SERVER)?;
    let mut row = db::server_row(&auth.srv.db).await?;
    if let Some(n) = &req.name {
        let n = n.trim();
        if n.is_empty() || n.chars().count() > 64 {
            return Err(one_field("name", if n.is_empty() { "too_short" } else { "too_long" }));
        }
        row.name = n.to_string();
    }
    if let Some(d) = &req.description {
        if d.chars().count() > 500 {
            return Err(one_field("description", "too_long"));
        }
        row.description = d.trim().to_string();
    }
    if let Some(m) = &req.ip_mode {
        owner_only(&auth)?;
        if m != "off" && m != "allowlist" {
            return Err(one_field("ip_mode", "invalid"));
        }
        row.ip_mode = m.clone();
    }
    if let Some(d) = req.discoverable {
        // Advertises the server to strangers on other hosts, so only the owner decides.
        owner_only(&auth)?;
        row.discoverable = d;
    }
    if let Some(a) = &req.storage_full_action {
        // It can delete messages, so only the owner chooses it.
        owner_only(&auth)?;
        if a != "delete_oldest" && a != "refuse" {
            return Err(one_field("storage_full_action", "invalid"));
        }
        row.storage_full_action = a.clone();
    }
    let limits = [
        ("max_file_mb", req.max_file_mb, 1, 2048, &mut row.max_file_mb),
        ("user_daily_mb", req.user_daily_mb, 0, 1_000_000, &mut row.user_daily_mb),
        ("user_total_mb", req.user_total_mb, 0, 10_000_000, &mut row.user_total_mb),
        ("server_total_mb", req.server_total_mb, 0, 100_000_000, &mut row.server_total_mb),
        ("media_retention_days", req.media_retention_days, 0, 36_500, &mut row.media_retention_days),
        ("new_account_restrict_hours", req.new_account_restrict_hours, 0, 720, &mut row.new_account_restrict_hours),
        ("max_accounts_per_ip", req.max_accounts_per_ip, 0, 100, &mut row.max_accounts_per_ip),
        ("spam_messages_per_5s", req.spam_messages_per_5s, 1, 50, &mut row.spam_messages_per_5s),
        ("spam_chars_per_minute", req.spam_chars_per_minute, 500, 100_000, &mut row.spam_chars_per_minute),
        ("spam_max_repeats", req.spam_max_repeats, 1, 20, &mut row.spam_max_repeats),
        ("spam_timeout_minutes", req.spam_timeout_minutes, 0, 1440, &mut row.spam_timeout_minutes),
    ];
    for (field, value, min, max, target) in limits {
        if let Some(v) = value {
            if !(min..=max).contains(&v) {
                return Err(one_field(field, "out_of_range"));
            }
            *target = v;
        }
    }
    row.invite_only = req.invite_only.unwrap_or(row.invite_only);
    row.registration_open = req.registration_open.unwrap_or(row.registration_open);
    row.listed = req.listed.unwrap_or(row.listed);
    row.voice_messages = req.voice_messages.unwrap_or(row.voice_messages);
    sqlx::query(
        "UPDATE server SET name = ?, description = ?, invite_only = ?, registration_open = ?, listed = ?, ip_mode = ?, \
         max_file_mb = ?, user_daily_mb = ?, user_total_mb = ?, server_total_mb = ?, media_retention_days = ?, \
         new_account_restrict_hours = ?, max_accounts_per_ip = ?, spam_messages_per_5s = ?, spam_chars_per_minute = ?, \
         spam_max_repeats = ?, spam_timeout_minutes = ?, storage_full_action = ?, discoverable = ?, voice_messages = ?",
    )
    .bind(&row.name)
    .bind(&row.description)
    .bind(row.invite_only)
    .bind(row.registration_open)
    .bind(row.listed)
    .bind(&row.ip_mode)
    .bind(row.max_file_mb)
    .bind(row.user_daily_mb)
    .bind(row.user_total_mb)
    .bind(row.server_total_mb)
    .bind(row.media_retention_days)
    .bind(row.new_account_restrict_hours)
    .bind(row.max_accounts_per_ip)
    .bind(row.spam_messages_per_5s)
    .bind(row.spam_chars_per_minute)
    .bind(row.spam_max_repeats)
    .bind(row.spam_timeout_minutes)
    .bind(&row.storage_full_action)
    .bind(row.discoverable)
    .bind(row.voice_messages)
    .execute(&auth.srv.db)
    .await?;
    *auth.srv.storage_used.lock().await = None;
    if req.ip_mode.is_some() {
        auth.srv.reload_ip_rules().await?;
    }
    auth.srv.apply_spam_settings(&row);
    audit(&auth.srv, Some(auth.user.id), "settings_update", None, serde_json::to_value(Settings::from(&row)).unwrap_or_default()).await;
    auth.srv.emit(Event::new("server_update", ServerOut::new(&auth.srv.slug, &row)));
    Ok(Json(settings_out(&auth, &row).await?))
}

#[derive(Deserialize)]
pub struct PasswordReq {
    /// `null` or "" removes the password.
    password: Option<String>,
}

pub async fn set_password(auth: Auth, Body(req): Body<PasswordReq>) -> ApiResult<Json<Settings>> {
    owner_only(&auth)?;
    let hash = match req.password.as_deref().map(str::trim).filter(|p| !p.is_empty()) {
        None => None,
        Some(p) => {
            if p.chars().count() < 4 || p.chars().count() > 128 {
                return Err(one_field("password", if p.chars().count() < 4 { "too_short" } else { "too_long" }));
            }
            Some(super::auth::hash_password(p.to_string()).await?)
        }
    };
    sqlx::query("UPDATE server SET password_hash = ?").bind(&hash).execute(&auth.srv.db).await?;
    audit(&auth.srv, Some(auth.user.id), "server_password", None, json!({ "set": hash.is_some() })).await;
    let row = db::server_row(&auth.srv.db).await?;
    auth.srv.emit(Event::new("server_update", ServerOut::new(&auth.srv.slug, &row)));
    Ok(Json(settings_out(&auth, &row).await?))
}

// ---------------------------------------------------------------- invites

#[derive(sqlx::FromRow)]
struct InviteRow {
    code: String,
    creator_id: i64,
    max_uses: Option<i64>,
    uses: i64,
    expires_at: Option<i64>,
    created_at: i64,
}

#[derive(Serialize)]
pub struct InviteOut {
    code: String,
    creator_id: String,
    max_uses: Option<i64>,
    uses: i64,
    expires_at: Option<String>,
    created_at: String,
}

impl From<&InviteRow> for InviteOut {
    fn from(i: &InviteRow) -> Self {
        InviteOut { code: i.code.clone(), creator_id: i.creator_id.to_string(), max_uses: i.max_uses, uses: i.uses, expires_at: i.expires_at.map(ts), created_at: ts(i.created_at) }
    }
}

impl InviteRow {
    fn usable(&self, now: i64) -> bool {
        self.expires_at.is_none_or(|e| e > now) && self.max_uses.is_none_or(|m| self.uses < m)
    }
}

fn new_invite_code() -> String {
    const ALPHABET: &[u8] = b"abcdefghjkmnpqrstuvwxyz23456789";
    let mut rng = rand::thread_rng();
    (0..10).map(|_| ALPHABET[rng.gen_range(0..ALPHABET.len())] as char).collect()
}

#[derive(Deserialize)]
pub struct InviteReq {
    max_uses: Option<i64>,
    /// Seconds until it expires; omitted or null = never.
    expires_in: Option<i64>,
}

pub async fn create_invite(auth: Auth, Body(req): Body<InviteReq>) -> ApiResult<(StatusCode, Json<InviteOut>)> {
    auth.require(CREATE_INVITES)?;
    if req.max_uses.is_some_and(|m| !(1..=10_000).contains(&m)) {
        return Err(one_field("max_uses", "invalid"));
    }
    if req.expires_in.is_some_and(|s| !(60..=60 * 60 * 24 * 365).contains(&s)) {
        return Err(one_field("expires_in", "invalid"));
    }
    let now = now_ms();
    let row = InviteRow { code: new_invite_code(), creator_id: auth.user.id, max_uses: req.max_uses, uses: 0, expires_at: req.expires_in.map(|s| now + s * 1000), created_at: now };
    sqlx::query("INSERT INTO invites (code, creator_id, max_uses, uses, expires_at, created_at) VALUES (?, ?, ?, 0, ?, ?)")
        .bind(&row.code)
        .bind(row.creator_id)
        .bind(row.max_uses)
        .bind(row.expires_at)
        .bind(row.created_at)
        .execute(&auth.srv.db)
        .await?;
    audit(&auth.srv, Some(auth.user.id), "invite_create", None, json!({ "code": row.code, "max_uses": row.max_uses })).await;
    Ok((StatusCode::CREATED, Json(InviteOut::from(&row))))
}

pub async fn list_invites(auth: Auth) -> ApiResult<Json<Vec<InviteOut>>> {
    auth.require(CREATE_INVITES)?;
    let rows: Vec<InviteRow> = sqlx::query_as("SELECT * FROM invites ORDER BY created_at DESC").fetch_all(&auth.srv.db).await?;
    let now = now_ms();
    // Only the server manager sees everyone's invites; others see their own.
    let all = crate::perms::has(auth.perms, MANAGE_SERVER);
    Ok(Json(rows.iter().filter(|r| r.usable(now) && (all || r.creator_id == auth.user.id)).map(InviteOut::from).collect()))
}

#[derive(Deserialize)]
pub struct CodePath {
    code: String,
}

pub async fn delete_invite(auth: Auth, Path(p): Path<CodePath>) -> ApiResult<StatusCode> {
    auth.require(CREATE_INVITES)?;
    let creator: Option<i64> = sqlx::query_scalar("SELECT creator_id FROM invites WHERE code = ?").bind(&p.code).fetch_optional(&auth.srv.db).await?;
    match creator {
        None => Err(ApiError::not_found("invalid_invite", "No such invite.")),
        Some(c) if c != auth.user.id && !crate::perms::has(auth.perms, MANAGE_SERVER) => Err(ApiError::missing_permission()),
        Some(_) => {
            sqlx::query("DELETE FROM invites WHERE code = ?").bind(&p.code).execute(&auth.srv.db).await?;
            audit(&auth.srv, Some(auth.user.id), "invite_delete", None, json!({ "code": p.code })).await;
            Ok(StatusCode::NO_CONTENT)
        }
    }
}

/// Public preview for an invite link: which server it joins and whether it still works.
pub async fn preview_invite(Srv(srv): Srv, Path(p): Path<CodePath>) -> ApiResult<Json<Value>> {
    let row: Option<InviteRow> = sqlx::query_as("SELECT * FROM invites WHERE code = ?").bind(&p.code).fetch_optional(&srv.db).await?;
    let row = row.filter(|r| r.usable(now_ms())).ok_or_else(|| ApiError::not_found("invalid_invite", "This invite is invalid or expired."))?;
    let server = db::server_row(&srv.db).await?;
    Ok(Json(json!({
        "code": row.code,
        "server": ServerSummary::new(&srv.slug, &server),
        "expires_at": row.expires_at.map(ts),
        "uses_left": row.max_uses.map(|m| m - row.uses),
    })))
}

/// Uses up one invite inside the registration transaction. Errors when it can't be used.
pub async fn consume_invite(tx: &mut sqlx::SqliteConnection, code: &str) -> ApiResult<()> {
    let used = sqlx::query(
        "UPDATE invites SET uses = uses + 1 WHERE code = ? AND (expires_at IS NULL OR expires_at > ?) \
         AND (max_uses IS NULL OR uses < max_uses)",
    )
    .bind(code)
    .bind(now_ms())
    .execute(&mut *tx)
    .await?;
    if used.rows_affected() == 1 { Ok(()) } else { Err(ApiError::forbidden("invalid_invite", "This invite is invalid or expired.")) }
}

// ---------------------------------------------------------------- IP rules

#[derive(sqlx::FromRow)]
struct IpRuleRow {
    id: i64,
    cidr: String,
    action: String,
    note: String,
    created_at: i64,
}

#[derive(Serialize)]
pub struct IpRuleOut {
    id: String,
    cidr: String,
    action: String,
    note: String,
    created_at: String,
}

impl From<&IpRuleRow> for IpRuleOut {
    fn from(r: &IpRuleRow) -> Self {
        IpRuleOut { id: r.id.to_string(), cidr: r.cidr.clone(), action: r.action.clone(), note: r.note.clone(), created_at: ts(r.created_at) }
    }
}

pub async fn list_ip_rules(auth: Auth) -> ApiResult<Json<Vec<IpRuleOut>>> {
    owner_only(&auth)?;
    let rows: Vec<IpRuleRow> = sqlx::query_as("SELECT * FROM ip_rules ORDER BY id").fetch_all(&auth.srv.db).await?;
    Ok(Json(rows.iter().map(IpRuleOut::from).collect()))
}

#[derive(Deserialize)]
pub struct IpRuleReq {
    cidr: String,
    action: String,
    #[serde(default)]
    note: String,
}

pub async fn add_ip_rule(auth: Auth, Body(req): Body<IpRuleReq>) -> ApiResult<(StatusCode, Json<IpRuleOut>)> {
    owner_only(&auth)?;
    let mut fields = BTreeMap::new();
    let cidr = Cidr::parse(&req.cidr);
    if cidr.is_none() {
        fields.insert("cidr", "invalid");
    }
    if req.action != "allow" && req.action != "block" {
        fields.insert("action", "invalid");
    }
    if req.note.chars().count() > 200 {
        fields.insert("note", "too_long");
    }
    let Some(cidr) = cidr.filter(|_| fields.is_empty()) else {
        return Err(ApiError::validation(fields));
    };
    let row = IpRuleRow { id: next_id(), cidr: cidr.to_string(), action: req.action, note: req.note.trim().to_string(), created_at: now_ms() };
    sqlx::query("INSERT INTO ip_rules (id, cidr, action, note, created_at) VALUES (?, ?, ?, ?, ?)")
        .bind(row.id)
        .bind(&row.cidr)
        .bind(&row.action)
        .bind(&row.note)
        .bind(row.created_at)
        .execute(&auth.srv.db)
        .await?;
    auth.srv.reload_ip_rules().await?;
    audit(&auth.srv, Some(auth.user.id), "ip_rule_add", None, json!({ "cidr": row.cidr, "action": row.action })).await;
    Ok((StatusCode::CREATED, Json(IpRuleOut::from(&row))))
}

#[derive(Deserialize)]
pub struct IdPath {
    id: String,
}

pub async fn delete_ip_rule(auth: Auth, Path(p): Path<IdPath>) -> ApiResult<StatusCode> {
    owner_only(&auth)?;
    let id = parse_id(&p.id, "not_found")?;
    let gone = sqlx::query("DELETE FROM ip_rules WHERE id = ?").bind(id).execute(&auth.srv.db).await?;
    if gone.rows_affected() == 0 {
        return Err(ApiError::not_found("not_found", "No such rule."));
    }
    auth.srv.reload_ip_rules().await?;
    audit(&auth.srv, Some(auth.user.id), "ip_rule_delete", None, json!({ "id": id.to_string() })).await;
    Ok(StatusCode::NO_CONTENT)
}
