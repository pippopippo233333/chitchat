//! Server-owned emoji images, re-encoded and kept with the database backups.

use axum::Json;
use axum::body::to_bytes;
use axum::extract::{Path, Query, Request};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::extract::parse_id;
use super::{ApiError, ApiResult, Auth, Srv};
use crate::events::Event;
use crate::host::ServerCtx;
use crate::models::now_ms;
use crate::perms::MANAGE_SERVER;

const MAX_BYTES: usize = 256 * 1024;

#[derive(Serialize)]
pub struct EmojiOut {
    pub id: String,
    pub name: String,
    pub url: String,
}

fn out(srv: &ServerCtx, id: i64, name: String, hash: String) -> EmojiOut {
    EmojiOut { id: id.to_string(), name, url: format!("/s/{}/api/emojis/{id}/{hash}.webp", srv.slug) }
}

pub(super) async fn list_for(srv: &ServerCtx) -> sqlx::Result<Vec<EmojiOut>> {
    let rows: Vec<(i64, String, String)> = sqlx::query_as("SELECT id, name, hash FROM custom_emojis ORDER BY name, id").fetch_all(&srv.db).await?;
    Ok(rows.into_iter().map(|(id, name, hash)| out(srv, id, name, hash)).collect())
}

pub async fn list(auth: Auth) -> ApiResult<Json<Vec<EmojiOut>>> {
    Ok(Json(list_for(&auth.srv).await?))
}

#[derive(Deserialize)]
pub struct CreateQuery {
    name: String,
}

pub async fn create(auth: Auth, Query(q): Query<CreateQuery>, req: Request) -> ApiResult<(StatusCode, Json<EmojiOut>)> {
    auth.require(MANAGE_SERVER)?;
    super::moderation::check_not_timed_out(&auth.user)?;
    let name = q.name.trim().to_ascii_lowercase();
    if !(2..=32).contains(&name.len()) || !name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_') {
        return Err(ApiError::bad_request("Emoji names need 2-32 letters, numbers or underscores."));
    }
    let bytes = to_bytes(req.into_body(), MAX_BYTES).await.map_err(|_| ApiError::new(StatusCode::PAYLOAD_TOO_LARGE, "too_large", "Emoji images can be at most 256 KB."))?;
    let image = tokio::task::spawn_blocking(move || crate::media::process_emoji(&bytes))
        .await
        .map_err(|_| ApiError::internal())?
        .map_err(|_| ApiError::new(StatusCode::UNSUPPORTED_MEDIA_TYPE, "unsupported_media", "Choose a valid PNG, JPEG, GIF or WebP image, at most 4096 pixels per side."))?;
    let mut room = super::storage::make_room(&auth.srv, image.len() as i64).await?;
    let hash = blake3::hash(&image).to_hex().to_string();
    let id = crate::ids::next_id();
    let result = sqlx::query("INSERT INTO custom_emojis (id, name, hash, image, created_by, created_at) SELECT ?, ?, ?, ?, ?, ? WHERE (SELECT COUNT(*) FROM custom_emojis) < 100")
        .bind(id)
        .bind(&name)
        .bind(&hash)
        .bind(image)
        .bind(auth.user.id)
        .bind(now_ms())
        .execute(&auth.srv.db)
        .await;
    *room = None;
    drop(room);
    let inserted = match result {
        Err(e) if e.as_database_error().is_some_and(|e| e.is_unique_violation()) => {
            return Err(ApiError::new(StatusCode::CONFLICT, "emoji_name_taken", "That emoji name is already used."));
        }
        r => r?,
    };
    if inserted.rows_affected() == 0 {
        return Err(ApiError::new(StatusCode::CONFLICT, "too_many_emojis", "A server can have at most 100 custom emoji."));
    }
    let emoji = out(&auth.srv, id, name, hash);
    auth.srv.emit(Event::new("emoji_create", &emoji));
    super::moderation::audit(&auth.srv, Some(auth.user.id), "emoji_create", Some(id), json!({"name":emoji.name})).await;
    Ok((StatusCode::CREATED, Json(emoji)))
}

#[derive(Deserialize)]
pub struct IdPath {
    id: String,
}

