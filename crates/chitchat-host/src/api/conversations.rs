//! Pins, thread channels, and role-specific posting rules.

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
use crate::models::{ChannelOut, ChannelRow, now_ms, ts};
use crate::perms::{self, MANAGE_MESSAGES, MANAGE_ROLES, SEND_MESSAGES};

#[derive(Deserialize)]
pub struct ChannelPath {
    id: String,
}
#[derive(Deserialize)]
pub struct MessagePath {
    id: String,
    mid: String,
}
#[derive(Deserialize)]
pub struct RulePath {
    id: String,
    role_id: String,
}

pub fn check_open(ch: &ChannelRow) -> ApiResult<()> {
    if ch.archived { Err(ApiError::forbidden("thread_archived", "This thread is archived. Ask a channel manager to reopen it.")) } else { Ok(()) }
}

pub async fn pins(auth: Auth, Path(p): Path<ChannelPath>) -> ApiResult<Json<Vec<Value>>> {
    let ch = super::messages::text_channel(&auth, &p.id).await?;
    let rows: Vec<(i64, i64, i64)> = sqlx::query_as("SELECT p.message_id, p.pinned_by, p.created_at FROM message_pins p JOIN messages m ON m.id = p.message_id WHERE m.channel_id = ? ORDER BY p.created_at DESC, p.message_id DESC LIMIT 50")
        .bind(ch.id).fetch_all(&auth.srv.db).await?;
    let mut out = Vec::new();
    for (id, by, at) in rows {
        if let Some(row) = db::message_row(&auth.srv.db, ch.id, id).await? {
            let message = db::messages_out(&auth.srv, &[row], auth.user.id).await?.remove(0);
            out.push(json!({ "message": message, "pinned_by": by.to_string(), "pinned_at": ts(at) }));
        }
    }
    Ok(Json(out))
}

pub async fn pin(auth: Auth, Path(p): Path<MessagePath>) -> ApiResult<StatusCode> {
    let ch = super::messages::text_channel(&auth, &p.id).await?;
    if ch.kind != "dm" {
        auth.require(MANAGE_MESSAGES)?;
    }
    super::moderation::check_not_timed_out(&auth.user)?;
    let msg = super::messages::message(&auth, ch.id, &p.mid).await?;
    let now = now_ms();
    let inserted = sqlx::query("INSERT OR IGNORE INTO message_pins (message_id, pinned_by, created_at) SELECT ?, ?, ? WHERE (SELECT COUNT(*) FROM message_pins p JOIN messages m ON m.id = p.message_id WHERE m.channel_id = ?) < 50")
        .bind(msg.id).bind(auth.user.id).bind(now).bind(ch.id).execute(&auth.srv.db).await?;
    if inserted.rows_affected() == 0 {
        let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM message_pins WHERE message_id = ?)").bind(msg.id).fetch_one(&auth.srv.db).await?;
        if !exists {
            return Err(ApiError::new(StatusCode::CONFLICT, "too_many_pins", "A conversation can have at most 50 pinned messages."));
        }
    } else {
        auth.srv.emit(super::messages::scoped(
            Event::new(
                "pin_update",
                json!({"channel_id": ch.id.to_string(), "message_id": msg.id.to_string(), "pinned": true, "pinned_by": auth.user.id.to_string(), "pinned_at": ts(now)}),
            ),
            &ch,
        ));
    }
    Ok(StatusCode::NO_CONTENT)
}

pub async fn unpin(auth: Auth, Path(p): Path<MessagePath>) -> ApiResult<StatusCode> {
    let ch = super::messages::text_channel(&auth, &p.id).await?;
    if ch.kind != "dm" {
        auth.require(MANAGE_MESSAGES)?;
    }
    super::moderation::check_not_timed_out(&auth.user)?;
    let msg = super::messages::message(&auth, ch.id, &p.mid).await?;
    if sqlx::query("DELETE FROM message_pins WHERE message_id = ?").bind(msg.id).execute(&auth.srv.db).await?.rows_affected() > 0 {
        auth.srv.emit(super::messages::scoped(Event::new("pin_update", json!({"channel_id": ch.id.to_string(), "message_id": msg.id.to_string(), "pinned": false})), &ch));
    }
    Ok(StatusCode::NO_CONTENT)
}

