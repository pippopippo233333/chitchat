//! Per-server storage: what counts toward `server_total_mb` (stored files + message text) and
//! what happens when it's full (`storage_full_action`: delete the oldest messages, or refuse).
//! The AI review queue is reported separately and doesn't count.

use std::time::{Duration, Instant};

use axum::Json;
use axum::http::StatusCode;
use serde_json::{Value, json};

use super::{ApiError, ApiResult, Auth};
use crate::db;
use crate::events::Event;
use crate::host::ServerCtx;

const MB: i64 = 1024 * 1024;
/// Pruning stops at this share of the limit, so it doesn't run again on the next message.
const PRUNE_TO: f64 = 0.9;
/// How long a measured total is trusted (it's kept up to date with what we add in between).
const CACHE_FOR: Duration = Duration::from_secs(30);
const PRUNE_BATCH: i64 = 100;

pub struct Usage {
    pub files: i64,
    pub messages: i64,
    pub ai: i64,
}

impl Usage {
    pub fn used(&self) -> i64 {
        self.files + self.messages
    }
}

pub async fn usage(srv: &ServerCtx) -> sqlx::Result<Usage> {
    let text = |table: &str| format!("SELECT COALESCE(SUM(LENGTH(CAST(content AS BLOB))), 0) FROM {table}");
    Ok(Usage {
        files: super::files::server_usage(srv).await?,
        messages: sqlx::query_scalar(&text("messages")).fetch_one(&srv.db).await?,
        ai: sqlx::query_scalar(&text("ai_flags")).fetch_one(&srv.db).await?,
    })
}

fn full(limit_mb: i64) -> ApiError {
    ApiError::new(StatusCode::PAYLOAD_TOO_LARGE, "server_storage_full", "This server's storage is full. Ask the owner to free space or raise the limit.")
        .with("limit_mb", json!(limit_mb))
}

/// Held while storing: pruning can't run between a file being stored and its row being written.
pub type Room<'a> = tokio::sync::MutexGuard<'a, Option<(Instant, i64)>>;

/// Called before storing `incoming` bytes (a message's text or an upload). Over the limit, either
/// deletes the oldest messages (and their files) or refuses. Keep the returned guard until the
/// new data is written.
pub async fn make_room(srv: &ServerCtx, incoming: i64) -> ApiResult<Room<'_>> {
    // Also serializes pruning: only one request deletes at a time.
    let mut cache = srv.storage_used.lock().await;
    let s = db::server_row(&srv.db).await?;
    if s.server_total_mb == 0 {
        return Ok(cache);
    }
    let limit = s.server_total_mb * MB;
    let used = match *cache {
        Some((at, used)) if at.elapsed() < CACHE_FOR => used,
        _ => usage(srv).await?.used(),
    };
    if used + incoming <= limit {
        *cache = Some((cache.map_or(Instant::now(), |(at, _)| at), used + incoming));
        return Ok(cache);
    }
    if incoming > limit || s.storage_full_action != "delete_oldest" {
        *cache = Some((Instant::now(), used));
        tracing::warn!(server = %srv.slug, "server storage is full");
        return Err(full(s.server_total_mb));
    }
    let used = prune(srv, (limit as f64 * PRUNE_TO) as i64 - incoming).await?;
    *cache = Some((Instant::now(), used));
    if used + incoming > limit {
        return Err(full(s.server_total_mb));
    }
    *cache = Some((Instant::now(), used + incoming));
    Ok(cache)
}

/// Deletes the oldest messages with their attachments until usage is at most `target`.
/// Returns the usage afterwards.
async fn prune(srv: &ServerCtx, target: i64) -> ApiResult<i64> {
    let before = usage(srv).await?.used();
    let (mut used, mut deleted) = (before, 0usize);
    'outer: while used > target {
        let batch: Vec<(i64, i64, i64)> =
            sqlx::query_as("SELECT id, channel_id, LENGTH(CAST(content AS BLOB)) FROM messages ORDER BY id LIMIT ?").bind(PRUNE_BATCH).fetch_all(&srv.db).await?;
        if batch.is_empty() {
            break;
        }
        // One message at a time, so no more is deleted than needed.
        for (id, channel_id, text_bytes) in batch {
            let files: Vec<(String, Option<String>)> = sqlx::query_as("SELECT blob_hash, thumb_hash FROM attachments WHERE message_id = ?").bind(id).fetch_all(&srv.db).await?;
            let hashes: Vec<String> = files.into_iter().flat_map(|(b, t)| std::iter::once(b).chain(t)).collect();
            sqlx::query("DELETE FROM messages WHERE id = ?").bind(id).execute(&srv.db).await?;
            super::messages::emit_in(srv, Event::new("message_delete", json!({ "id": id.to_string(), "channel_id": channel_id.to_string() })), channel_id).await?;
            deleted += 1;
            used -= text_bytes + super::files::remove_blobs_if_unused(srv, &hashes).await?;
            if used <= target {
                break 'outer;
            }
        }
    }
    let used = usage(srv).await?.used();
    if deleted > 0 {
        tracing::info!(server = %srv.slug, deleted, "storage full: deleted the oldest messages");
        super::moderation::audit(srv, None, "storage_prune", None, json!({ "messages": deleted, "bytes": before - used })).await;
    }
    Ok(used)
}

/// `GET /storage` (owner): this server's usage, limit and full action.
pub async fn get(auth: Auth) -> ApiResult<Json<Value>> {
    if !auth.is_owner {
        return Err(ApiError::forbidden("forbidden", "Only the server owner can do that."));
    }
    let s = db::server_row(&auth.srv.db).await?;
    let u = usage(&auth.srv).await?;
    Ok(Json(json!({
        "used_bytes": u.used(),
        "limit_bytes": (s.server_total_mb > 0).then_some(s.server_total_mb * MB),
        "files_bytes": u.files,
        "messages_bytes": u.messages,
        "ai_bytes": u.ai,
        "full_action": s.storage_full_action,
    })))
}
