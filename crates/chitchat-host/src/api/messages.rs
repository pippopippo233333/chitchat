use std::collections::BTreeMap;

use axum::Json;
use axum::extract::{Path, Query};
use axum::http::StatusCode;
use serde::Deserialize;
use serde_json::json;

use super::extract::parse_id;
use super::{ApiError, ApiResult, Auth, Body};
use crate::db::{self, Page};
use crate::events::Event;
use crate::ids::next_id;
use crate::models::{ChannelRow, MessageOut, now_ms, ts};
use crate::perms::{self, ADD_REACTIONS, MANAGE_MESSAGES, MENTION_EVERYONE, SEND_DMS, SEND_MESSAGES, VIEW_CHANNEL};
use crate::spam::Verdict;

pub const MAX_CHARS: usize = 4000;
pub const MAX_LINES: usize = 60;

#[derive(Deserialize)]
pub struct ChanPath {
    id: String,
}

#[derive(Deserialize)]
pub struct MsgPath {
    id: String,
    mid: String,
}

#[derive(Deserialize)]
pub struct ReactPath {
    id: String,
    mid: String,
    emoji: String,
}

pub fn check_content(raw: &str) -> Result<String, &'static str> {
    let c = raw.trim();
    if c.is_empty() {
        return Err("empty");
    }
    if c.chars().count() > MAX_CHARS {
        return Err("too_long");
    }
    if c.lines().count() > MAX_LINES {
        return Err("too_many_lines");
    }
    Ok(c.to_string())
}

fn check_emoji(e: &str) -> ApiResult<()> {
    if e.is_empty() || e.len() > 32 || e.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(ApiError::validation(BTreeMap::from([("emoji", "invalid")])));
    }
    Ok(())
}

/// The text channel or direct message in the path, if the caller may see it.
pub(super) async fn text_channel(auth: &Auth, id: &str) -> ApiResult<ChannelRow> {
    let unknown = || ApiError::not_found("unknown_channel", "No such channel.");
    let id = parse_id(id, "unknown_channel")?;
    let ch = db::any_channel_row(&auth.srv.db, id).await?.ok_or_else(unknown)?;
    match ch.kind.as_str() {
        "text" => auth.require(VIEW_CHANNEL).map_err(|_| unknown())?,
        "dm" if ch.dm_members().contains(&auth.user.id) => {}
        "dm" => return Err(unknown()),
        _ => return Err(ApiError::bad_request("This isn't a text channel.")),
    }
    Ok(ch)
}

/// Sends a channel's event to whoever may see it: everyone who can view channels, or just the
/// two people in a direct message.
pub fn scoped(ev: Event, ch: &ChannelRow) -> Event {
    if ch.kind == "dm" { ev.for_users(ch.dm_members()) } else { ev.in_channel(ch.id) }
}

/// `scoped` for code that only has the channel's ID (cleanups, bans). Nothing is sent if the
/// channel is gone.
pub async fn emit_in(srv: &crate::host::ServerCtx, ev: Event, channel_id: i64) -> sqlx::Result<()> {
    if let Some(ch) = db::any_channel_row(&srv.db, channel_id).await? {
        srv.emit(scoped(ev, &ch));
    }
    Ok(())
}

/// Sending in a direct message: SEND_DMS, and the other person still here and accepting DMs.
async fn check_dm_send(auth: &Auth, ch: &ChannelRow) -> ApiResult<()> {
    auth.require(SEND_DMS)?;
    let gone = || ApiError::forbidden("dm_unavailable", "This person is no longer on the server.");
    let other = ch.dm_members().into_iter().find(|u| *u != auth.user.id).unwrap_or(auth.user.id);
    let allow: Option<bool> = sqlx::query_scalar("SELECT allow_dms FROM users WHERE id = ?").bind(other).fetch_optional(&auth.srv.db).await?;
    match allow {
        None => Err(gone()),
        Some(_) if super::moderation::user_banned(&auth.srv, other).await? => Err(gone()),
        Some(false) => Err(ApiError::forbidden("dms_closed", "This person doesn't accept direct messages.")),
        Some(true) => Ok(()),
    }
}