pub async fn threads(auth: Auth, Path(p): Path<ChannelPath>) -> ApiResult<Json<Vec<ChannelOut>>> {
    let ch = super::messages::text_channel(&auth, &p.id).await?;
    let channels = db::list_channels(&auth.srv.db, auth.user.id, auth.perms).await?;
    Ok(Json(channels.into_iter().filter(|c| c.parent_id.as_deref() == Some(&ch.id.to_string())).collect()))
}

#[derive(Deserialize)]
pub struct ThreadReq {
    name: Option<String>,
}

pub async fn create_thread(auth: Auth, Path(p): Path<MessagePath>, Body(req): Body<ThreadReq>) -> ApiResult<(StatusCode, Json<ChannelOut>)> {
    let parent = super::messages::text_channel(&auth, &p.id).await?;
    if parent.kind != "text" || parent.parent_id.is_some() {
        return Err(ApiError::bad_request("Start a thread from a message in a server text channel."));
    }
    auth.require_in(&parent, SEND_MESSAGES).await?;
    super::moderation::check_not_timed_out(&auth.user)?;
    super::moderation::check_new_account(&auth, false).await?;
    let root = super::messages::message(&auth, parent.id, &p.mid).await?;
    let name = req.name.unwrap_or_else(|| root.content.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(80).collect());
    let name = if name.trim().is_empty() { "Thread" } else { name.trim() };
    if name.chars().count() > 100 || name.chars().any(char::is_control) {
        return Err(ApiError::validation(BTreeMap::from([("name", "invalid")])));
    }
    let id = crate::ids::next_id();
    let inserted = sqlx::query("INSERT OR IGNORE INTO channels (id, type, name, category_id, position, media_allowed, parent_id, root_message_id) SELECT ?, 'text', ?, ?, 0, ?, ?, ? WHERE (SELECT COUNT(*) FROM channels WHERE parent_id = ?) < 100 AND (SELECT COUNT(*) FROM channels WHERE parent_id IS NOT NULL) < 1000")
        .bind(id).bind(name).bind(parent.category_id).bind(parent.media_allowed).bind(parent.id).bind(root.id).bind(parent.id).execute(&auth.srv.db).await?;
    let row: Option<ChannelRow> = sqlx::query_as("SELECT * FROM channels WHERE root_message_id = ?").bind(root.id).fetch_optional(&auth.srv.db).await?;
    let row = row.ok_or_else(|| ApiError::new(StatusCode::CONFLICT, "too_many_threads", "The thread limit has been reached. Ask a channel manager to remove old threads."))?;
    let out = ChannelOut::new(&row, db::channel_perms(&auth.srv.db, auth.user.id, auth.perms, &row).await?);
    if inserted.rows_affected() > 0 {
        auth.srv.emit(Event::channel("channel_create", ChannelOut::new(&row, 0), row.id).refresh());
        if let Some(root) = db::message_row(&auth.srv.db, parent.id, root.id).await? {
            let message = db::messages_out(&auth.srv, &[root], 0).await?.remove(0);
            auth.srv.emit(Event::new("message_update", message).in_channel(parent.id));
        }
    }
    Ok((if inserted.rows_affected() > 0 { StatusCode::CREATED } else { StatusCode::OK }, Json(out)))
}

pub async fn permissions(auth: Auth, Path(p): Path<ChannelPath>) -> ApiResult<Json<Value>> {
    let ch = super::messages::text_channel(&auth, &p.id).await?;
    let rows: Vec<(i64, i64, i64)> =
        sqlx::query_as("SELECT role_id, allow, deny FROM channel_overwrites WHERE channel_id = ? ORDER BY role_id").bind(ch.id).fetch_all(&auth.srv.db).await?;
    Ok(Json(
        json!({"inherited_from": ch.parent_id.map(|id| id.to_string()), "overwrites": rows.into_iter().map(|(id,a,d)| json!({"role_id":id.to_string(),"allow":a.to_string(),"deny":d.to_string()})).collect::<Vec<_>>()}),
    ))
}

