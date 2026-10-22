//! Discovery routes (see `crate::discovery`): the public document other hosts read, announces,
//! the merged browse list, and the host admin's settings.

use std::collections::BTreeMap;
use std::net::SocketAddr;

use axum::Json;
use axum::extract::ConnectInfo;
use axum::http::StatusCode;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{ApiError, ApiResult, Auth, Body};
use crate::discovery::{self, Config};

fn off() -> ApiError {
    ApiError::not_found("discovery_off", "This host doesn't take part in server discovery.")
}

/// `GET /api/discovery` (public): read by other hosts.
pub async fn document() -> ApiResult<Json<Value>> {
    discovery::document().await.map(Json).ok_or_else(off)
}

#[derive(Deserialize)]
pub struct AnnounceReq {
    url: String,
}

/// `POST /api/discovery/announce` (public): another host introduces itself.
pub async fn announce(ConnectInfo(addr): ConnectInfo<SocketAddr>, Body(req): Body<AnnounceReq>) -> ApiResult<StatusCode> {
    discovery::announce(addr.ip(), &req.url).map_err(|e| ApiError::bad_request(e.to_string()))?;
    Ok(StatusCode::ACCEPTED)
}

/// `GET /api/discovery/browse` (public): every discoverable server this host knows.
pub async fn browse() -> ApiResult<Json<Value>> {
    discovery::browse().await.map(Json).ok_or_else(off)
}

fn settings_out() -> Value {
    let (peers, last_round) = discovery::peers();
    let hosts: Vec<Value> = peers.iter().map(|p| json!({ "url": p.url, "name": p.name, "servers": p.servers.len(), "last_ok": p.last_ok, "fails": p.fails })).collect();
    json!({ "config": discovery::config(), "hosts": hosts, "last_round": last_round })
}

/// `GET /host/discovery` (host admin).
pub async fn get_settings(auth: Auth) -> ApiResult<Json<Value>> {
    super::resources::require_host_admin(&auth)?;
    Ok(Json(settings_out()))
}

#[derive(Deserialize)]
pub struct SettingsReq {
    enabled: Option<bool>,
    public_url: Option<Option<String>>,
    seeds: Option<Vec<String>>,
    seed_lists: Option<Vec<String>>,
    blocked: Option<Vec<String>>,
}

/// Checks a list of URLs, returning them normalized.
fn urls(field: &'static str, list: &[String], fields: &mut BTreeMap<&'static str, &'static str>) -> Vec<String> {
    if list.len() > 200 {
        fields.insert(field, "too_many");
        return Vec::new();
    }
    let mut out = Vec::new();
    for u in list.iter().map(|u| u.trim()).filter(|u| !u.is_empty()) {
        // Seed list files may have a path; host addresses may not.
        let ok = if field == "seed_lists" { reqwest::Url::parse(u).ok().filter(|p| p.scheme() == "https").map(|p| p.to_string()) } else { discovery::normalize_url(u).ok() };
        match ok {
            Some(u) if !out.contains(&u) => out.push(u),
            Some(_) => {}
            None => {
                fields.insert(field, "invalid");
            }
        }
    }
    out
}

/// `PATCH /host/discovery` (host admin). Turning it on (or changing it) starts a round now.
pub async fn patch_settings(auth: Auth, Body(req): Body<SettingsReq>) -> ApiResult<Json<Value>> {
    super::resources::require_host_admin(&auth)?;
    let mut cfg: Config = discovery::config();
    let mut fields = BTreeMap::new();
    if let Some(u) = req.public_url {
        cfg.public_url = match u.as_deref().map(str::trim).filter(|u| !u.is_empty()) {
            None => None,
            Some(u) => match discovery::normalize_url(u) {
                Ok(u) => Some(u),
                Err(_) => {
                    fields.insert("public_url", "invalid");
                    None
                }
            },
        };
    }
    if let Some(l) = &req.seeds {
        cfg.seeds = urls("seeds", l, &mut fields);
    }
    if let Some(l) = &req.seed_lists {
        cfg.seed_lists = urls("seed_lists", l, &mut fields);
    }
    if let Some(l) = &req.blocked {
        cfg.blocked = urls("blocked", l, &mut fields);
    }
    if let Some(e) = req.enabled {
        cfg.enabled = e;
    }
    if cfg.enabled && cfg.public_url.is_none() {
        fields.insert("public_url", "required");
    }
    if !fields.is_empty() {
        return Err(ApiError::validation(fields));
    }
    super::moderation::audit(&auth.srv, Some(auth.user.id), "discovery_update", None, serde_json::to_value(&cfg).unwrap_or_default()).await;
    discovery::set_config(cfg);
    Ok(Json(settings_out()))
}
