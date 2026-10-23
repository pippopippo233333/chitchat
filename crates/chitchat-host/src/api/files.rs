//! Uploading, serving and cleaning up images, videos and documents (Phase 3).

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use axum::Json;
use axum::body::{Body as RawBody, BodyDataStream};
use axum::extract::{Path, Query, Request};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::{Value, json};
use tower::ServiceExt;
use tower_http::services::ServeFile;

use super::extract::parse_id;
use super::{ApiError, ApiResult, Auth, Srv};
use crate::db;
use crate::host::ServerCtx;
use crate::ids::next_id;
use crate::media::{self, MediaError, Sniffed};
use crate::models::{AttachmentOut, AttachmentRow, now_ms};
use crate::perms::ATTACH_FILES;

const MB: i64 = 1024 * 1024;
const DAY_MS: i64 = 86_400_000;
/// Uploads per user per minute.
const UPLOADS_PER_MINUTE: i64 = 10;
/// Attachments per message.
pub const MAX_PER_MESSAGE: usize = 10;
/// Uploaded but never sent: removed after this long.
const PENDING_TTL_MS: i64 = 60 * 60 * 1000;

fn too_large(code: &'static str, message: String, limit_mb: i64) -> ApiError {
    ApiError::new(StatusCode::PAYLOAD_TOO_LARGE, code, message).with("limit_mb", json!(limit_mb))
}

/// Bytes a user has uploaded (stored size) since `since_ms`.
async fn user_usage(srv: &ServerCtx, user_id: i64, since_ms: i64) -> sqlx::Result<i64> {
    sqlx::query_scalar("SELECT COALESCE(SUM(size), 0) FROM attachments WHERE uploader_id = ? AND created_at >= ?").bind(user_id).bind(since_ms).fetch_one(&srv.db).await
}

/// Bytes on disk for the whole server (each identical file counted once).
pub async fn server_usage(srv: &ServerCtx) -> sqlx::Result<i64> {
    sqlx::query_scalar("SELECT (SELECT COALESCE(SUM(size), 0) FROM blobs) + (SELECT COALESCE(SUM(LENGTH(image)), 0) FROM user_avatars) + (SELECT COALESCE(SUM(LENGTH(image)), 0) FROM custom_emojis)").fetch_one(&srv.db).await
}

#[derive(Deserialize)]
pub struct ChanPath {
    id: String,
}

#[derive(Deserialize)]
pub struct UploadQuery {
    #[serde(default)]
    filename: String,
    /// A voice message recorded in the app (`voice=1`).
    #[serde(default, deserialize_with = "flag")]
    voice: bool,
}

fn flag<'de, D: serde::Deserializer<'de>>(d: D) -> Result<bool, D::Error> {
    let s = <String as serde::Deserialize>::deserialize(d)?;
    Ok(matches!(s.as_str(), "1" | "true"))
}

/// The longest voice message, when its length can be measured.
const MAX_VOICE_MS: i64 = 5 * 60 * 1000;

/// Refused uploads still read (and throw away) up to this much of the body, so the browser gets the
/// error message instead of a "connection reset".
const DRAIN_LIMIT: i64 = 100 * MB;

/// `POST /channels/{id}/attachments?filename=cat.jpg` with the raw file as the body.
pub async fn upload(auth: Auth, Path(p): Path<ChanPath>, Query(q): Query<UploadQuery>, req: Request) -> ApiResult<(StatusCode, Json<AttachmentOut>)> {
    let declared = req.headers().get(header::CONTENT_LENGTH).and_then(|v| v.to_str().ok()).and_then(|v| v.parse::<i64>().ok());
    let mut body = req.into_body().into_data_stream();
    let result = receive(&auth, &p, &q, &mut body, declared).await;
    if result.is_err() && declared.is_none_or(|n| n <= DRAIN_LIMIT) {
        let mut seen = 0i64;
        while let Some(Ok(chunk)) = body.next().await {
            seen += chunk.len() as i64;
            if seen > DRAIN_LIMIT {
                break;
            }
        }
    }
    result
}

