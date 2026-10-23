//! Per-server profile pictures and presence. Pictures are bounded, re-encoded still images.

use axum::Json;
use axum::body::to_bytes;
use axum::extract::{Path, Request};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::json;

use super::extract::parse_id;
use super::{ApiError, ApiResult, Auth, Srv};
use crate::db;
use crate::events::Event;
use crate::host::ServerCtx;
use crate::models::{UserOut, UserRow};

const MAX_AVATAR_BYTES: usize = 2 * 1024 * 1024;

pub fn presence_event(srv: &ServerCtx, user: &UserRow) -> Event {
    let online = srv.is_online(user.id);
    Event::new(
        "presence_update",
        json!({
            "user_id": user.id.to_string(), "online": online,
            "status": if online { user.status.as_str() } else { "offline" },
        }),
    )
}

async fn updated(auth: &Auth) -> ApiResult<Json<UserOut>> {
    let user = db::user_by_id(&auth.srv.db, auth.user.id).await?.ok_or_else(ApiError::internal)?;
    if let Some(member) = db::member(&auth.srv, user.id).await? {
        auth.srv.emit(Event::new("member_update", member));
    }
    Ok(Json(UserOut::from(&user)))
}

pub async fn upload_avatar(auth: Auth, req: Request) -> ApiResult<Json<UserOut>> {
    super::moderation::check_not_timed_out(&auth.user)?;
    let bytes =
        to_bytes(req.into_body(), MAX_AVATAR_BYTES).await.map_err(|_| ApiError::new(StatusCode::PAYLOAD_TOO_LARGE, "too_large", "Profile pictures can be at most 2 MB."))?;
    let image = tokio::task::spawn_blocking(move || crate::media::process_avatar(&bytes))
        .await
        .map_err(|_| ApiError::internal())?
        .map_err(|_| ApiError::new(StatusCode::UNSUPPORTED_MEDIA_TYPE, "unsupported_media", "Choose a valid PNG, JPEG, GIF or WebP picture, at most 4096 pixels wide or tall."))?;
    // Serialize replacements with all other storage changes; only the increase needs room.
    let mut room = super::storage::make_avatar_room(&auth.srv, auth.user.id, image.len() as i64).await?;
    let hash = blake3::hash(&image).to_hex().to_string();
    let url = format!("/s/{}/api/avatars/{}/{hash}.webp", auth.srv.slug, auth.user.id);
    let mut tx = auth.srv.db.begin().await?;
    sqlx::query("INSERT INTO user_avatars (user_id, hash, image) VALUES (?, ?, ?) ON CONFLICT(user_id) DO UPDATE SET hash = excluded.hash, image = excluded.image")
        .bind(auth.user.id)
        .bind(hash)
        .bind(image)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE users SET avatar_url = ? WHERE id = ?").bind(url).bind(auth.user.id).execute(&mut *tx).await?;
    tx.commit().await?;
    *room = None;
    drop(room);
    updated(&auth).await
}

pub async fn delete_avatar(auth: Auth) -> ApiResult<Json<UserOut>> {
    let mut room = auth.srv.storage_used.lock().await;
    let mut tx = auth.srv.db.begin().await?;
    sqlx::query("DELETE FROM user_avatars WHERE user_id = ?").bind(auth.user.id).execute(&mut *tx).await?;
    sqlx::query("UPDATE users SET avatar_url = NULL WHERE id = ?").bind(auth.user.id).execute(&mut *tx).await?;
    tx.commit().await?;
    *room = None;
    drop(room);
    updated(&auth).await
}

#[derive(Deserialize)]
pub struct AvatarPath {
    user_id: String,
    name: String,
}