/// Mentions written in a message: `<@id>`, `<@&id>`, `@everyone`, `@here`.
#[derive(Debug, Default, PartialEq)]
pub struct Mentions {
    pub users: Vec<i64>,
    pub roles: Vec<i64>,
    /// 0 = none, 1 = @everyone, 2 = @here.
    pub everyone: i64,
}

const MAX_MENTIONS: usize = 50;

pub fn parse_mentions(content: &str) -> Mentions {
    let mut m = Mentions::default();
    let mut rest = content;
    while let Some(i) = rest.find("<@") {
        rest = &rest[i + 2..];
        let (role, digits) = match rest.strip_prefix('&') {
            Some(r) => (true, r),
            None => (false, rest),
        };
        let end = digits.find(|c: char| !c.is_ascii_digit()).unwrap_or(digits.len());
        if end > 0
            && digits[end..].starts_with('>')
            && let Some(id) = crate::ids::parse(&digits[..end])
        {
            let full = m.users.len() + m.roles.len() >= MAX_MENTIONS;
            let list = if role { &mut m.roles } else { &mut m.users };
            if !list.contains(&id) && !full {
                list.push(id);
            }
        }
    }
    // A whole word: not part of an e-mail address or a longer word.
    let word = |w: &str| {
        content.match_indices(w).any(|(i, _)| {
            let before = content[..i].chars().next_back();
            let after = content[i + w.len()..].chars().next();
            before.is_none_or(|c| !c.is_alphanumeric() && c != '_') && after.is_none_or(|c| !c.is_alphanumeric() && c != '_')
        })
    };
    m.everyone = if word("@everyone") {
        1
    } else if word("@here") {
        2
    } else {
        0
    };
    m
}

/// Stores a message's mentions (replacing earlier ones after an edit). Role pings and
/// `@everyone`/`@here` count only with MENTION_EVERYONE; mentions in direct messages aren't kept.
async fn store_mentions(auth: &Auth, ch: &ChannelRow, message_id: i64, content: &str) -> ApiResult<()> {
    let m = if ch.kind == "dm" { Mentions::default() } else { parse_mentions(content) };
    let can_ping_all = perms::has(auth.perms, MENTION_EVERYONE);
    let mut tx = auth.srv.db.begin().await?;
    sqlx::query("DELETE FROM message_mentions WHERE message_id = ?").bind(message_id).execute(&mut *tx).await?;
    for u in &m.users {
        sqlx::query("INSERT OR IGNORE INTO message_mentions (message_id, kind, target_id) SELECT ?, 'user', id FROM users WHERE id = ?")
            .bind(message_id)
            .bind(u)
            .execute(&mut *tx)
            .await?;
    }
    if can_ping_all {
        for r in &m.roles {
            sqlx::query("INSERT OR IGNORE INTO message_mentions (message_id, kind, target_id) SELECT ?, 'role', id FROM roles WHERE id = ? AND is_default = 0")
                .bind(message_id)
                .bind(r)
                .execute(&mut *tx)
                .await?;
        }
    }
    let everyone = if can_ping_all { m.everyone } else { 0 };
    sqlx::query("UPDATE messages SET mention_everyone = ? WHERE id = ?").bind(everyone).bind(message_id).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}

async fn message(auth: &Auth, channel_id: i64, mid: &str) -> ApiResult<crate::models::MessageRow> {
    let mid = parse_id(mid, "unknown_message")?;
    db::message_row(&auth.srv.db, channel_id, mid).await?.ok_or_else(|| ApiError::not_found("unknown_message", "No such message."))
}

#[derive(Deserialize)]
pub struct PageQuery {
    before: Option<String>,
    after: Option<String>,
    around: Option<String>,
    limit: Option<i64>,
}

