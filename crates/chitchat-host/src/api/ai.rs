//! Owner-only AI moderation settings, and the flag review queue (docs/api.md section 4f).

use axum::Json;
use axum::extract::{Path, Query};
use serde::Deserialize;
use serde_json::{Value, json};

use super::extract::parse_id;
use super::{ApiError, ApiResult, Auth, Body};
use crate::ai::{self, AiConfig, LlmConfig};
use crate::events::Event;
use crate::host::ServerCtx;
use crate::models::{now_ms, ts};
use crate::perms::MANAGE_MESSAGES;

const MAX_RULES: usize = 2000;

#[derive(Deserialize)]
pub struct IdPath {
    id: String,
}

fn owner_only(auth: &Auth) -> ApiResult<()> {
    if auth.is_owner { Ok(()) } else { Err(ApiError::forbidden("owner_only", "Only the server owner can set up AI moderation.")) }
}

fn masked(cfg: &AiConfig) -> Value {
    let key = |k: &Option<String>| k.as_deref().is_some_and(|k| !k.is_empty());
    json!({
        "enabled": cfg.enabled,
        "channel_ids": cfg.channel_ids,
        "action": cfg.action(),
        "timeout_minutes": cfg.timeout_minutes,
        "rules": cfg.rules,
        "laya": cfg.laya.as_ref().map(|l| json!({ "url": l.url, "has_api_key": key(&l.api_key), "threshold": l.threshold, "spam_threshold": l.spam_threshold })),
        "llm": cfg.llm.as_ref().map(|l| json!({
            "kind": l.kind, "base_url": l.base_url, "has_api_key": key(&l.api_key), "model": l.model,
            "requests_per_minute": l.requests_per_minute, "requests_per_day": l.requests_per_day, "cloud": l.is_cloud(),
        })),
    })
}

fn settings_out(srv: &ServerCtx) -> Value {
    let cfg = srv.ai.config();
    let stats = srv.ai.stats.lock().unwrap().clone();
    json!({ "config": masked(&cfg), "status": { "queued": srv.ai.queued(), "checked": stats.checked, "flagged": stats.flagged,
        "llm_requests_today": stats.llm_requests_today, "last_error": stats.last_error }, "notice": ai::notice(&cfg) })
}

pub async fn get_settings(auth: Auth) -> ApiResult<Json<Value>> {
    owner_only(&auth)?;
    Ok(Json(settings_out(&auth.srv)))
}

/// Keys: absent = keep the saved one, "" = remove, a string = replace.
#[derive(Deserialize)]
pub struct LayaPatch {
    url: Option<String>,
    api_key: Option<String>,
    threshold: Option<f64>,
    spam_threshold: Option<f64>,
}

#[derive(Deserialize)]
pub struct LlmPatch {
    kind: Option<String>,
    base_url: Option<String>,
    api_key: Option<String>,
    model: Option<String>,
    requests_per_minute: Option<u32>,
    requests_per_day: Option<u32>,
}

/// `laya`/`llm`: absent = keep, null = turn that provider off, an object = set/merge.
#[derive(Deserialize)]
pub struct SettingsPatch {
    enabled: Option<bool>,
    channel_ids: Option<Vec<String>>,
    action: Option<String>,
    timeout_minutes: Option<i64>,
    rules: Option<String>,
    #[serde(default, deserialize_with = "present")]
    laya: Option<Option<LayaPatch>>,
    #[serde(default, deserialize_with = "present")]
    llm: Option<Option<LlmPatch>>,
}

pub(crate) fn present<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(d: D) -> Result<Option<Option<T>>, D::Error> {
    Ok(Some(Option::deserialize(d)?))
}

fn bad(field: &'static str, err: &'static str) -> ApiError {
    ApiError::validation(std::collections::BTreeMap::from([(field, err)]))
}

fn http_url(u: &str) -> bool {
    reqwest::Url::parse(u).is_ok_and(|u| matches!(u.scheme(), "http" | "https") && u.host().is_some())
}