pub async fn delete(auth: Auth, Path(p): Path<IdPath>) -> ApiResult<StatusCode> {
    auth.require(MANAGE_SERVER)?;
    let id = parse_id(&p.id, "not_found")?;
    let mut room = auth.srv.storage_used.lock().await;
    let deleted = sqlx::query("DELETE FROM custom_emojis WHERE id = ?").bind(id).execute(&auth.srv.db).await?;
    *room = None;
    drop(room);
    if deleted.rows_affected() == 0 {
        return Err(ApiError::not_found("not_found", "No such emoji."));
    }
    auth.srv.emit(Event::new("emoji_delete", json!({"id":id.to_string()})));
    super::moderation::audit(&auth.srv, Some(auth.user.id), "emoji_delete", Some(id), json!({})).await;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct ImagePath {
    id: String,
    name: String,
}

pub async fn image(Srv(srv): Srv, Path(p): Path<ImagePath>) -> ApiResult<Response> {
    let id = parse_id(&p.id, "not_found")?;
    let hash =
        p.name.strip_suffix(".webp").filter(|h| h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit())).ok_or_else(|| ApiError::not_found("not_found", "Emoji not found."))?;
    let image: Option<Vec<u8>> = sqlx::query_scalar("SELECT image FROM custom_emojis WHERE id = ? AND hash = ?").bind(id).bind(hash).fetch_optional(&srv.db).await?;
    let image = image.ok_or_else(|| ApiError::not_found("not_found", "Emoji not found."))?;
    Ok(([(header::CONTENT_TYPE, "image/webp"), (header::CACHE_CONTROL, "public, max-age=86400, immutable"), (header::X_CONTENT_TYPE_OPTIONS, "nosniff")], image).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::Host;
    use crate::{db, perms};
    use axum::body::Body as RawBody;
    use std::io::Cursor;

    #[tokio::test]
    async fn custom_emoji_permissions_content_validation_and_removal() {
        let dir = std::env::temp_dir().join(format!("chitchat-emoji-{}", crate::ids::next_id()));
        let host = Host::load(&dir).await.unwrap();
        host.create_server("emoji", "Emoji").await.unwrap();
        let srv = host.get("emoji").await.unwrap();
        let uid = crate::ids::next_id();
        sqlx::query("INSERT INTO users (id, username, display_name, password_hash, created_at) VALUES (?, 'emoji', 'Emoji', 'unused', ?)")
            .bind(uid)
            .bind(now_ms())
            .execute(&srv.db)
            .await
            .unwrap();
        let auth = |manager: bool| {
            let srv = srv.clone();
            async move {
                Auth {
                    user: db::user_by_id(&srv.db, uid).await.unwrap().unwrap(),
                    srv,
                    perms: if manager { perms::ALL } else { perms::DEFAULT_EVERYONE },
                    is_owner: manager,
                    token_hash: "test".into(),
                }
            }
        };
        assert!(create(auth(false).await, Query(CreateQuery { name: "party".into() }), Request::new(RawBody::empty())).await.is_err());
        assert!(create(auth(true).await, Query(CreateQuery { name: "../bad".into() }), Request::new(RawBody::empty())).await.is_err());
        assert!(create(auth(true).await, Query(CreateQuery { name: "party".into() }), Request::new(RawBody::from("<script>bad</script>"))).await.is_err());
        let mut png = Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(256, 128).write_to(&mut png, image::ImageFormat::Png).unwrap();
        let bytes = png.into_inner();
        let (_, Json(emoji)) = create(auth(true).await, Query(CreateQuery { name: "Party".into() }), Request::new(RawBody::from(bytes.clone()))).await.unwrap();
        assert_eq!(emoji.name, "party");
        assert_eq!(list_for(&srv).await.unwrap().len(), 1);
        let duplicate = create(auth(true).await, Query(CreateQuery { name: "party".into() }), Request::new(RawBody::from(bytes))).await.err().unwrap();
        assert_eq!(duplicate.code, "emoji_name_taken");
        let response = image(Srv(srv.clone()), Path(ImagePath { id: emoji.id.clone(), name: emoji.url.rsplit('/').next().unwrap().into() })).await.unwrap();
        let saved = to_bytes(response.into_body(), MAX_BYTES).await.unwrap();
        let decoded = image::load_from_memory(&saved).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (128, 64));
        assert_eq!(super::super::files::server_usage(&srv).await.unwrap(), saved.len() as i64);
        delete(auth(true).await, Path(IdPath { id: emoji.id.clone() })).await.unwrap();
        assert!(list_for(&srv).await.unwrap().is_empty());
        assert!(image(Srv(srv.clone()), Path(ImagePath { id: emoji.id, name: emoji.url.rsplit('/').next().unwrap().into() })).await.is_err());
        assert_eq!(super::super::files::server_usage(&srv).await.unwrap(), 0);
        srv.db.close().await;
        drop(srv);
        drop(host);
        assert!(dir.canonicalize().unwrap().starts_with(std::env::temp_dir().canonicalize().unwrap()));
        for attempt in 0..20 {
            match std::fs::remove_dir_all(&dir) {
                Ok(()) => break,
                Err(e) if attempt < 19 && matches!(e.raw_os_error(), Some(32 | 33)) => tokio::time::sleep(std::time::Duration::from_millis(50)).await,
                Err(e) => panic!("remove temporary emoji database: {e}"),
            }
        }
    }
}