/// The opaque content URL can be used directly by img across hosts, without a bearer token.
pub async fn avatar(Srv(srv): Srv, Path(p): Path<AvatarPath>) -> ApiResult<Response> {
    let uid = parse_id(&p.user_id, "not_found")?;
    let hash =
        p.name.strip_suffix(".webp").filter(|h| h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit())).ok_or_else(|| ApiError::not_found("not_found", "Picture not found."))?;
    let image: Option<Vec<u8>> = sqlx::query_scalar("SELECT image FROM user_avatars WHERE user_id = ? AND hash = ?").bind(uid).bind(hash).fetch_optional(&srv.db).await?;
    let image = image.ok_or_else(|| ApiError::not_found("not_found", "Picture not found."))?;
    Ok(([(header::CONTENT_TYPE, "image/webp"), (header::CACHE_CONTROL, "public, max-age=86400, immutable"), (header::X_CONTENT_TYPE_OPTIONS, "nosniff")], image).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::Host;
    use crate::models::now_ms;
    use axum::body::Body as RawBody;
    use std::io::Cursor;

    #[tokio::test]
    async fn profiles_replace_bounded_pictures_and_keep_away_across_connections() {
        let dir = std::env::temp_dir().join(format!("chitchat-profile-{}", crate::ids::next_id()));
        let host = Host::load(&dir).await.unwrap();
        host.create_server("profiles", "Profiles").await.unwrap();
        let srv = host.get("profiles").await.unwrap();
        let uid = crate::ids::next_id();
        sqlx::query("INSERT INTO users (id, username, display_name, password_hash, created_at) VALUES (?, 'profile', 'Profile', 'unused', ?)")
            .bind(uid)
            .bind(now_ms())
            .execute(&srv.db)
            .await
            .unwrap();
        let auth = || async { Auth { srv: srv.clone(), user: db::user_by_id(&srv.db, uid).await.unwrap().unwrap(), perms: 0, is_owner: false, token_hash: "test".into() } };
        let req = serde_json::from_value(json!({"status":"away", "status_text":"Building things"})).unwrap();
        let user = super::super::auth::patch_me(auth().await, super::super::Body(req)).await.unwrap().0;
        assert_eq!(user.status, "away");
        assert_eq!(user.status_text, "Building things");
        assert_eq!(db::member(&srv, uid).await.unwrap().unwrap().status, "offline");
        assert!(srv.connect(uid));
        assert!(!srv.connect(uid));
        assert_eq!(db::member(&srv, uid).await.unwrap().unwrap().status, "away");
        assert!(!srv.disconnect(uid));
        assert_eq!(db::member(&srv, uid).await.unwrap().unwrap().status, "away");
        assert!(srv.disconnect(uid));
        assert_eq!(db::member(&srv, uid).await.unwrap().unwrap().status, "offline");
        for invalid in [json!({"status":"invisible"}), json!({"status_text":"bad\nstatus"}), json!({"status_text":"x".repeat(129)})] {
            let req = serde_json::from_value(invalid).unwrap();
            assert!(super::super::auth::patch_me(auth().await, super::super::Body(req)).await.is_err());
        }
        let mut png = Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(600, 300).write_to(&mut png, image::ImageFormat::Png).unwrap();
        let first = upload_avatar(auth().await, Request::new(RawBody::from(png.into_inner()))).await.unwrap().0;
        let url = first.avatar_url.unwrap();
        let name = url.rsplit('/').next().unwrap().to_string();
        let response = avatar(Srv(srv.clone()), Path(AvatarPath { user_id: uid.to_string(), name: name.clone() })).await.unwrap();
        let image = to_bytes(response.into_body(), MAX_AVATAR_BYTES).await.unwrap();
        let decoded = image::load_from_memory(&image).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (256, 128));
        assert_eq!(super::super::files::server_usage(&srv).await.unwrap(), image.len() as i64);
        assert!(upload_avatar(auth().await, Request::new(RawBody::from("<svg onload='bad'>"))).await.is_err());
        assert!(upload_avatar(auth().await, Request::new(RawBody::from(vec![0; MAX_AVATAR_BYTES + 1]))).await.is_err());
        assert!(delete_avatar(auth().await).await.unwrap().0.avatar_url.is_none());
        assert!(avatar(Srv(srv.clone()), Path(AvatarPath { user_id: uid.to_string(), name })).await.is_err());
        assert_eq!(super::super::files::server_usage(&srv).await.unwrap(), 0);
        srv.db.close().await;
        drop(srv);
        drop(host);
        assert!(dir.canonicalize().unwrap().starts_with(std::env::temp_dir().canonicalize().unwrap()));
        // SQLite's worker releases its last Windows handle just after close completes.
        for attempt in 0..20 {
            match std::fs::remove_dir_all(&dir) {
                Ok(()) => break,
                Err(e) if attempt < 19 && matches!(e.raw_os_error(), Some(32 | 33)) => {
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                }
                Err(e) => panic!("remove temporary profile database: {e}"),
            }
        }
    }
}