async fn rule_target(auth: &Auth, p: &RulePath) -> ApiResult<(ChannelRow, i64)> {
    auth.require(MANAGE_ROLES)?;
    let ch = super::messages::text_channel(auth, &p.id).await?;
    if ch.kind != "text" {
        return Err(ApiError::bad_request("Posting rules apply to server text channels and threads."));
    }
    let role_id = parse_id(&p.role_id, "not_found")?;
    let role: Option<(i64, bool)> = sqlx::query_as("SELECT position, is_default FROM roles WHERE id = ?").bind(role_id).fetch_optional(&auth.srv.db).await?;
    let (position, everyone) = role.ok_or_else(|| ApiError::not_found("not_found", "No such role."))?;
    if !auth.is_owner && !everyone {
        let top: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(r.position), 0) FROM roles r JOIN member_roles m ON r.id = m.role_id WHERE m.user_id = ?")
            .bind(auth.user.id)
            .fetch_one(&auth.srv.db)
            .await?;
        if position >= top {
            return Err(ApiError::forbidden("forbidden", "You can only change rules for roles below your highest role."));
        }
    }
    Ok((ch, role_id))
}

#[derive(Deserialize)]
pub struct RuleReq {
    allow: String,
    deny: String,
}

pub async fn set_permission(auth: Auth, Path(p): Path<RulePath>, Body(req): Body<RuleReq>) -> ApiResult<StatusCode> {
    let (ch, role_id) = rule_target(&auth, &p).await?;
    let invalid = || ApiError::bad_request("Rules can only allow or deny writing, file uploads, and reactions; a bit cannot be both allowed and denied.");
    let allow = req.allow.parse::<u64>().map_err(|_| invalid())?;
    let deny = req.deny.parse::<u64>().map_err(|_| invalid())?;
    if (allow | deny) & !perms::CHANNEL_OVERRIDES != 0 || allow & deny != 0 {
        return Err(invalid());
    }
    if !auth.is_owner && allow & !auth.perms != 0 {
        return Err(ApiError::missing_permission());
    }
    sqlx::query("INSERT INTO channel_overwrites (channel_id, role_id, allow, deny) VALUES (?, ?, ?, ?) ON CONFLICT(channel_id, role_id) DO UPDATE SET allow = excluded.allow, deny = excluded.deny")
        .bind(ch.id).bind(role_id).bind(allow as i64).bind(deny as i64).execute(&auth.srv.db).await?;
    super::moderation::audit(&auth.srv, Some(auth.user.id), "channel_permissions", Some(ch.id), json!({"role_id":role_id.to_string(),"allow":req.allow,"deny":req.deny})).await;
    auth.srv.emit(Event::channel("channel_update", ChannelOut::new(&ch, 0), ch.id).refresh());
    Ok(StatusCode::NO_CONTENT)
}

