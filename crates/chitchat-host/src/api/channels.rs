//! Creating, changing and deleting categories and channels (MANAGE_CHANNELS).

use std::collections::BTreeMap;

use axum::Json;
use axum::extract::Path;
use axum::http::StatusCode;
use serde::Deserialize;
use serde_json::{Value, json};

use super::extract::parse_id;
use super::{ApiError, ApiResult, Auth, Body};
use crate::db;
use crate::events::Event;
use crate::ids::next_id;
use crate::models::{CategoryOut, CategoryRow, ChannelOut, ChannelRow};
use crate::perms::MANAGE_CHANNELS;

#[derive(Deserialize)]
pub struct IdPath {
    id: String,
}

fn check_name(raw: &str, max: usize) -> Result<String, &'static str> {
    let n = raw.trim();
    match n.chars().count() {
        0 => Err("too_short"),
        c if c > max => Err("too_long"),
        _ if n.chars().any(char::is_control) => Err("invalid_characters"),
        _ => Ok(n.to_string()),
    }
}

fn one_field(field: &'static str, err: &'static str) -> ApiError {
    ApiError::validation(BTreeMap::from([(field, err)]))
}

async fn category_exists(auth: &Auth, id: i64) -> ApiResult<bool> {
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM categories WHERE id = ?").bind(id).fetch_one(&auth.srv.db).await?;
    Ok(n > 0)
}

/// `category_id` in a PATCH: absent = keep, `null` = remove from category, string = move.
fn parse_category(v: &Value) -> Result<Option<i64>, ApiError> {
    match v {
        Value::Null => Ok(None),
        Value::String(s) => crate::ids::parse(s).map(Some).ok_or_else(|| one_field("category_id", "unknown_category")),
        _ => Err(one_field("category_id", "invalid")),
    }
}

// ---------------------------------------------------------------- categories

#[derive(Deserialize)]
pub struct CategoryReq {
    name: Option<String>,
    position: Option<i64>,
}

pub async fn create_category(auth: Auth, Body(req): Body<CategoryReq>) -> ApiResult<(StatusCode, Json<CategoryOut>)> {
    auth.require(MANAGE_CHANNELS)?;
    let name = check_name(req.name.as_deref().unwrap_or(""), 100).map_err(|e| one_field("name", e))?;
    let position = match req.position {
        Some(p) => p,
        None => sqlx::query_scalar("SELECT COALESCE(MAX(position) + 1, 0) FROM categories").fetch_one(&auth.srv.db).await?,
    };
    let row = CategoryRow { id: next_id(), name, position };
    sqlx::query("INSERT INTO categories (id, name, position) VALUES (?, ?, ?)").bind(row.id).bind(&row.name).bind(row.position).execute(&auth.srv.db).await?;
    let out = CategoryOut::from(&row);
    auth.srv.emit(Event::new("category_create", &out));
    Ok((StatusCode::CREATED, Json(out)))
}

pub async fn patch_category(auth: Auth, Path(p): Path<IdPath>, Body(req): Body<CategoryReq>) -> ApiResult<Json<CategoryOut>> {
    auth.require(MANAGE_CHANNELS)?;
    let id = parse_id(&p.id, "not_found")?;
    let mut row: CategoryRow = sqlx::query_as("SELECT * FROM categories WHERE id = ?")
        .bind(id)
        .fetch_optional(&auth.srv.db)
        .await?
        .ok_or_else(|| ApiError::not_found("not_found", "No such category."))?;
    if let Some(n) = &req.name {
        row.name = check_name(n, 100).map_err(|e| one_field("name", e))?;
    }
    if let Some(pos) = req.position {
        row.position = pos;
    }
    sqlx::query("UPDATE categories SET name = ?, position = ? WHERE id = ?").bind(&row.name).bind(row.position).bind(row.id).execute(&auth.srv.db).await?;
    let out = CategoryOut::from(&row);
    auth.srv.emit(Event::new("category_update", &out));
    Ok(Json(out))
}

pub async fn delete_category(auth: Auth, Path(p): Path<IdPath>) -> ApiResult<StatusCode> {
    auth.require(MANAGE_CHANNELS)?;
    let id = parse_id(&p.id, "not_found")?;
    let moved: Vec<ChannelRow> = sqlx::query_as("SELECT * FROM channels WHERE category_id = ?").bind(id).fetch_all(&auth.srv.db).await?;
    let deleted = sqlx::query("DELETE FROM categories WHERE id = ?").bind(id).execute(&auth.srv.db).await?;
    if deleted.rows_affected() == 0 {
        return Err(ApiError::not_found("not_found", "No such category."));
    }
    // Its channels stay, without a category (ON DELETE SET NULL).
    for mut c in moved {
        c.category_id = None;
        auth.srv.emit(Event::channel("channel_update", ChannelOut::new(&c, 0), c.id));
    }
    auth.srv.emit(Event::new("category_delete", json!({ "id": id.to_string() })));
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------- channels

#[derive(Deserialize)]
pub struct CreateChannelReq {
    name: String,
    #[serde(rename = "type", default = "default_kind")]
    kind: String,
    category_id: Option<String>,
    #[serde(default)]
    topic: String,
    position: Option<i64>,
    user_limit: Option<i64>,
}

/// Hard ceiling for a voice room; the client warns well before this (docs/api.md 4e).
pub const MAX_USER_LIMIT: i64 = 99;
pub const DEFAULT_USER_LIMIT: i64 = 10;

fn check_user_limit(v: i64) -> Result<i64, &'static str> {
    if (1..=MAX_USER_LIMIT).contains(&v) { Ok(v) } else { Err("out_of_range") }
}

fn default_kind() -> String {
    "text".into()
}

