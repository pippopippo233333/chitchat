//! Direct messages between two members of the same server. A DM is a channel of type `dm`
//! that only its two people can see; messages, reactions, files and typing use the normal
//! channel endpoints. Not end-to-end encrypted: whoever runs the host can read them.

use std::collections::BTreeMap;

use axum::Json;
use axum::http::StatusCode;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{ApiError, ApiResult, Auth, Body};
use crate::events::Event;
use crate::ids::next_id;
use crate::perms::SEND_DMS;

const SELECT_DMS: &str = "SELECT c.id, u.id, u.username, u.display_name, u.avatar_url, (SELECT MAX(id) FROM messages WHERE channel_id = c.id) AS last \
     FROM channels c JOIN users u ON u.id = CASE WHEN c.dm_a = ? THEN c.dm_b ELSE c.dm_a END \
     WHERE c.type = 'dm' AND ? IN (c.dm_a, c.dm_b)";

type DmRow = (i64, i64, String, String, Option<String>, Option<i64>);

fn dm_out(r: &DmRow, my_permissions: u64) -> Value {
    let (id, uid, username, display_name, avatar_url, last) = r;
    json!({
        "id": id.to_string(),
        "type": "dm",
        "user": { "id": uid.to_string(), "username": username, "display_name": display_name, "avatar_url": avatar_url },
        "last_message_id": last.map(|v| v.to_string()),
        "media_allowed": true,
        "my_permissions": my_permissions.to_string(),
    })
}

/// `GET /dms`: the caller's direct messages, most recently active first.
pub async fn list(auth: Auth) -> ApiResult<Json<Vec<Value>>> {
    let rows: Vec<DmRow> = sqlx::query_as(&format!("{SELECT_DMS} ORDER BY COALESCE(last, c.id) DESC")).bind(auth.user.id).bind(auth.user.id).fetch_all(&auth.srv.db).await?;
    Ok(Json(rows.iter().map(|r| dm_out(r, auth.perms)).collect()))
}

#[derive(Deserialize)]
pub struct OpenReq {
    user_id: String,
}

/// `POST /dms {user_id}`: opens (or returns) the direct message with that member.
pub async fn open(auth: Auth, Body(req): Body<OpenReq>) -> ApiResult<(StatusCode, Json<Value>)> {
    auth.require(SEND_DMS)?;
    let bad = |code: &'static str| ApiError::validation(BTreeMap::from([("user_id", code)]));
    let other = crate::ids::parse(&req.user_id).ok_or_else(|| bad("unknown_user"))?;
    if other == auth.user.id {
        return Err(bad("self"));
    }
    let allow: Option<bool> = sqlx::query_scalar("SELECT allow_dms FROM users WHERE id = ?").bind(other).fetch_optional(&auth.srv.db).await?;
    let Some(allow) = allow else { return Err(bad("unknown_user")) };
    if super::moderation::user_banned(&auth.srv, other).await? {
        return Err(bad("unknown_user"));
    }
    let (a, b) = (auth.user.id.min(other), auth.user.id.max(other));
    let existing: Option<i64> = sqlx::query_scalar("SELECT id FROM channels WHERE type = 'dm' AND dm_a = ? AND dm_b = ?").bind(a).bind(b).fetch_optional(&auth.srv.db).await?;
    let status = match existing {
        Some(_) => StatusCode::OK,
        None => {
            if !allow {
                return Err(ApiError::forbidden("dms_closed", "This person doesn't accept direct messages."));
            }
            // Two people opening the same DM at once: the unique index keeps one.
            sqlx::query("INSERT OR IGNORE INTO channels (id, type, name, topic, position, media_allowed, user_limit, dm_a, dm_b) VALUES (?, 'dm', '', '', 0, 1, 0, ?, ?)")
                .bind(next_id())
                .bind(a)
                .bind(b)
                .execute(&auth.srv.db)
                .await?;
            StatusCode::CREATED
        }
    };
    let row = |viewer: i64| {
        let db = auth.srv.db.clone();
        async move { sqlx::query_as::<_, DmRow>(&format!("{SELECT_DMS} AND c.dm_a = ? AND c.dm_b = ?")).bind(viewer).bind(viewer).bind(a).bind(b).fetch_one(&db).await }
    };
    let mine = row(auth.user.id).await?;
    if status == StatusCode::CREATED {
        // Each side sees the other person; permissions are filled in by each client's snapshot.
        let (theirs_perms, _) = crate::db::user_perms(&auth.srv.db, other).await?;
        auth.srv.emit(Event::new("dm_create", dm_out(&mine, auth.perms)).for_users(vec![auth.user.id]));
        auth.srv.emit(Event::new("dm_create", dm_out(&row(other).await?, theirs_perms)).for_users(vec![other]));
    }
    Ok((status, Json(dm_out(&mine, auth.perms))))
}
