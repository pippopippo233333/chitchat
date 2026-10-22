use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::Json;
use axum::extract::{ConnectInfo, FromRequest, FromRequestParts, Path, Request};
use axum::http::header::AUTHORIZATION;
use axum::http::request::Parts;
use serde::de::DeserializeOwned;

use super::{ApiError, ApiResult};
use crate::host::{Host, ServerCtx};
use crate::models::UserRow;
use crate::perms;

/// The server named by `{slug}` in the path.
pub struct Srv(pub Arc<ServerCtx>);

impl FromRequestParts<Arc<Host>> for Srv {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, host: &Arc<Host>) -> ApiResult<Self> {
        let unknown = || ApiError::not_found("unknown_server", "This server doesn't exist on this host.");
        let Path(params) = Path::<HashMap<String, String>>::from_request_parts(parts, host).await.map_err(|_| unknown())?;
        let slug = params.get("slug").ok_or_else(unknown)?;
        let srv = host.get(slug).await.ok_or_else(unknown)?;
        if let Some(ConnectInfo(addr)) = parts.extensions.get::<ConnectInfo<SocketAddr>>()
            && !srv.ip_allowed(addr.ip())
        {
            return Err(ApiError::forbidden("ip_blocked", "Your network isn't allowed to access this server."));
        }
        Ok(Srv(srv))
    }
}

/// A logged-in member of the server in the path.
pub struct Auth {
    pub srv: Arc<ServerCtx>,
    pub user: UserRow,
    pub perms: u64,
    pub is_owner: bool,
    pub token_hash: String,
}

impl Auth {
    pub fn require(&self, wanted: u64) -> ApiResult<()> {
        if perms::has(self.perms, wanted) { Ok(()) } else { Err(ApiError::missing_permission()) }
    }
}

impl FromRequestParts<Arc<Host>> for Auth {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, host: &Arc<Host>) -> ApiResult<Self> {
        let Srv(srv) = Srv::from_request_parts(parts, host).await?;
        let token = parts.headers.get(AUTHORIZATION).and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("Bearer ")).ok_or_else(ApiError::unauthorized)?;
        super::auth::authenticate(srv, token.trim()).await
    }
}

/// A JSON body whose parse errors use the API error format.
pub struct Body<T>(pub T);

impl<S: Send + Sync, T: DeserializeOwned> FromRequest<S> for Body<T> {
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &S) -> ApiResult<Self> {
        match Json::<T>::from_request(req, state).await {
            Ok(Json(v)) => Ok(Body(v)),
            Err(e) => Err(ApiError::bad_request(e.body_text())),
        }
    }
}

pub fn parse_id(s: &str, code: &'static str) -> ApiResult<i64> {
    crate::ids::parse(s).ok_or_else(|| ApiError::not_found(code, "Not found."))
}