pub async fn create_channel(auth: Auth, Body(req): Body<CreateChannelReq>) -> ApiResult<(StatusCode, Json<ChannelOut>)> {
    auth.require(MANAGE_CHANNELS)?;
    let mut fields = BTreeMap::new();
    let name = check_name(&req.name, 100).map_err(|e| fields.insert("name", e)).ok();
    if req.kind != "text" && req.kind != "voice" {
        fields.insert("type", "invalid");
    }
    if req.topic.chars().count() > 1024 {
        fields.insert("topic", "too_long");
    }
    let user_limit = check_user_limit(req.user_limit.unwrap_or(DEFAULT_USER_LIMIT)).map_err(|e| fields.insert("user_limit", e)).unwrap_or(DEFAULT_USER_LIMIT);
    let category_id = match req.category_id.as_deref() {
        None | Some("") => None,
        Some(s) => match crate::ids::parse(s) {
            Some(id) if category_exists(&auth, id).await? => Some(id),
            _ => {
                fields.insert("category_id", "unknown_category");
                None
            }
        },
    };
    let Some(name) = name.filter(|_| fields.is_empty()) else {
        return Err(ApiError::validation(fields));
    };
    let position = match req.position {
        Some(p) => p,
        None => sqlx::query_scalar("SELECT COALESCE(MAX(position) + 1, 0) FROM channels WHERE category_id IS ?").bind(category_id).fetch_one(&auth.srv.db).await?,
    };
    let row = ChannelRow { id: next_id(), kind: req.kind, name, topic: req.topic.trim().to_string(), category_id, position, media_allowed: false, user_limit };
    sqlx::query("INSERT INTO channels (id, type, name, topic, category_id, position, media_allowed, user_limit) VALUES (?, ?, ?, ?, ?, ?, 0, ?)")
        .bind(row.id)
        .bind(&row.kind)
        .bind(&row.name)
        .bind(&row.topic)
        .bind(row.category_id)
        .bind(row.position)
        .bind(row.user_limit)
        .execute(&auth.srv.db)
        .await?;
    auth.srv.emit(Event::channel("channel_create", ChannelOut::new(&row, 0), row.id));
    Ok((StatusCode::CREATED, Json(ChannelOut::new(&row, auth.perms))))
}

#[derive(Deserialize)]
pub struct PatchChannelReq {
    name: Option<String>,
    topic: Option<String>,
    /// Absent = keep; null = no category.
    #[serde(default, deserialize_with = "present")]
    category_id: Option<Value>,
    position: Option<i64>,
    media_allowed: Option<bool>,
    user_limit: Option<i64>,
}

/// Lets `"category_id": null` deserialize as `Some(Value::Null)` instead of `None`.
fn present<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Value>, D::Error> {
    Ok(Some(Value::deserialize(d)?))
}

pub async fn patch_channel(auth: Auth, Path(p): Path<IdPath>, Body(req): Body<PatchChannelReq>) -> ApiResult<Json<ChannelOut>> {
    auth.require(MANAGE_CHANNELS)?;
    let id = parse_id(&p.id, "unknown_channel")?;
    let mut row = db::channel_row(&auth.srv.db, id).await?.ok_or_else(|| ApiError::not_found("unknown_channel", "No such channel."))?;
    if let Some(n) = &req.name {
        row.name = check_name(n, 100).map_err(|e| one_field("name", e))?;
    }
    if let Some(t) = &req.topic {
        if t.chars().count() > 1024 {
            return Err(one_field("topic", "too_long"));
        }
        row.topic = t.trim().to_string();
    }
    if let Some(v) = &req.category_id {
        let cat = parse_category(v)?;
        if let Some(cid) = cat
            && !category_exists(&auth, cid).await?
        {
            return Err(one_field("category_id", "unknown_category"));
        }
        row.category_id = cat;
    }
    if let Some(pos) = req.position {
        row.position = pos;
    }
    if let Some(m) = req.media_allowed {
        row.media_allowed = m;
    }
    if let Some(l) = req.user_limit {
        row.user_limit = check_user_limit(l).map_err(|e| one_field("user_limit", e))?;
    }
    sqlx::query("UPDATE channels SET name = ?, topic = ?, category_id = ?, position = ?, media_allowed = ?, user_limit = ? WHERE id = ?")
        .bind(&row.name)
        .bind(&row.topic)
        .bind(row.category_id)
        .bind(row.position)
        .bind(row.media_allowed)
        .bind(row.user_limit)
        .bind(row.id)
        .execute(&auth.srv.db)
        .await?;
    auth.srv.emit(Event::channel("channel_update", ChannelOut::new(&row, 0), row.id));
    Ok(Json(ChannelOut::new(&row, auth.perms)))
}

pub async fn delete_channel(auth: Auth, Path(p): Path<IdPath>) -> ApiResult<StatusCode> {
    auth.require(MANAGE_CHANNELS)?;
    let id = parse_id(&p.id, "unknown_channel")?;
    let deleted = sqlx::query("DELETE FROM channels WHERE id = ?").bind(id).execute(&auth.srv.db).await?;
    if deleted.rows_affected() == 0 {
        return Err(ApiError::not_found("unknown_channel", "No such channel."));
    }
    auth.srv.emit(Event::new("channel_delete", json!({ "id": id.to_string() })).in_channel(id));
    // Anyone in that voice room drops out (their sockets clear their voice state).
    if let Some(sfu) = crate::voice::sfu() {
        for s in auth.srv.voice.lock().unwrap().values().filter(|s| s.channel_id == id) {
            sfu.leave(s.session, Some("channel_deleted"));
        }
    }
    Ok(StatusCode::NO_CONTENT)
}
