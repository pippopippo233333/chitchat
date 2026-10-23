//! Host updates as the host admin sees them in Resources. The work is in `crate::update`.

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde_json::{Value, json};

use super::{ApiError, ApiResult, Auth};
use crate::host::Host;

/// `GET /host/update`: this version, the newest release, and how the last update went.
pub async fn get(State(host): State<Arc<Host>>, auth: Auth) -> ApiResult<Json<Value>> {
    super::resources::require_host_admin(&auth)?;
    Ok(Json(crate::update::status(&host.data_dir).await))
}

/// `POST /host/update`: install the newest release and restart (the host is briefly offline).
pub async fn apply(State(host): State<Arc<Host>>, auth: Auth) -> ApiResult<(StatusCode, Json<Value>)> {
    super::resources::require_host_admin(&auth)?;
    let web_dir = crate::update::WEB_DIR.get().cloned().ok_or_else(ApiError::internal)?;
    match crate::update::start_apply(host, web_dir).await {
        Ok(version) => Ok((StatusCode::ACCEPTED, Json(json!({ "installing": version })))),
        Err(e) => Err(ApiError::new(StatusCode::CONFLICT, "update_unavailable", format!("{e:#}"))),
    }
}