pub async fn patch_settings(auth: Auth, Body(req): Body<SettingsPatch>) -> ApiResult<Json<Value>> {
    owner_only(&auth)?;
    let mut cfg = (*auth.srv.ai.config()).clone();
    if let Some(e) = req.enabled {
        cfg.enabled = e;
    }
    if let Some(ids) = req.channel_ids {
        let mut ok = vec![];
        for id in ids {
            let cid = crate::ids::parse(&id).ok_or_else(|| bad("channel_ids", "unknown_channel"))?;
            match crate::db::channel_row(&auth.srv.db, cid).await? {
                Some(c) if c.kind == "text" => ok.push(cid.to_string()),
                _ => return Err(bad("channel_ids", "unknown_channel")),
            }
        }
        cfg.channel_ids = ok;
    }
    if let Some(a) = req.action {
        if !matches!(a.as_str(), "flag" | "hide" | "timeout") {
            return Err(bad("action", "invalid"));
        }
        cfg.action = a;
    }
    if let Some(m) = req.timeout_minutes {
        if !(1..=40_320).contains(&m) {
            return Err(bad("timeout_minutes", "out_of_range"));
        }
        cfg.timeout_minutes = m;
    }
    if let Some(r) = req.rules {
        if r.chars().count() > MAX_RULES {
            return Err(bad("rules", "too_long"));
        }
        cfg.rules = r;
    }
    match req.laya {
        Some(None) => cfg.laya = None,
        Some(Some(p)) => {
            let mut l = cfg.laya.take().unwrap_or_default();
            if let Some(u) = p.url {
                if !http_url(&u) {
                    return Err(bad("laya.url", "invalid"));
                }
                l.url = u;
            }
            if let Some(k) = p.api_key {
                l.api_key = Some(k).filter(|k| !k.is_empty());
            }
            for (v, slot, field) in [(p.threshold, &mut l.threshold, "laya.threshold"), (p.spam_threshold, &mut l.spam_threshold, "laya.spam_threshold")] {
                if let Some(v) = v {
                    if !(0.05..=1.0).contains(&v) {
                        return Err(bad(field, "out_of_range"));
                    }
                    *slot = v;
                }
            }
            cfg.laya = Some(l);
        }
        None => {}
    }
    match req.llm {
        Some(None) => cfg.llm = None,
        Some(Some(p)) => {
            let mut l = cfg.llm.take().unwrap_or_default();
            if let Some(k) = p.kind {
                if !matches!(k.as_str(), "gemini" | "openai") {
                    return Err(bad("llm.kind", "invalid"));
                }
                l.kind = k;
            }
            if let Some(u) = p.base_url {
                l.base_url = u;
            }
            if l.kind == "openai" && !http_url(&l.base_url) {
                return Err(bad("llm.base_url", "invalid"));
            }
            if let Some(k) = p.api_key {
                l.api_key = Some(k).filter(|k| !k.is_empty());
            }
            if let Some(m) = p.model {
                let m = m.trim().to_string();
                if m.is_empty() || m.len() > 200 {
                    return Err(bad("llm.model", "invalid"));
                }
                l.model = m;
            }
            if let Some(r) = p.requests_per_minute {
                l.requests_per_minute = r.min(10_000);
            }
            if let Some(r) = p.requests_per_day {
                l.requests_per_day = r.min(10_000_000);
            }
            cfg.llm = Some(l);
        }
        None => {}
    }
    let before = ai::notice(&auth.srv.ai.config());
    ai::save_config(&auth.srv.db, &cfg).await?;
    *auth.srv.ai.config.write().unwrap() = std::sync::Arc::new(cfg);
    let after = ai::notice(&auth.srv.ai.config());
    super::moderation::audit(&auth.srv, Some(auth.user.id), "ai_settings_update", None, json!({})).await;
    if before != after {
        auth.srv.emit(Event::new("ai_notice_update", json!({ "ai_moderation": after })));
    }
    Ok(Json(settings_out(&auth.srv)))
}

#[derive(Deserialize)]
pub struct TestReq {
    content: String,
}

/// Runs the current pipeline on a sample text; nothing is stored or acted on.
pub async fn test(auth: Auth, Body(req): Body<TestReq>) -> ApiResult<Json<Value>> {
    owner_only(&auth)?;
    let cfg = auth.srv.ai.config();
    if cfg.laya.is_none() && cfg.llm.is_none() {
        return Err(ApiError::bad_request("Set up Laya or an LLM first."));
    }
    let content: String = req.content.chars().take(4000).collect();
    let error_before = auth.srv.ai.stats.lock().unwrap().last_error.clone();
    let started = std::time::Instant::now();
    let v = ai::check(&auth.srv.ai, &cfg, &[&content]).await.remove(0);
    let error_after = auth.srv.ai.stats.lock().unwrap().last_error.clone();
    Ok(Json(json!({
        "verdict": v,
        "action": if v.flagged { if v.unconfirmed { "flag" } else { cfg.action() } } else { "none" },
        "ms": started.elapsed().as_millis() as u64,
        "error": if error_after != error_before { error_after } else { None },
    })))
}

pub async fn models(auth: Auth) -> ApiResult<Json<Value>> {
    owner_only(&auth)?;
    let cfg = auth.srv.ai.config();
    let llm: LlmConfig = cfg.llm.clone().ok_or_else(|| ApiError::bad_request("Set up an LLM first."))?;
    match ai::list_models(&auth.srv.ai, &llm).await {
        Ok(m) => Ok(Json(json!({ "models": m }))),
        Err(e) => Err(ApiError::bad_request(format!("Couldn't list models: {e}"))),
    }
}

// ---------------------------------------------------------------- flags

#[derive(sqlx::FromRow)]
struct FlagRow {
    id: i64,
    message_id: Option<i64>,
    channel_id: i64,
    author_id: i64,
    content: String,
    categories: String,
    scores: String,
    provider: String,
    reason: String,
    action: String,
    status: String,
    created_at: i64,
    resolved_by: Option<i64>,
    resolved_at: Option<i64>,
}

