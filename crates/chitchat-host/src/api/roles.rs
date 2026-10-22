//! Role management (MANAGE_ROLES) with a rank rule: below the owner, you can only create, edit,
//! assign or remove roles ranked lower than your own highest role, and only grant permissions you have.

use std::collections::BTreeMap;

use axum::Json;
use axum::extract::Path;
use axum::http::StatusCode;
use serde::Deserialize;
use serde_json::json;

use super::extract::parse_id;
use super::moderation::audit;
use super::{ApiError, ApiResult, Auth, Body};
use crate::db;
use crate::events::Event;
use crate::ids::next_id;
use crate::models::{RoleOut, RoleRow};
use crate::perms::{self, MANAGE_ROLES};

fn one_field(field: &'static str, err: &'static str) -> ApiError {
    ApiError::validation(BTreeMap::from([(field, err)]))
}

fn outranked() -> ApiError {
    ApiError::forbidden("forbidden", "That role is ranked the same as or higher than your highest role.")
}

/// The caller's highest role position (the owner ranks above everything).
async fn top_position(auth: &Auth) -> ApiResult<i64> {
    if auth.is_owner {
        return Ok(i64::MAX);
    }
    let top: Option<i64> =
        sqlx::query_scalar("SELECT MAX(r.position) FROM roles r JOIN member_roles m ON m.role_id = r.id WHERE m.user_id = ?").bind(auth.user.id).fetch_one(&auth.srv.db).await?;
    Ok(top.unwrap_or(0))
}

async fn role(auth: &Auth, id: &str) -> ApiResult<RoleRow> {
    let id = parse_id(id, "not_found")?;
    sqlx::query_as("SELECT * FROM roles WHERE id = ?").bind(id).fetch_optional(&auth.srv.db).await?.ok_or_else(|| ApiError::not_found("not_found", "No such role."))
}

fn parse_perms(auth: &Auth, s: &str) -> ApiResult<u64> {
    let p: u64 = s.trim().parse().map_err(|_| one_field("permissions", "invalid"))?;
    if p & !perms::ALL != 0 {
        return Err(one_field("permissions", "invalid"));
    }
    // You can't hand out permissions you don't have yourself.
    if !auth.is_owner && p & !auth.perms != 0 {
        return Err(ApiError::forbidden("forbidden", "You can't grant permissions you don't have."));
    }
    Ok(p)
}

fn check_color(c: &str) -> bool {
    c.len() == 7 && c.starts_with('#') && c[1..].chars().all(|ch| ch.is_ascii_hexdigit())
}

#[derive(Deserialize)]
pub struct RoleReq {
    name: Option<String>,
    /// "#rrggbb"; null clears it.
    #[serde(default, deserialize_with = "present")]
    color: Option<Option<String>>,
    permissions: Option<String>,
    position: Option<i64>,
}

fn present<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Option<String>>, D::Error> {
    Ok(Some(Option::<String>::deserialize(d)?))
}

fn check_role_name(n: &str) -> ApiResult<String> {
    let n = n.trim();
    match n.chars().count() {
        0 => Err(one_field("name", "too_short")),
        c if c > 32 => Err(one_field("name", "too_long")),
        _ if n.starts_with('@') => Err(one_field("name", "invalid_characters")),
        _ => Ok(n.to_string()),
    }
}

