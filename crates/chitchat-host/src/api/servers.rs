//! Host admin: list and create servers on this host from the web, without the CLI.
//! A new server is owned by the host admin straight away, with the same username and password.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::Json;
use axum::extract::{ConnectInfo, State};
use axum::http::StatusCode;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{ApiError, ApiResult, Auth, Body};
use crate::db;
use crate::host::{Host, valid_slug};
use crate::ids::next_id;
use crate::models::{ServerSummary, UserOut};

const MAX_SERVERS: usize = 50;

/// `GET /host/servers` (host admin): every server on this host, listed or not.
pub async fn list(State(host): State<Arc<Host>>, auth: Auth) -> ApiResult<Json<Vec<Value>>> {
    super::resources::require_host_admin(&auth)?;
    let servers: Vec<_> = host.servers.read().await.values().cloned().collect();
    let mut out = Vec::new();
    for s in servers {
        let row = db::server_row(&s.db).await?;
        let members: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users").fetch_one(&s.db).await?;
        let mut v = serde_json::to_value(ServerSummary::new(&s.slug, &row)).map_err(|_| ApiError::internal())?;
        v["listed"] = json!(row.listed);
        v["has_owner"] = json!(row.owner_id.is_some());
        v["member_count"] = json!(members);
        v["created_at"] = json!(row.created_at);
        out.push(v);
    }
    out.sort_by_key(|v| v["created_at"].as_i64());
    Ok(Json(out))
}

#[derive(Deserialize)]
pub struct CreateReq {
    slug: String,
    name: Option<String>,
    #[serde(default)]
    device_id: String,
}

/// `POST /host/servers` (host admin): creates a server owned by the caller's account (copied
/// over with the same password) and returns a session for it.
pub async fn create(State(host): State<Arc<Host>>, ConnectInfo(addr): ConnectInfo<SocketAddr>, auth: Auth, Body(req): Body<CreateReq>) -> ApiResult<(StatusCode, Json<Value>)> {
    super::resources::require_host_admin(&auth)?;
    let slug = req.slug.trim().to_lowercase();
    let name = req.name.as_deref().map(str::trim).filter(|n| !n.is_empty()).unwrap_or(&slug).to_string();
    let mut fields = BTreeMap::new();
    if !valid_slug(&slug) {
        fields.insert("slug", "invalid");
    }
    if name.chars().count() > 100 {
        fields.insert("name", "too_long");
    }
    if !fields.is_empty() {
        return Err(ApiError::validation(fields));
    }
    if host.get(&slug).await.is_some() {
        return Err(ApiError::new(StatusCode::CONFLICT, "slug_taken", "A server with that address already exists."));
    }
    if host.servers.read().await.len() >= MAX_SERVERS {
        return Err(ApiError::forbidden("too_many_servers", format!("This host already has {MAX_SERVERS} servers.")));
    }
    // db::create_server's future isn't Send (sqlx transaction lifetimes), so run it on its own thread.
    let (h, s, n) = (host.clone(), slug.clone(), name.clone());
    tokio::task::spawn_blocking(move || tokio::runtime::Handle::current().block_on(h.create_server(&s, &n))).await.map_err(|_| ApiError::internal())??;
    let srv = host.get(&slug).await.ok_or_else(ApiError::internal)?;
    super::spawn_cleanup(srv.clone());

    let me = &auth.user;
    let id = next_id();
    let now = crate::models::now_ms();
    sqlx::query("INSERT INTO users (id, username, display_name, password_hash, created_at) VALUES (?, ?, ?, ?, ?)")
        .bind(id)
        .bind(&me.username)
        .bind(&me.display_name)
        .bind(&me.password_hash)
        .bind(now)
        .execute(&srv.db)
        .await?;
    sqlx::query("UPDATE server SET owner_id = ?, setup_code = NULL").bind(id).execute(&srv.db).await?;

    let user: crate::models::UserRow = sqlx::query_as("SELECT * FROM users WHERE id = ?").bind(id).fetch_one(&srv.db).await?;
    let token = super::auth::create_session(&srv, id, &super::auth::clean_device_id(&req.device_id), addr).await?;
    let row = db::server_row(&srv.db).await?;
    tracing::info!(server = %slug, owner = %me.username, "server created from the web");
    super::moderation::audit(&auth.srv, Some(me.id), "server_create", None, json!({ "slug": slug, "name": name })).await;
    Ok((StatusCode::CREATED, Json(json!({ "server": ServerSummary::new(&slug, &row), "token": token, "user": UserOut::from(&user) }))))
}