fn row_out(r: FlagRow, author: Option<crate::models::AuthorOut>) -> Value {
    json!({
        "id": r.id.to_string(),
        "message_id": r.message_id.map(|m| m.to_string()),
        "channel_id": r.channel_id.to_string(),
        "author": author,
        "author_id": r.author_id.to_string(),
        "content": r.content,
        "categories": serde_json::from_str::<Value>(&r.categories).unwrap_or(json!([])),
        "scores": serde_json::from_str::<Value>(&r.scores).unwrap_or(Value::Null),
        "provider": r.provider,
        "reason": r.reason,
        "action": r.action,
        "status": r.status,
        "created_at": ts(r.created_at),
        "resolved_by": r.resolved_by.map(|v| v.to_string()),
        "resolved_at": r.resolved_at.map(ts),
    })
}

async fn author(srv: &ServerCtx, id: i64) -> Option<crate::models::AuthorOut> {
    crate::db::user_by_id(&srv.db, id).await.ok().flatten().map(|u| crate::models::AuthorOut {
        id: u.id.to_string(),
        username: u.username,
        display_name: u.display_name,
        avatar_url: u.avatar_url,
    })
}

pub async fn flag_out(srv: &ServerCtx, id: i64) -> sqlx::Result<Option<Value>> {
    let row: Option<FlagRow> = sqlx::query_as("SELECT * FROM ai_flags WHERE id = ?").bind(id).fetch_optional(&srv.db).await?;
    Ok(match row {
        Some(r) => {
            let a = author(srv, r.author_id).await;
            Some(row_out(r, a))
        }
        None => None,
    })
}

#[derive(Deserialize)]
pub struct FlagQuery {
    status: Option<String>,
    before: Option<String>,
    limit: Option<i64>,
}

pub async fn list_flags(auth: Auth, Query(q): Query<FlagQuery>) -> ApiResult<Json<Vec<Value>>> {
    auth.require(MANAGE_MESSAGES)?;
    let status = q.status.unwrap_or_else(|| "open".into());
    let before = q.before.as_deref().and_then(crate::ids::parse).unwrap_or(i64::MAX);
    let limit = q.limit.unwrap_or(50).clamp(1, 100);
    let rows: Vec<FlagRow> = if status == "all" {
        sqlx::query_as("SELECT * FROM ai_flags WHERE id < ? ORDER BY id DESC LIMIT ?").bind(before).bind(limit).fetch_all(&auth.srv.db).await?
    } else {
        sqlx::query_as("SELECT * FROM ai_flags WHERE status = ? AND id < ? ORDER BY id DESC LIMIT ?").bind(&status).bind(before).bind(limit).fetch_all(&auth.srv.db).await?
    };
    let mut out = vec![];
    for r in rows {
        let a = author(&auth.srv, r.author_id).await;
        out.push(row_out(r, a));
    }
    Ok(Json(out))
}

#[derive(Deserialize)]
pub struct ResolveReq {
    /// "dismiss" (the AI was wrong / no action needed), "delete_message", or "resolved"
    /// (handled some other way, e.g. a ban).
    action: String,
}

pub async fn resolve(auth: Auth, Path(p): Path<IdPath>, Body(req): Body<ResolveReq>) -> ApiResult<Json<Value>> {
    auth.require(MANAGE_MESSAGES)?;
    let id = parse_id(&p.id, "not_found")?;
    let row: FlagRow =
        sqlx::query_as("SELECT * FROM ai_flags WHERE id = ?").bind(id).fetch_optional(&auth.srv.db).await?.ok_or_else(|| ApiError::not_found("not_found", "No such flag."))?;
    let status = match req.action.as_str() {
        "dismiss" => "dismissed",
        "resolved" => "resolved",
        "delete_message" => {
            if let Some(mid) = row.message_id {
                let gone = sqlx::query("DELETE FROM messages WHERE id = ?").bind(mid).execute(&auth.srv.db).await?;
                if gone.rows_affected() > 0 {
                    auth.srv.emit(Event::new("message_delete", json!({ "id": mid.to_string(), "channel_id": row.channel_id.to_string() })).in_channel(row.channel_id));
                    let preview: String = row.content.chars().take(200).collect();
                    super::moderation::audit(
                        &auth.srv,
                        Some(auth.user.id),
                        "message_delete",
                        Some(row.author_id),
                        json!({ "channel_id": row.channel_id.to_string(), "content": preview, "flag_id": id.to_string() }),
                    )
                    .await;
                }
            }
            "deleted"
        }
        _ => return Err(bad("action", "invalid")),
    };
    sqlx::query("UPDATE ai_flags SET status = ?, resolved_by = ?, resolved_at = ? WHERE id = ?")
        .bind(status)
        .bind(auth.user.id)
        .bind(now_ms())
        .bind(id)
        .execute(&auth.srv.db)
        .await?;
    let out = flag_out(&auth.srv, id).await?.ok_or_else(ApiError::internal)?;
    auth.srv.emit(Event::new("ai_flag_update", &out).needs(MANAGE_MESSAGES));
    Ok(Json(out))
}