pub async fn list(auth: Auth, Path(p): Path<ChanPath>, Query(q): Query<PageQuery>) -> ApiResult<Json<Vec<MessageOut>>> {
    let ch = text_channel(&auth, &p.id).await?;
    let limit = q.limit.unwrap_or(50).clamp(1, 100);
    let cursor = |s: &str| crate::ids::parse(s).ok_or_else(|| ApiError::bad_request("Invalid cursor."));
    let page = match (q.around.as_deref(), q.before.as_deref(), q.after.as_deref()) {
        (Some(a), _, _) => Page::Around(cursor(a)?),
        (None, Some(b), _) => Page::Before(cursor(b)?),
        (None, None, Some(a)) => Page::After(cursor(a)?),
        (None, None, None) => Page::Latest,
    };
    let rows = db::message_page(&auth.srv.db, ch.id, page, limit).await?;
    Ok(Json(db::messages_out(&auth.srv, &rows, auth.user.id).await?))
}

#[derive(Deserialize)]
pub struct SendReq {
    #[serde(default)]
    content: String,
    #[serde(default)]
    attachment_ids: Vec<String>,
    reply_to: Option<String>,
    nonce: Option<String>,
}

pub async fn create(auth: Auth, Path(p): Path<ChanPath>, Body(req): Body<SendReq>) -> ApiResult<(StatusCode, Json<MessageOut>)> {
    let ch = text_channel(&auth, &p.id).await?;
    if ch.kind == "dm" {
        check_dm_send(&auth, &ch).await?;
    } else {
        auth.require(SEND_MESSAGES)?;
    }
    let mut fields = BTreeMap::new();
    let attachments = super::files::check_for_message(&auth, ch.id, &req.attachment_ids).await?;
    let content =
        if !attachments.is_empty() && req.content.trim().is_empty() { Some(String::new()) } else { check_content(&req.content).map_err(|e| fields.insert("content", e)).ok() };
    if req.nonce.as_ref().is_some_and(|n| n.chars().count() > 64) {
        fields.insert("nonce", "too_long");
    }
    let reply_to = match req.reply_to.as_deref() {
        None | Some("") => None,
        Some(r) => match crate::ids::parse(r) {
            Some(rid) if db::message_row(&auth.srv.db, ch.id, rid).await?.is_some() => Some(rid),
            _ => {
                fields.insert("reply_to", "unknown_message");
                None
            }
        },
    };
    let Some(content) = content.filter(|_| fields.is_empty()) else {
        return Err(ApiError::validation(fields));
    };

    super::moderation::check_not_timed_out(&auth.user)?;
    super::moderation::check_new_account(&auth, !attachments.is_empty() || super::moderation::has_link(&content)).await?;

    // The owner is never throttled; everyone else goes through the spam guard.
    if !auth.is_owner {
        // Attached files count as part of the text for the repeat filter.
        let fingerprint = format!("{content}{}", super::files::blob_hashes(&auth.srv, &attachments).await?);
        let verdict = auth.srv.spam.lock().unwrap().check(auth.user.id, &fingerprint);
        match verdict {
            Verdict::Allow => {}
            Verdict::Slow { retry_after, reason } => {
                return Err(ApiError::new(StatusCode::TOO_MANY_REQUESTS, "rate_limited", reason).with("retry_after_ms", json!(retry_after.as_millis() as u64)));
            }
            Verdict::TimedOut { remaining } => {
                let until = now_ms() + remaining.as_millis() as i64;
                tracing::warn!(server = %auth.srv.slug, user = %auth.user.username, "automatic spam timeout");
                if auth.user.timed_out_until.is_none_or(|t| t < until) {
                    super::moderation::set_timeout(&auth.srv, auth.user.id, Some(until)).await?;
                    super::moderation::audit(&auth.srv, None, "member_timeout", Some(auth.user.id), json!({ "reason": "automatic: spam", "minutes": remaining.as_secs() / 60 }))
                        .await;
                }
                return Err(ApiError::forbidden("timed_out", "You were muted for a while for sending too much too fast.").with("until", json!(ts(until))));
            }
        }
    }

    let _room = super::storage::make_room(&auth.srv, content.len() as i64).await?;
    let id = next_id();
    let mut tx = auth.srv.db.begin().await?;
    sqlx::query("INSERT INTO messages (id, channel_id, author_id, content, created_at, reply_to) VALUES (?, ?, ?, ?, ?, ?)")
        .bind(id)
        .bind(ch.id)
        .bind(auth.user.id)
        .bind(&content)
        .bind(now_ms())
        .bind(reply_to)
        .execute(&mut *tx)
        .await?;
    for aid in &attachments {
        let used = sqlx::query("UPDATE attachments SET message_id = ? WHERE id = ? AND message_id IS NULL").bind(id).bind(aid).execute(&mut *tx).await?;
        if used.rows_affected() != 1 {
            return Err(ApiError::validation(BTreeMap::from([("attachment_ids", "unknown_attachment")])));
        }
    }
    tx.commit().await?;
    store_mentions(&auth, &ch, id, &content).await?;
    // Your own message is read.
    super::notify::mark_read(&auth.srv, auth.user.id, ch.id, id).await?;
    let row = db::message_row(&auth.srv.db, ch.id, id).await?.ok_or_else(ApiError::internal)?;
    let mut out = db::messages_out(&auth.srv, std::slice::from_ref(&row), 0).await?.remove(0);
    out.nonce = req.nonce;
    auth.srv.emit(scoped(Event::new("message_create", &out), &ch));
    if ch.kind != "dm" {
        crate::ai::submit(&auth.srv, crate::ai::Job { message_id: id, channel_id: ch.id, author_id: auth.user.id, content: content.clone() }, auth.is_owner);
    }
    Ok((StatusCode::CREATED, Json(out)))
}