pub async fn clear_permission(auth: Auth, Path(p): Path<RulePath>) -> ApiResult<StatusCode> {
    let (ch, role_id) = rule_target(&auth, &p).await?;
    sqlx::query("DELETE FROM channel_overwrites WHERE channel_id = ? AND role_id = ?").bind(ch.id).bind(role_id).execute(&auth.srv.db).await?;
    auth.srv.emit(Event::channel("channel_update", ChannelOut::new(&ch, 0), ch.id).refresh());
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::Host;

    #[tokio::test]
    async fn pins_threads_and_inherited_posting_rules() {
        let dir = std::env::temp_dir().join(format!("chitchat-conversations-{}", crate::ids::next_id()));
        let host = Host::load(&dir).await.unwrap();
        host.create_server("threads", "Threads").await.unwrap();
        let srv = host.get("threads").await.unwrap();
        let user_id = crate::ids::next_id();
        sqlx::query("INSERT INTO users (id, username, display_name, password_hash, created_at) VALUES (?, 'writer', 'Writer', 'unused', ?)")
            .bind(user_id)
            .bind(now_ms())
            .execute(&srv.db)
            .await
            .unwrap();
        let channel: i64 = sqlx::query_scalar("SELECT id FROM channels WHERE type = 'text' LIMIT 1").fetch_one(&srv.db).await.unwrap();
        let root = crate::ids::next_id();
        sqlx::query("INSERT INTO messages (id, channel_id, author_id, content, created_at) VALUES (?, ?, ?, 'Topic\nwith more text', ?)")
            .bind(root)
            .bind(channel)
            .bind(user_id)
            .bind(now_ms())
            .execute(&srv.db)
            .await
            .unwrap();
        let auth = |owner: bool| {
            let srv = srv.clone();
            async move {
                Auth {
                    user: db::user_by_id(&srv.db, user_id).await.unwrap().unwrap(),
                    srv,
                    perms: if owner { perms::ALL } else { perms::DEFAULT_EVERYONE },
                    is_owner: owner,
                    token_hash: "test".into(),
                }
            }
        };
        let path = || Path(MessagePath { id: channel.to_string(), mid: root.to_string() });
        assert!(pin(auth(false).await, path()).await.is_err());
        pin(auth(true).await, path()).await.unwrap();
        assert!(db::message_row(&srv.db, channel, root).await.unwrap().unwrap().pinned);
        assert_eq!(pins(auth(false).await, Path(ChannelPath { id: channel.to_string() })).await.unwrap().0.len(), 1);
        let (status, Json(thread)) = create_thread(auth(false).await, path(), Body(ThreadReq { name: None })).await.unwrap();
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(thread.parent_id, Some(channel.to_string()));
        assert_eq!(thread.name, "Topic with more text");
        assert_eq!(create_thread(auth(false).await, path(), Body(ThreadReq { name: None })).await.unwrap().0, StatusCode::OK);
        assert_eq!(db::message_row(&srv.db, channel, root).await.unwrap().unwrap().thread_id.unwrap().to_string(), thread.id);
        let everyone: i64 = sqlx::query_scalar("SELECT id FROM roles WHERE is_default = 1").fetch_one(&srv.db).await.unwrap();
        set_permission(
            auth(true).await,
            Path(RulePath { id: channel.to_string(), role_id: everyone.to_string() }),
            Body(RuleReq { allow: "0".into(), deny: SEND_MESSAGES.to_string() }),
        )
        .await
        .unwrap();
        let child = db::channel_row(&srv.db, thread.id.parse().unwrap()).await.unwrap().unwrap();
        assert!(auth(false).await.require_in(&child, SEND_MESSAGES).await.is_err());
        set_permission(
            auth(true).await,
            Path(RulePath { id: thread.id.clone(), role_id: everyone.to_string() }),
            Body(RuleReq { allow: SEND_MESSAGES.to_string(), deny: "0".into() }),
        )
        .await
        .unwrap();
        assert!(auth(false).await.require_in(&child, SEND_MESSAGES).await.is_ok());
        assert!(
            set_permission(
                auth(false).await,
                Path(RulePath { id: thread.id.clone(), role_id: everyone.to_string() }),
                Body(RuleReq { allow: perms::ALL.to_string(), deny: "0".into() })
            )
            .await
            .is_err()
        );
        unpin(auth(true).await, path()).await.unwrap();
        assert!(!db::message_row(&srv.db, channel, root).await.unwrap().unwrap().pinned);
        sqlx::query("DELETE FROM messages WHERE id = ?").bind(root).execute(&srv.db).await.unwrap();
        assert!(db::channel_row(&srv.db, child.id).await.unwrap().unwrap().root_message_id.is_none());
        sqlx::query("DELETE FROM channels WHERE id = ?").bind(channel).execute(&srv.db).await.unwrap();
        assert!(db::channel_row(&srv.db, child.id).await.unwrap().is_none());
        srv.db.close().await;
        drop(srv);
        drop(host);
        assert!(dir.canonicalize().unwrap().starts_with(std::env::temp_dir().canonicalize().unwrap()));
        for attempt in 0..20 {
            match std::fs::remove_dir_all(&dir) {
                Ok(()) => break,
                Err(e) if attempt < 19 && matches!(e.raw_os_error(), Some(32 | 33)) => tokio::time::sleep(std::time::Duration::from_millis(50)).await,
                Err(e) => panic!("remove temporary threads database: {e}"),
            }
        }
    }
}
