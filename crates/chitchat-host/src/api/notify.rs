//! Unread messages, mentions and each member's notification preferences.
//!
//! A read state is the newest message a member has seen in a channel. Channels never opened
//! count from when the member joined or the channel was made (IDs are time-sortable), so a new
//! member doesn't start with the whole history unread.

use std::collections::{BTreeMap, HashMap};

use axum::Json;
use axum::extract::Path;
use axum::http::StatusCode;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{ApiError, ApiResult, Auth, Body};
use crate::events::Event;
use crate::host::ServerCtx;
use crate::perms::{self, VIEW_CHANNEL};

const LEVELS: [&str; 3] = ["all", "mentions", "none"];
const MAX_MENTION_COUNT: i64 = 100;
const MAX_CHANNEL_SETTINGS: usize = 500;

/// Moves a member's read position forward (never back).
pub async fn mark_read(srv: &ServerCtx, user_id: i64, channel_id: i64, message_id: i64) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO read_states (user_id, channel_id, last_read_id) VALUES (?, ?, ?) \
         ON CONFLICT (user_id, channel_id) DO UPDATE SET last_read_id = MAX(last_read_id, excluded.last_read_id)",
    )
    .bind(user_id)
    .bind(channel_id)
    .bind(message_id)
    .execute(&srv.db)
    .await?;
    Ok(())
}

/// Messages after `after` that ping the member: their name, one of their roles, @everyone, or
/// anything the other person wrote in a direct message. Not their own.
async fn mention_count(auth: &Auth, channel_id: i64, is_dm: bool, after: i64) -> sqlx::Result<i64> {
    sqlx::query_scalar(
        "SELECT COUNT(*) FROM (SELECT 1 FROM messages m WHERE m.channel_id = ? AND m.id > ? AND m.author_id != ? AND (? \
           OR m.mention_everyone = 1 \
           OR EXISTS (SELECT 1 FROM message_mentions x WHERE x.message_id = m.id AND ( \
                (x.kind = 'user' AND x.target_id = ?) \
             OR (x.kind = 'role' AND x.target_id IN (SELECT role_id FROM member_roles WHERE user_id = ?))))) \
         LIMIT ?)",
    )
    .bind(channel_id)
    .bind(after)
    .bind(auth.user.id)
    .bind(is_dm)
    .bind(auth.user.id)
    .bind(auth.user.id)
    .bind(MAX_MENTION_COUNT)
    .fetch_one(&auth.srv.db)
    .await
}

/// `GET /read-states`: every text channel the caller can see and their direct messages.
pub async fn read_states(auth: Auth) -> ApiResult<Json<Vec<Value>>> {
    let rows: Vec<(i64, String, Option<i64>, Option<i64>)> = sqlx::query_as(
        "SELECT c.id, c.type, r.last_read_id, (SELECT MAX(id) FROM messages WHERE channel_id = c.id) \
         FROM channels c LEFT JOIN read_states r ON r.channel_id = c.id AND r.user_id = ? \
         WHERE (c.type = 'text' AND ?) OR (c.type = 'dm' AND ? IN (c.dm_a, c.dm_b))",
    )
    .bind(auth.user.id)
    .bind(perms::has(auth.perms, VIEW_CHANNEL))
    .bind(auth.user.id)
    .fetch_all(&auth.srv.db)
    .await?;
    let joined = crate::ids::at_ms(auth.user.created_at);
    let mut out = Vec::with_capacity(rows.len());
    for (id, kind, read, last) in rows {
        let read = read.unwrap_or(id.max(joined));
        let mentions = match last {
            Some(last) if last > read => mention_count(&auth, id, kind == "dm", read).await?,
            _ => 0,
        };
        out.push(json!({
            "channel_id": id.to_string(),
            "last_read_id": read.to_string(),
            "last_message_id": last.map(|v| v.to_string()),
            "mention_count": mentions,
        }));
    }
    Ok(Json(out))
}

#[derive(Deserialize)]
pub struct AckPath {
    id: String,
}

#[derive(Deserialize)]
pub struct AckReq {
    message_id: String,
}