async fn receive(auth: &Auth, p: &ChanPath, q: &UploadQuery, stream: &mut BodyDataStream, declared: Option<i64>) -> ApiResult<(StatusCode, Json<AttachmentOut>)> {
    // A text channel or one of the caller's direct messages (always open to files).
    let ch = super::messages::text_channel(auth, &p.id).await?;
    super::conversations::check_open(&ch)?;
    if ch.kind == "dm" {
        auth.require(crate::perms::SEND_DMS)?;
    }
    if !ch.media_allowed {
        return Err(ApiError::forbidden("media_not_allowed", "Images and videos aren't allowed in this channel."));
    }
    auth.require_in(&ch, ATTACH_FILES).await?;
    if ch.kind != "dm" {
        auth.require_in(&ch, crate::perms::SEND_MESSAGES).await?;
    }
    super::moderation::check_not_timed_out(&auth.user)?;
    super::moderation::check_new_account(auth, true).await?;
    let srv = &auth.srv;
    let settings = db::server_row(&srv.db).await?;
    let now = now_ms();

    if !auth.is_owner {
        let recent: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM attachments WHERE uploader_id = ? AND created_at >= ?").bind(auth.user.id).bind(now - 60_000).fetch_one(&srv.db).await?;
        if recent >= UPLOADS_PER_MINUTE {
            return Err(ApiError::new(StatusCode::TOO_MANY_REQUESTS, "rate_limited", "Slow down: too many uploads.").with("retry_after_ms", json!(10_000)));
        }
    }

    // Read the body, refusing as soon as it passes the per-file limit.
    let max = settings.max_file_mb * MB;
    if declared.is_some_and(|n| n > max) {
        return Err(too_large("too_large", format!("Files can be at most {} MB.", settings.max_file_mb), settings.max_file_mb));
    }
    let mut bytes = Vec::with_capacity(declared.unwrap_or(0).clamp(0, max) as usize);
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| ApiError::bad_request("The upload was interrupted."))?;
        if (bytes.len() + chunk.len()) as i64 > max {
            return Err(too_large("too_large", format!("Files can be at most {} MB.", settings.max_file_mb), settings.max_file_mb));
        }
        bytes.extend_from_slice(&chunk);
    }
    if bytes.is_empty() {
        return Err(ApiError::bad_request("The file is empty."));
    }

    let unsupported = || {
        ApiError::new(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "unsupported_media",
            "Only images (PNG, JPEG, GIF, WebP), videos (MP4, WebM), audio (MP3, OGG, WAV, FLAC, M4A) and documents (TXT, MD, CSV, LOG, JSON as UTF-8 text, and PDF) can be uploaded.",
        )
    };
    if q.voice && !settings.voice_messages {
        return Err(ApiError::forbidden("voice_messages_off", "Voice messages are turned off on this server."));
    }
    let tmp = srv.files_dir.join("tmp");
    let broken_audio = || ApiError::bad_request("That audio file couldn't be read. It may be damaged.");
    let kind = match media::sniff(&bytes) {
        Some(k) => Some(k),
        None if media::document(&q.filename, &bytes).is_some() => None,
        None => return Err(unsupported()),
    };
    let processed = match kind {
        None => {
            let (mime, _) = media::document(&q.filename, &bytes).ok_or_else(unsupported)?;
            media::Processed { bytes, mime, kind: "file", width: None, height: None, thumb: None, duration_ms: None }
        }
        Some(kind @ (Sniffed::Mp4 | Sniffed::Webm)) => media::process_av(bytes, kind, &tmp, q.voice).await.map_err(|_| broken_audio())?,
        Some(kind) if media::is_audio(kind) => media::process_audio(bytes, kind, &tmp, q.voice).await.map_err(|_| broken_audio())?,
        Some(kind) => tokio::task::spawn_blocking(move || media::process_image(bytes, kind)).await.map_err(|_| ApiError::internal())?.map_err(|e| match e {
            MediaError::Broken => ApiError::bad_request("That image couldn't be read. It may be damaged."),
            MediaError::Unsupported => ApiError::new(StatusCode::UNSUPPORTED_MEDIA_TYPE, "unsupported_media", "Unsupported file."),
        })?,
    };
    if q.voice {
        if processed.kind != "audio" {
            return Err(ApiError::new(StatusCode::UNSUPPORTED_MEDIA_TYPE, "unsupported_media", "A voice message must be audio."));
        }
        if processed.duration_ms.is_some_and(|d| d > MAX_VOICE_MS) {
            return Err(ApiError::new(StatusCode::BAD_REQUEST, "voice_too_long", "Voice messages can be at most 5 minutes long.").with("max_ms", json!(MAX_VOICE_MS)));
        }
    }
    let size = processed.bytes.len() as i64;

    // Quotas are checked on the size actually stored (after compression). The owner is exempt from per-user limits.
    if !auth.is_owner {
        if user_usage(srv, auth.user.id, now - DAY_MS).await? + size > settings.user_daily_mb * MB {
            return Err(too_large("quota_exceeded", format!("You can upload {} MB per day. Try again tomorrow.", settings.user_daily_mb), settings.user_daily_mb)
                .with("quota", json!("daily")));
        }
        if user_usage(srv, auth.user.id, 0).await? + size > settings.user_total_mb * MB {
            return Err(too_large("quota_exceeded", format!("You've reached your {} MB storage limit on this server.", settings.user_total_mb), settings.user_total_mb)
                .with("quota", json!("total")));
        }
    }
    let _room = super::storage::make_room(srv, size).await?;

    let blob_hash = media::store(&srv.files_dir, &processed.bytes).await.map_err(|e| anyhow::anyhow!(e))?;
    let thumb_hash = match &processed.thumb {
        Some(t) => Some(media::store(&srv.files_dir, t).await.map_err(|e| anyhow::anyhow!(e))?),
        None => None,
    };
    for (hash, len) in [(Some(&blob_hash), size), (thumb_hash.as_ref(), processed.thumb.as_ref().map_or(0, |t| t.len() as i64))] {
        if let Some(h) = hash {
            sqlx::query("INSERT OR IGNORE INTO blobs (hash, size, created_at) VALUES (?, ?, ?)").bind(h).bind(len).bind(now).execute(&srv.db).await?;
        }
    }

    let row = AttachmentRow {
        id: next_id(),
        channel_id: ch.id,
        message_id: None,
        uploader_id: auth.user.id,
        blob_hash,
        thumb_hash,
        filename: media::clean_filename(
            &q.filename,
            match media::document(&q.filename, &processed.bytes) {
                Some((_, ext)) if processed.kind == "file" => ext,
                _ => media::ext_for(processed.mime),
            },
        ),
        kind: processed.kind.to_string(),
        mime: processed.mime.to_string(),
        size,
        width: processed.width.map(i64::from),
        height: processed.height.map(i64::from),
        created_at: now,
        duration_ms: processed.duration_ms,
        voice: q.voice,
    };
    sqlx::query(
        "INSERT INTO attachments (id, channel_id, message_id, uploader_id, blob_hash, thumb_hash, filename, kind, mime, size, width, height, created_at, duration_ms, voice) \
         VALUES (?, ?, NULL, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(row.id)
    .bind(row.channel_id)
    .bind(row.uploader_id)
    .bind(&row.blob_hash)
    .bind(&row.thumb_hash)
    .bind(&row.filename)
    .bind(&row.kind)
    .bind(&row.mime)
    .bind(row.size)
    .bind(row.width)
    .bind(row.height)
    .bind(row.created_at)
    .bind(row.duration_ms)
    .bind(row.voice)
    .execute(&srv.db)
    .await?;
    Ok((StatusCode::CREATED, Json(srv.attachment_out(&row))))
}