#[derive(Deserialize)]
pub struct EditReq {
    content: String,
}

pub async fn edit(auth: Auth, Path(p): Path<MsgPath>, Body(req): Body<EditReq>) -> ApiResult<Json<MessageOut>> {
    let ch = text_channel(&auth, &p.id).await?;
    let msg = message(&auth, ch.id, &p.mid).await?;
    if msg.author_id != auth.user.id {
        return Err(ApiError::forbidden("forbidden", "You can only edit your own messages."));
    }
    super::moderation::check_not_timed_out(&auth.user)?;
    super::moderation::check_new_account(&auth, super::moderation::has_link(&req.content)).await?;
    let has_files: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM attachments WHERE message_id = ?").bind(msg.id).fetch_one(&auth.srv.db).await?;
    let content = if has_files > 0 && req.content.trim().is_empty() {
        String::new()
    } else {
        check_content(&req.content).map_err(|e| ApiError::validation(BTreeMap::from([("content", e)])))?
    };
    sqlx::query("UPDATE messages SET content = ?, edited_at = ? WHERE id = ?").bind(&content).bind(now_ms()).bind(msg.id).execute(&auth.srv.db).await?;
    store_mentions(&auth, &ch, msg.id, &content).await?;
    let row = db::message_row(&auth.srv.db, ch.id, msg.id).await?.ok_or_else(ApiError::internal)?;
    let broadcast = db::messages_out(&auth.srv, std::slice::from_ref(&row), 0).await?.remove(0);
    auth.srv.emit(scoped(Event::new("message_update", &broadcast), &ch));
    if ch.kind != "dm" {
        crate::ai::submit(&auth.srv, crate::ai::Job { message_id: msg.id, channel_id: ch.id, author_id: auth.user.id, content: content.clone() }, auth.is_owner);
    }
    Ok(Json(db::messages_out(&auth.srv, &[row], auth.user.id).await?.remove(0)))
}

pub async fn delete(auth: Auth, Path(p): Path<MsgPath>) -> ApiResult<StatusCode> {
    let ch = text_channel(&auth, &p.id).await?;
    let msg = message(&auth, ch.id, &p.mid).await?;
    if msg.author_id != auth.user.id {
        // In a direct message only the author can delete.
        if ch.kind == "dm" {
            return Err(ApiError::forbidden("forbidden", "You can only delete your own direct messages."));
        }
        auth.require(MANAGE_MESSAGES)?;
    }
    sqlx::query("DELETE FROM messages WHERE id = ?").bind(msg.id).execute(&auth.srv.db).await?;
    if msg.author_id != auth.user.id {
        let preview: String = msg.content.chars().take(200).collect();
        super::moderation::audit(&auth.srv, Some(auth.user.id), "message_delete", Some(msg.author_id), json!({ "channel_id": ch.id.to_string(), "content": preview })).await;
    }
    auth.srv.emit(scoped(Event::new("message_delete", json!({ "id": msg.id.to_string(), "channel_id": ch.id.to_string() })), &ch));
    Ok(StatusCode::NO_CONTENT)
}