/// Moves `id` to `position` and keeps every non-default role at a distinct position >= 1.
async fn reorder(auth: &Auth, id: i64, position: i64) -> ApiResult<()> {
    let mut ids: Vec<i64> = sqlx::query_scalar("SELECT id FROM roles WHERE is_default = 0 AND id != ? ORDER BY position, id").bind(id).fetch_all(&auth.srv.db).await?;
    let at = (position.max(1) - 1).min(ids.len() as i64) as usize;
    ids.insert(at, id);
    let mut tx = auth.srv.db.begin().await?;
    for (i, rid) in ids.iter().enumerate() {
        sqlx::query("UPDATE roles SET position = ? WHERE id = ?").bind(i as i64 + 1).bind(rid).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(())
}

async fn emit_all_roles(auth: &Auth) -> ApiResult<()> {
    for r in db::list_roles(&auth.srv.db).await? {
        auth.srv.emit(Event::new("role_update", r).refresh());
    }
    Ok(())
}

pub async fn create(auth: Auth, Body(req): Body<RoleReq>) -> ApiResult<(StatusCode, Json<RoleOut>)> {
    auth.require(MANAGE_ROLES)?;
    let name = check_role_name(req.name.as_deref().unwrap_or(""))?;
    let color = match req.color.flatten() {
        Some(c) if !check_color(&c) => return Err(one_field("color", "invalid")),
        c => c,
    };
    let permissions = match &req.permissions {
        Some(p) => parse_perms(&auth, p)?,
        None => 0,
    };
    // New roles start at the bottom (just above @everyone), or where asked if that's under the caller.
    // Inserting at a position pushes the roles from there upward, so `pos == top` is still below the caller.
    let top = top_position(&auth).await?;
    let pos = req.position.unwrap_or(1);
    if pos > top {
        return Err(outranked());
    }
    let id = next_id();
    sqlx::query("INSERT INTO roles (id, name, color, position, permissions, is_default) VALUES (?, ?, ?, 0, ?, 0)")
        .bind(id)
        .bind(&name)
        .bind(&color)
        .bind(permissions as i64)
        .execute(&auth.srv.db)
        .await?;
    reorder(&auth, id, pos).await?;
    let row = role(&auth, &id.to_string()).await?;
    let out = RoleOut::from(&row);
    audit(&auth.srv, Some(auth.user.id), "role_create", None, json!({ "role_id": out.id, "name": out.name, "permissions": out.permissions })).await;
    auth.srv.emit(Event::new("role_create", &out).refresh());
    emit_all_roles(&auth).await?;
    Ok((StatusCode::CREATED, Json(out)))
}

pub async fn update(auth: Auth, Path(p): Path<IdPath>, Body(req): Body<RoleReq>) -> ApiResult<Json<RoleOut>> {
    auth.require(MANAGE_ROLES)?;
    let mut row = role(&auth, &p.id).await?;
    let top = top_position(&auth).await?;
    if !row.is_default && row.position >= top {
        return Err(outranked());
    }
    if row.is_default && (req.name.is_some() || req.position.is_some()) {
        return Err(ApiError::bad_request("@everyone can't be renamed or moved."));
    }
    if let Some(n) = &req.name {
        row.name = check_role_name(n)?;
    }
    if let Some(c) = req.color {
        if c.as_deref().is_some_and(|c| !check_color(c)) {
            return Err(one_field("color", "invalid"));
        }
        row.color = c;
    }
    if let Some(ps) = &req.permissions {
        row.permissions = parse_perms(&auth, ps)? as i64;
    }
    sqlx::query("UPDATE roles SET name = ?, color = ?, permissions = ? WHERE id = ?")
        .bind(&row.name)
        .bind(&row.color)
        .bind(row.permissions)
        .bind(row.id)
        .execute(&auth.srv.db)
        .await?;
    if let Some(pos) = req.position {
        if pos >= top {
            return Err(outranked());
        }
        reorder(&auth, row.id, pos).await?;
        emit_all_roles(&auth).await?;
    }
    let row = role(&auth, &p.id).await?;
    let out = RoleOut::from(&row);
    audit(&auth.srv, Some(auth.user.id), "role_update", None, json!({ "role_id": out.id, "name": out.name, "permissions": out.permissions, "position": out.position })).await;
    auth.srv.emit(Event::new("role_update", &out).refresh());
    Ok(Json(out))
}

#[derive(Deserialize)]
pub struct IdPath {
    id: String,
}

pub async fn delete(auth: Auth, Path(p): Path<IdPath>) -> ApiResult<StatusCode> {
    auth.require(MANAGE_ROLES)?;
    let row = role(&auth, &p.id).await?;
    if row.is_default {
        return Err(ApiError::bad_request("@everyone can't be deleted."));
    }
    if row.position >= top_position(&auth).await? {
        return Err(outranked());
    }
    let holders: Vec<i64> = sqlx::query_scalar("SELECT user_id FROM member_roles WHERE role_id = ?").bind(row.id).fetch_all(&auth.srv.db).await?;
    sqlx::query("DELETE FROM roles WHERE id = ?").bind(row.id).execute(&auth.srv.db).await?;
    audit(&auth.srv, Some(auth.user.id), "role_delete", None, json!({ "role_id": row.id.to_string(), "name": row.name })).await;
    auth.srv.emit(Event::new("role_delete", json!({ "id": row.id.to_string() })).refresh());
    for uid in holders {
        if let Some(m) = db::member(&auth.srv, uid).await? {
            auth.srv.emit(Event::new("member_update", m));
        }
    }
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct MemberRolePath {
    user_id: String,
    role_id: String,
}

async fn change_member_role(auth: Auth, p: MemberRolePath, add: bool) -> ApiResult<StatusCode> {
    auth.require(MANAGE_ROLES)?;
    let uid = parse_id(&p.user_id, "not_found")?;
    let row = role(&auth, &p.role_id).await?;
    if row.is_default {
        return Err(ApiError::bad_request("Everyone has @everyone."));
    }
    if row.position >= top_position(&auth).await? {
        return Err(outranked());
    }
    if db::user_by_id(&auth.srv.db, uid).await?.is_none() {
        return Err(ApiError::not_found("not_found", "No such member."));
    }
    let q = if add { "INSERT OR IGNORE INTO member_roles (user_id, role_id) VALUES (?, ?)" } else { "DELETE FROM member_roles WHERE user_id = ? AND role_id = ?" };
    let changed = sqlx::query(q).bind(uid).bind(row.id).execute(&auth.srv.db).await?;
    if changed.rows_affected() > 0 {
        let action = if add { "member_role_add" } else { "member_role_remove" };
        audit(&auth.srv, Some(auth.user.id), action, Some(uid), json!({ "role_id": row.id.to_string(), "name": row.name })).await;
        if let Some(m) = db::member(&auth.srv, uid).await? {
            auth.srv.emit(Event::new("member_update", m).refresh());
        }
    }
    Ok(StatusCode::NO_CONTENT)
}

pub async fn add_member_role(auth: Auth, Path(p): Path<MemberRolePath>) -> ApiResult<StatusCode> {
    change_member_role(auth, p, true).await
}

pub async fn remove_member_role(auth: Auth, Path(p): Path<MemberRolePath>) -> ApiResult<StatusCode> {
    change_member_role(auth, p, false).await
}