/// Checks the attachments a new message wants to use and returns their content hashes (for the spam filter).
pub async fn check_for_message(auth: &Auth, channel_id: i64, ids: &[String]) -> ApiResult<Vec<i64>> {
    if ids.len() > MAX_PER_MESSAGE {
        return Err(ApiError::validation(BTreeMap::from([("attachment_ids", "too_many")])));
    }
    let mut out = Vec::new();
    for raw in ids {
        let id = crate::ids::parse(raw).ok_or_else(|| ApiError::validation(BTreeMap::from([("attachment_ids", "unknown_attachment")])))?;
        let ok: Option<i64> = sqlx::query_scalar("SELECT id FROM attachments WHERE id = ? AND uploader_id = ? AND channel_id = ? AND message_id IS NULL")
            .bind(id)
            .bind(auth.user.id)
            .bind(channel_id)
            .fetch_optional(&auth.srv.db)
            .await?;
        if ok.is_none() {
            return Err(ApiError::validation(BTreeMap::from([("attachment_ids", "unknown_attachment")])));
        }
        if !out.contains(&id) {
            out.push(id);
        }
    }
    Ok(out)
}

pub async fn blob_hashes(srv: &ServerCtx, ids: &[i64]) -> sqlx::Result<String> {
    let mut all = String::new();
    for id in ids {
        let h: Option<String> = sqlx::query_scalar("SELECT blob_hash FROM attachments WHERE id = ?").bind(id).fetch_optional(&srv.db).await?;
        all.push_str(&h.unwrap_or_default());
    }
    Ok(all)
}