pub async fn react(auth: Auth, Path(p): Path<ReactPath>) -> ApiResult<StatusCode> {
    let ch = text_channel(&auth, &p.id).await?;
    auth.require(ADD_REACTIONS)?;
    super::moderation::check_not_timed_out(&auth.user)?;
    check_emoji(&p.emoji)?;
    let msg = message(&auth, ch.id, &p.mid).await?;
    let distinct: i64 =
        sqlx::query_scalar("SELECT COUNT(DISTINCT emoji) FROM reactions WHERE message_id = ? AND emoji != ?").bind(msg.id).bind(&p.emoji).fetch_one(&auth.srv.db).await?;
    if distinct >= 20 {
        return Err(ApiError::bad_request("This message already has the maximum number of different reactions."));
    }
    let added = sqlx::query("INSERT OR IGNORE INTO reactions (message_id, user_id, emoji, created_at) VALUES (?, ?, ?, ?)")
        .bind(msg.id)
        .bind(auth.user.id)
        .bind(&p.emoji)
        .bind(now_ms())
        .execute(&auth.srv.db)
        .await?;
    if added.rows_affected() > 0 {
        auth.srv.emit(reaction_event("reaction_add", &auth, &ch, msg.id, &p.emoji));
    }
    Ok(StatusCode::NO_CONTENT)
}

pub async fn unreact(auth: Auth, Path(p): Path<ReactPath>) -> ApiResult<StatusCode> {
    let ch = text_channel(&auth, &p.id).await?;
    let msg = message(&auth, ch.id, &p.mid).await?;
    let removed =
        sqlx::query("DELETE FROM reactions WHERE message_id = ? AND user_id = ? AND emoji = ?").bind(msg.id).bind(auth.user.id).bind(&p.emoji).execute(&auth.srv.db).await?;
    if removed.rows_affected() > 0 {
        auth.srv.emit(reaction_event("reaction_remove", &auth, &ch, msg.id, &p.emoji));
    }
    Ok(StatusCode::NO_CONTENT)
}

fn reaction_event(ty: &str, auth: &Auth, ch: &ChannelRow, message_id: i64, emoji: &str) -> Event {
    let data = json!({
        "message_id": message_id.to_string(),
        "channel_id": ch.id.to_string(),
        "emoji": emoji,
        "user_id": auth.user.id.to_string(),
    });
    scoped(Event::new(ty, data), ch)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_limits() {
        assert_eq!(check_content("  hi "), Ok("hi".into()));
        assert_eq!(check_content("   "), Err("empty"));
        assert_eq!(check_content(&"a".repeat(4001)), Err("too_long"));
        assert_eq!(check_content(&"a\n".repeat(61)), Err("too_many_lines"));
        assert!(check_content(&"a\n".repeat(60)).is_ok());
    }

    #[test]
    fn mentions() {
        let m = parse_mentions("hi <@12> and <@&34>, <@12> again, <@x> <@5 <@&> @everyone");
        assert_eq!(m, Mentions { users: vec![12], roles: vec![34], everyone: 1 });
        assert_eq!(parse_mentions("mail me at a@here.com or @everyones").everyone, 0);
        assert_eq!(parse_mentions("(@here)").everyone, 2);
        assert_eq!(parse_mentions(&"<@1>".repeat(80)).users, vec![1]);
        let many: String = (1..80).map(|i| format!("<@{i}>")).collect();
        assert_eq!(parse_mentions(&many).users.len(), MAX_MENTIONS);
    }
}
