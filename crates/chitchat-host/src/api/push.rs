//! Web Push subscriptions, and sending a push for a new message to the people who'd be
//! notified about it but don't have ChitChat on screen. Encryption and delivery: `crate::push`.

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::Json;
use axum::http::StatusCode;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{ApiError, ApiResult, Auth, Body};
use crate::host::ServerCtx;
use crate::models::{ChannelRow, MessageOut, now_ms};
use crate::perms::{self, VIEW_CHANNEL};
use crate::push::{self, Outcome};

const MAX_PER_USER: i64 = 10;
const PREVIEW_CHARS: usize = 180;

/// `GET /push/key`: the key a browser subscribes with (the same for every server on this host).
pub async fn key(_auth: Auth) -> ApiResult<Json<Value>> {
    let key = push::public_key().ok_or_else(ApiError::internal)?;
    Ok(Json(json!({ "public_key": key })))
}

#[derive(Deserialize)]
pub struct Keys {
    p256dh: String,
    auth: String,
}

#[derive(Deserialize)]
pub struct SubscribeReq {
    endpoint: String,
    keys: Keys,
}

/// `PUT /push/subscription`: this browser wants pushes for the caller (`PushSubscription.toJSON()`).
pub async fn subscribe(auth: Auth, Body(req): Body<SubscribeReq>) -> ApiResult<StatusCode> {
    let bad = |field: &'static str, code: &'static str| ApiError::validation(BTreeMap::from([(field, code)]));
    if !push::allowed_endpoint(&req.endpoint) {
        return Err(bad("endpoint", "unsupported"));
    }
    let len = |s: &str| B64.decode(s.trim_end_matches('=')).map(|b| b.len()).unwrap_or(0);
    if len(&req.keys.p256dh) != 65 || len(&req.keys.auth) != 16 {
        return Err(bad("keys", "invalid"));
    }
    // A browser has one subscription: whoever subscribes it last on this server receives.
    sqlx::query("INSERT OR REPLACE INTO push_subscriptions (endpoint, user_id, token_hash, p256dh, auth, created_at) VALUES (?, ?, ?, ?, ?, ?)")
        .bind(&req.endpoint)
        .bind(auth.user.id)
        .bind(&auth.token_hash)
        .bind(&req.keys.p256dh)
        .bind(&req.keys.auth)
        .bind(now_ms())
        .execute(&auth.srv.db)
        .await?;
    sqlx::query(
        "DELETE FROM push_subscriptions WHERE user_id = ? AND endpoint NOT IN (SELECT endpoint FROM push_subscriptions WHERE user_id = ? ORDER BY created_at DESC LIMIT ?)",
    )
    .bind(auth.user.id)
    .bind(auth.user.id)
    .bind(MAX_PER_USER)
    .execute(&auth.srv.db)
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct UnsubscribeReq {
    endpoint: String,
}

/// `DELETE /push/subscription {endpoint}`
pub async fn unsubscribe(auth: Auth, Body(req): Body<UnsubscribeReq>) -> ApiResult<StatusCode> {
    sqlx::query("DELETE FROM push_subscriptions WHERE endpoint = ? AND user_id = ?").bind(&req.endpoint).bind(auth.user.id).execute(&auth.srv.db).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Readable text for a notification: mentions become names, long text is cut.
async fn preview(srv: &ServerCtx, msg: &MessageOut) -> String {
    let mut text = msg.content.clone();
    for id in msg.mentions.iter() {
        let name: Option<String> = sqlx::query_scalar("SELECT display_name FROM users WHERE id = ?").bind(id).fetch_optional(&srv.db).await.ok().flatten();
        text = text.replace(&format!("<@{id}>"), &format!("@{}", name.unwrap_or_else(|| "someone".into())));
    }
    for id in msg.mention_roles.iter() {
        let name: Option<String> = sqlx::query_scalar("SELECT name FROM roles WHERE id = ?").bind(id).fetch_optional(&srv.db).await.ok().flatten();
        text = text.replace(&format!("<@&{id}>"), &format!("@{}", name.unwrap_or_else(|| "role".into())));
    }
    // Channel links and anything left over.
    while let Some(start) = text.find("<#") {
        let Some(len) = text[start..].find('>') else { break };
        let id = text[start + 2..start + len].to_string();
        let name: Option<String> = match crate::ids::parse(&id) {
            Some(cid) => crate::db::channel_row(&srv.db, cid).await.ok().flatten().map(|c| c.name),
            None => None,
        };
        text.replace_range(start..start + len + 1, &format!("#{}", name.unwrap_or_else(|| "channel".into())));
    }
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.is_empty() {
        return match msg.attachments.first() {
            Some(a) if a.voice => "Sent a voice message".into(),
            Some(_) => "Sent a file".into(),
            None => String::new(),
        };
    }
    if text.chars().count() > PREVIEW_CHARS { format!("{}…", text.chars().take(PREVIEW_CHARS).collect::<String>()) } else { text }
}

/// Whether `user` would be notified about `msg` (their preferences and pings), ignoring
/// whether they're watching.
async fn wants(srv: &ServerCtx, ch: &ChannelRow, msg: &MessageOut, user: i64, notify: &str) -> sqlx::Result<bool> {
    if ch.kind == "dm" {
        return Ok(ch.dm_members().contains(&user) && notify != "none");
    }
    let (p, _) = crate::db::user_perms(&srv.db, user).await?;
    if !perms::has(p, VIEW_CHANNEL) {
        return Ok(false);
    }
    let level: Option<String> = sqlx::query_scalar("SELECT level FROM channel_notify WHERE user_id = ? AND channel_id = ?").bind(user).bind(ch.id).fetch_optional(&srv.db).await?;
    match level.as_deref().unwrap_or(notify) {
        "all" => Ok(true),
        "mentions" => {
            let me = user.to_string();
            if msg.mention_everyone || msg.mentions.contains(&me) {
                return Ok(true);
            }
            if msg.mention_roles.is_empty() {
                return Ok(false);
            }
            let roles: Vec<i64> = sqlx::query_scalar("SELECT role_id FROM member_roles WHERE user_id = ?").bind(user).fetch_all(&srv.db).await?;
            Ok(msg.mention_roles.iter().any(|r| roles.iter().any(|x| x.to_string() == *r)))
        }
        _ => Ok(false),
    }
}

/// Sends pushes for a new message (in the background; never delays sending the message).
pub fn message_created(srv: Arc<ServerCtx>, ch: ChannelRow, msg: MessageOut) {
    tokio::spawn(async move {
        if let Err(e) = deliver(&srv, &ch, &msg).await {
            tracing::warn!(server = %srv.slug, "push delivery: {e:#}");
        }
    });
}

async fn deliver(srv: &ServerCtx, ch: &ChannelRow, msg: &MessageOut) -> anyhow::Result<()> {
    let author = crate::ids::parse(&msg.author.id).unwrap_or(0);
    let subs: Vec<(String, i64, String, String, String)> =
        sqlx::query_as("SELECT s.endpoint, s.user_id, s.p256dh, s.auth, u.notify FROM push_subscriptions s JOIN users u ON u.id = s.user_id WHERE s.user_id != ?")
            .bind(author)
            .fetch_all(&srv.db)
            .await?;
    if subs.is_empty() {
        return Ok(());
    }
    let server = crate::db::server_row(&srv.db).await?.name;
    let body = preview(srv, msg).await;
    let mut decided: BTreeMap<i64, bool> = BTreeMap::new();
    for (endpoint, user, p256dh, auth, notify) in subs {
        let wanted = match decided.get(&user) {
            Some(w) => *w,
            None => {
                let w = !srv.is_watching(user) && !super::moderation::user_banned(srv, user).await? && wants(srv, ch, msg, user, &notify).await?;
                decided.insert(user, w);
                w
            }
        };
        if !wanted {
            continue;
        }
        let me = user.to_string();
        let urgent = ch.kind == "dm" || msg.mention_everyone || msg.mentions.contains(&me) || !msg.mention_roles.is_empty();
        let title = if ch.kind == "dm" { msg.author.display_name.clone() } else { format!("{} · #{} · {}", msg.author.display_name, ch.name, server) };
        let payload = json!({
            "title": title,
            "body": body,
            "slug": srv.slug,
            "channel_id": ch.id.to_string(),
            "message_id": msg.id,
            "tag": format!("{}:{}", srv.slug, ch.id),
        })
        .to_string();
        let topic = format!("c{}", ch.id);
        match push::send(&endpoint, &p256dh, &auth, payload.as_bytes(), &topic, urgent).await {
            Outcome::Gone => {
                sqlx::query("DELETE FROM push_subscriptions WHERE endpoint = ?").bind(&endpoint).execute(&srv.db).await?;
            }
            Outcome::Sent | Outcome::Failed => {}
        }
    }
    Ok(())
}