#[derive(Deserialize)]
pub struct FilePath {
    id: String,
    variant: String,
    #[allow(dead_code)]
    name: String,
}

#[derive(Deserialize)]
pub struct Signed {
    exp: i64,
    sig: String,
    /// `download=1`: save it instead of showing it (works for links to other hosts, where the
    /// browser ignores `<a download>`).
    #[serde(default, deserialize_with = "flag")]
    download: bool,
}

/// `GET /files/{id}/{file|thumb}/{name}?exp=&sig=`. The signature is the permission, so `<img>` and
/// `<video>` tags work without login headers. Supports range requests (seeking in videos).
pub async fn serve(Srv(srv): Srv, Path(p): Path<FilePath>, Query(s): Query<Signed>, req: Request) -> Response {
    let not_found = || ApiError::not_found("not_found", "This file doesn't exist or the link expired.").into_response();
    let Some(id) = crate::ids::parse(&p.id) else { return not_found() };
    if (p.variant != "file" && p.variant != "thumb") || !srv.verify_file_link(id, &p.variant, s.exp, &s.sig) {
        return not_found();
    }
    let row: Option<AttachmentRow> = match sqlx::query_as("SELECT * FROM attachments WHERE id = ?").bind(id).fetch_optional(&srv.db).await {
        Ok(r) => r,
        Err(e) => return ApiError::from(e).into_response(),
    };
    let Some(row) = row else { return not_found() };
    let (hash, mime) = if p.variant == "thumb" {
        match &row.thumb_hash {
            Some(h) => (h.clone(), "image/webp".to_string()),
            None => return not_found(),
        }
    } else {
        (row.blob_hash.clone(), row.mime.clone())
    };
    let path = media::blob_path(&srv.files_dir, &hash);
    // Documents are only ever downloads; text is always plain text, whatever it contains.
    let document = row.kind == "file";
    let mime = if mime == "text/plain" { "text/plain; charset=utf-8".to_string() } else { mime };
    let mime_value: mime::Mime = mime.parse().unwrap_or(mime::APPLICATION_OCTET_STREAM);
    let mut resp = match ServeFile::new_with_mime(path, &mime_value).oneshot(req).await {
        Ok(r) => r.map(RawBody::new).into_response(),
        Err(_) => return ApiError::internal().into_response(),
    };
    let h = resp.headers_mut();
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("private, max-age=86400, immutable"));
    h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    h.insert(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static("default-src 'none'; sandbox"));
    let disposition = if (document || s.download) && p.variant == "file" { "attachment" } else { "inline" };
    if let Ok(v) = HeaderValue::from_str(&format!("{disposition}; filename=\"{}\"", row.filename)) {
        h.insert(header::CONTENT_DISPOSITION, v);
    }
    resp
}

/// `GET /me/storage`: how much the caller has used and the limits.
pub async fn my_storage(auth: Auth) -> ApiResult<Json<Value>> {
    let s = db::server_row(&auth.srv.db).await?;
    let now = now_ms();
    Ok(Json(json!({
        "used_today_bytes": user_usage(&auth.srv, auth.user.id, now - DAY_MS).await?,
        "used_total_bytes": user_usage(&auth.srv, auth.user.id, 0).await?,
        "max_file_mb": s.max_file_mb,
        "user_daily_mb": s.user_daily_mb,
        "user_total_mb": s.user_total_mb,
        "exempt": auth.is_owner,
    })))
}

/// Background cleanup for one server, every 10 minutes:
/// - uploads never attached to a message (after 1 hour)
/// - media older than the owner's retention setting (the messages stay, the files go)
/// - files on disk no attachment uses any more
pub fn spawn_cleanup(srv: Arc<ServerCtx>) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(600));
        loop {
            tick.tick().await;
            if let Err(e) = cleanup(&srv).await {
                tracing::warn!(server = %srv.slug, error = %e, "media cleanup failed");
            }
        }
    });
}