/// `POST /channels/{id}/ack`: the caller has seen everything up to this message. Their other
/// tabs and devices are told, so badges clear everywhere.
pub async fn ack(auth: Auth, Path(p): Path<AckPath>, Body(req): Body<AckReq>) -> ApiResult<StatusCode> {
    let ch = super::messages::text_channel(&auth, &p.id).await?;
    let mid = crate::ids::parse(&req.message_id).ok_or_else(|| ApiError::validation(BTreeMap::from([("message_id", "invalid")])))?;
    mark_read(&auth.srv, auth.user.id, ch.id, mid).await?;
    let read: i64 = sqlx::query_scalar("SELECT last_read_id FROM read_states WHERE user_id = ? AND channel_id = ?").bind(auth.user.id).bind(ch.id).fetch_one(&auth.srv.db).await?;
    let mentions = mention_count(&auth, ch.id, ch.kind == "dm", read).await?;
    let data = json!({ "channel_id": ch.id.to_string(), "last_read_id": read.to_string(), "mention_count": mentions });
    auth.srv.emit(Event::new("read_state_update", data).for_users(vec![auth.user.id]));
    Ok(StatusCode::NO_CONTENT)
}

async fn preferences_out(auth: &Auth) -> ApiResult<Value> {
    let (notify, allow_dms): (String, bool) = sqlx::query_as("SELECT notify, allow_dms FROM users WHERE id = ?").bind(auth.user.id).fetch_one(&auth.srv.db).await?;
    let channels: Vec<(i64, String)> = sqlx::query_as("SELECT channel_id, level FROM channel_notify WHERE user_id = ?").bind(auth.user.id).fetch_all(&auth.srv.db).await?;
    let channels: HashMap<String, String> = channels.into_iter().map(|(id, level)| (id.to_string(), level)).collect();
    Ok(json!({ "notify": notify, "channels": channels, "allow_dms": allow_dms }))
}

/// `GET /me/preferences`
pub async fn get_preferences(auth: Auth) -> ApiResult<Json<Value>> {
    Ok(Json(preferences_out(&auth).await?))
}

#[derive(Deserialize)]
pub struct PreferencesReq {
    notify: Option<String>,
    /// Channel ID → level, or null to follow `notify` again.
    channels: Option<HashMap<String, Option<String>>>,
    allow_dms: Option<bool>,
}

/// `PATCH /me/preferences`
pub async fn patch_preferences(auth: Auth, Body(req): Body<PreferencesReq>) -> ApiResult<Json<Value>> {
    let bad = |field: &'static str, code: &'static str| ApiError::validation(BTreeMap::from([(field, code)]));
    if req.notify.as_deref().is_some_and(|l| !LEVELS.contains(&l)) {
        return Err(bad("notify", "invalid"));
    }
    let mut channels = Vec::new();
    for (id, level) in req.channels.unwrap_or_default() {
        if level.as_deref().is_some_and(|l| !LEVELS.contains(&l)) {
            return Err(bad("channels", "invalid"));
        }
        // Only channels the caller can see (a hidden channel looks the same as a missing one).
        let ch = super::messages::text_channel(&auth, &id).await.map_err(|_| bad("channels", "unknown_channel"))?;
        channels.push((ch.id, level));
    }
    let mut tx = auth.srv.db.begin().await?;
    if let Some(level) = &req.notify {
        sqlx::query("UPDATE users SET notify = ? WHERE id = ?").bind(level).bind(auth.user.id).execute(&mut *tx).await?;
    }
    if let Some(allow) = req.allow_dms {
        sqlx::query("UPDATE users SET allow_dms = ? WHERE id = ?").bind(allow).bind(auth.user.id).execute(&mut *tx).await?;
    }
    for (id, level) in &channels {
        match level {
            Some(level) => {
                sqlx::query("INSERT INTO channel_notify (user_id, channel_id, level) VALUES (?, ?, ?) ON CONFLICT (user_id, channel_id) DO UPDATE SET level = excluded.level")
                    .bind(auth.user.id)
                    .bind(id)
                    .bind(level)
                    .execute(&mut *tx)
                    .await?
            }
            None => sqlx::query("DELETE FROM channel_notify WHERE user_id = ? AND channel_id = ?").bind(auth.user.id).bind(id).execute(&mut *tx).await?,
        };
    }
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM channel_notify WHERE user_id = ?").bind(auth.user.id).fetch_one(&mut *tx).await?;
    if count as usize > MAX_CHANNEL_SETTINGS {
        return Err(bad("channels", "too_many"));
    }
    tx.commit().await?;
    let out = preferences_out(&auth).await?;
    auth.srv.emit(Event::new("preferences_update", &out).for_users(vec![auth.user.id]));
    Ok(Json(out))
}
