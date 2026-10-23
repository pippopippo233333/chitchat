use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use serde::Deserialize;
use serde_json::{Value, json};

use super::extract::parse_id;
use super::{ApiError, ApiResult, Auth, Srv};
use crate::db;
use crate::host::Host;
use crate::models::{CategoryOut, ChannelOut, MemberOut, RoleOut, ServerOut, ServerSummary};

pub async fn host_info(State(host): State<Arc<Host>>) -> ApiResult<Json<Value>> {
    let servers: Vec<_> = host.servers.read().await.values().cloned().collect();
    let mut listed = Vec::new();
    for s in servers {
        let row = db::server_row(&s.db).await?;
        if row.listed {
            listed.push(ServerSummary::new(&s.slug, &row));
        }
    }
    listed.sort_by_key(|a| a.name.to_lowercase());
    Ok(Json(json!({ "name": host.name, "version": env!("CARGO_PKG_VERSION"), "servers": listed })))
}

pub async fn public(Srv(srv): Srv) -> ApiResult<Json<ServerSummary>> {
    Ok(Json(ServerSummary::new(&srv.slug, &db::server_row(&srv.db).await?)))
}

pub async fn get(auth: Auth) -> ApiResult<Json<ServerOut>> {
    Ok(Json(ServerOut::new(&auth.srv.slug, &db::server_row(&auth.srv.db).await?)))
}

pub async fn categories(auth: Auth) -> ApiResult<Json<Vec<CategoryOut>>> {
    Ok(Json(db::list_categories(&auth.srv.db).await?))
}

pub async fn channels(auth: Auth) -> ApiResult<Json<Vec<ChannelOut>>> {
    Ok(Json(db::list_channels(&auth.srv.db, auth.user.id, auth.perms).await?))
}

pub async fn members(auth: Auth) -> ApiResult<Json<Vec<MemberOut>>> {
    Ok(Json(db::list_members(&auth.srv).await?))
}

#[derive(Deserialize)]
pub struct UserPath {
    user_id: String,
}

pub async fn member(auth: Auth, Path(p): Path<UserPath>) -> ApiResult<Json<MemberOut>> {
    let id = parse_id(&p.user_id, "not_found")?;
    db::member(&auth.srv, id).await?.map(Json).ok_or_else(|| ApiError::not_found("not_found", "No such member."))
}

pub async fn roles(auth: Auth) -> ApiResult<Json<Vec<RoleOut>>> {
    Ok(Json(db::list_roles(&auth.srv.db).await?))
}

/// The snapshot sent in the WebSocket `ready` event.
pub async fn ready_snapshot(auth: &Auth) -> ApiResult<Value> {
    let srv = &auth.srv;
    Ok(json!({
        "user": crate::models::UserOut::from(&auth.user),
        "is_owner": auth.is_owner,
        "permissions": auth.perms.to_string(),
        "server": ServerOut::new(&srv.slug, &db::server_row(&srv.db).await?),
        "categories": db::list_categories(&srv.db).await?,
        "channels": db::list_channels(&srv.db, auth.user.id, auth.perms).await?,
        "members": db::list_members(srv).await?,
        "roles": db::list_roles(&srv.db).await?,
        "voice_states": srv.voice_states(auth.user.id),
        "ai_moderation": crate::ai::notice(&srv.ai.config()),
        "emojis": super::emoji::list_for(srv).await?,
    }))
}