pub async fn cleanup(srv: &ServerCtx) -> anyhow::Result<()> {
    let now = now_ms();
    sqlx::query("DELETE FROM attachments WHERE message_id IS NULL AND created_at < ?").bind(now - PENDING_TTL_MS).execute(&srv.db).await?;
    let days: i64 = sqlx::query_scalar("SELECT media_retention_days FROM server LIMIT 1").fetch_one(&srv.db).await?;
    if days > 0 {
        let expired: Vec<(i64, i64)> = sqlx::query_as("SELECT DISTINCT message_id, channel_id FROM attachments WHERE message_id IS NOT NULL AND created_at < ?")
            .bind(now - days * DAY_MS)
            .fetch_all(&srv.db)
            .await?;
        sqlx::query("DELETE FROM attachments WHERE message_id IS NOT NULL AND created_at < ?").bind(now - days * DAY_MS).execute(&srv.db).await?;
        for (mid, cid) in expired {
            if let Some(row) = db::message_row(&srv.db, cid, mid).await? {
                let out = db::messages_out(srv, &[row], 0).await?.remove(0);
                super::messages::emit_in(srv, crate::events::Event::new("message_update", out), cid).await?;
            }
        }
    }
    srv.reload_ip_rules().await?;
    remove_orphan_blobs(srv, Duration::from_secs(600)).await?;
    Ok(())
}

/// Deletes these stored files if no attachment uses them any more (right after their messages
/// were deleted). Callers hold `storage::Room`, so no upload is between storing a file and
/// writing its attachment row.
/// Returns the bytes freed.
pub async fn remove_blobs_if_unused(srv: &ServerCtx, hashes: &[String]) -> sqlx::Result<i64> {
    let mut freed = 0;
    for hash in hashes {
        let size: Option<i64> = sqlx::query_scalar("SELECT size FROM blobs WHERE hash = ?").bind(hash).fetch_optional(&srv.db).await?;
        let gone = sqlx::query(
            "DELETE FROM blobs WHERE hash = ? AND hash NOT IN (SELECT blob_hash FROM attachments)              AND hash NOT IN (SELECT thumb_hash FROM attachments WHERE thumb_hash IS NOT NULL)",
        )
        .bind(hash)
        .execute(&srv.db)
        .await?;
        if gone.rows_affected() > 0 {
            let _ = tokio::fs::remove_file(media::blob_path(&srv.files_dir, hash)).await;
            freed += size.unwrap_or(0);
        }
    }
    Ok(freed)
}

/// Deletes stored files no attachment uses any more. `grace` spares files stored just now
/// (an upload stores its file a moment before its attachment row).
pub async fn remove_orphan_blobs(srv: &ServerCtx, grace: Duration) -> sqlx::Result<()> {
    let orphans: Vec<String> = sqlx::query_scalar(
        "SELECT hash FROM blobs WHERE created_at < ? AND hash NOT IN (SELECT blob_hash FROM attachments) \
         AND hash NOT IN (SELECT thumb_hash FROM attachments WHERE thumb_hash IS NOT NULL)",
    )
    .bind(now_ms() - grace.as_millis() as i64)
    .fetch_all(&srv.db)
    .await?;
    for hash in orphans {
        let _ = tokio::fs::remove_file(media::blob_path(&srv.files_dir, &hash)).await;
        sqlx::query("DELETE FROM blobs WHERE hash = ?").bind(&hash).execute(&srv.db).await?;
    }
    Ok(())
}

#[derive(Deserialize)]
pub struct AttachmentPath {
    id: String,
}

/// `DELETE /attachments/{id}`: the uploader removes an upload they haven't sent (a discarded
/// recording or a file taken out of the composer), so it stops counting toward their limits.
pub async fn delete_unsent(auth: Auth, Path(p): Path<AttachmentPath>) -> ApiResult<StatusCode> {
    let id = parse_id(&p.id, "not_found")?;
    let srv = &auth.srv;
    let row: Option<(String, Option<String>)> = sqlx::query_as("SELECT blob_hash, thumb_hash FROM attachments WHERE id = ? AND uploader_id = ? AND message_id IS NULL")
        .bind(id)
        .bind(auth.user.id)
        .fetch_optional(&srv.db)
        .await?;
    let Some((blob, thumb)) = row else {
        return Err(ApiError::not_found("not_found", "No such unsent upload."));
    };
    sqlx::query("DELETE FROM attachments WHERE id = ? AND message_id IS NULL").bind(id).execute(&srv.db).await?;
    let hashes: Vec<String> = std::iter::once(blob).chain(thumb).collect();
    remove_blobs_if_unused(srv, &hashes).await?;
    *srv.storage_used.lock().await = None;
    Ok(StatusCode::NO_CONTENT)
}
