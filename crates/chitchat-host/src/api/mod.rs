//! HTTP routes (docs/api.md).

mod admin;
pub(crate) mod ai;
mod auth;
mod backups;
mod channels;
mod conversations;
mod discovery;
mod dms;
mod emoji;
mod error;
mod extract;
mod files;
mod messages;
pub(crate) mod moderation;
mod notify;
mod profile;
mod push;
mod resources;
mod roles;
mod search;
mod server;
mod servers;
mod storage;
mod update;
mod voice;
mod ws;

use std::path::Path;
use std::sync::Arc;

use axum::Router;
use axum::http::Method;
use axum::http::header::{AUTHORIZATION, CONTENT_TYPE};
use axum::routing::{any, get, patch, post, put};
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;

pub use auth::hash_password;
pub use error::{ApiError, ApiResult};
pub use extract::{Auth, Body, Srv};
pub use files::spawn_cleanup;
pub use resources::start as start_resources;

use crate::host::Host;

pub fn router(host: Arc<Host>, web_dir: &Path) -> Router {
    let server_api = Router::new()
        .route("/server/public", get(server::public))
        .route("/server", get(server::get))
        .route("/auth/register", post(auth::register))
        .route("/auth/login", post(auth::login))
        .route("/auth/logout", post(auth::logout))
        .route("/me", get(auth::me).patch(auth::patch_me))
        .route("/me/avatar", put(profile::upload_avatar).delete(profile::delete_avatar))
        .route("/emojis", get(emoji::list).post(emoji::create))
        .route("/emojis/{id}", axum::routing::delete(emoji::delete))
        .route("/emojis/{id}/{name}", get(emoji::image))
        .route("/avatars/{user_id}/{name}", get(profile::avatar))
        .route("/categories", get(server::categories).post(channels::create_category))
        .route("/categories/{id}", patch(channels::patch_category).delete(channels::delete_category))
        .route("/channels", get(server::channels).post(channels::create_channel))
        .route("/channels/{id}", patch(channels::patch_channel).delete(channels::delete_channel))
        .route("/channels/{id}/messages", get(messages::list).post(messages::create))
        .route("/channels/{id}/pins", get(conversations::pins))
        .route("/channels/{id}/pins/{mid}", put(conversations::pin).delete(conversations::unpin))
        .route("/channels/{id}/threads", get(conversations::threads))
        .route("/channels/{id}/messages/{mid}/thread", post(conversations::create_thread))
        .route("/channels/{id}/ack", post(notify::ack))
        .route("/read-states", get(notify::read_states))
        .route("/me/preferences", get(notify::get_preferences).patch(notify::patch_preferences))
        .route("/dms", get(dms::list).post(dms::open))
        .route("/search", get(search::search))
        .route("/push/key", get(push::key))
        .route("/push/subscription", put(push::subscribe).delete(push::unsubscribe))
        .route("/push/test", post(push::test))
        .route("/channels/{id}/permissions", get(conversations::permissions))
        .route("/channels/{id}/permissions/{role_id}", put(conversations::set_permission).delete(conversations::clear_permission))
        .route("/channels/{id}/messages/{mid}", patch(messages::edit).delete(messages::delete))
        .route("/channels/{id}/messages/{mid}/reactions/{emoji}", put(messages::react).delete(messages::unreact))
        .route("/members", get(server::members))
        .route("/members/{user_id}", get(server::member))
        .route("/roles", get(server::roles).post(roles::create))
        .route("/roles/{id}", patch(roles::update).delete(roles::delete))
        .route("/members/{user_id}/roles/{role_id}", put(roles::add_member_role).delete(roles::remove_member_role))
        .route("/settings", get(admin::get_settings).patch(admin::patch_settings))
        .route("/settings/password", put(admin::set_password))
        .route("/invites", get(admin::list_invites).post(admin::create_invite))
        .route("/invites/{code}", get(admin::preview_invite).delete(admin::delete_invite))
        .route("/ip-rules", get(admin::list_ip_rules).post(admin::add_ip_rule))
        .route("/ip-rules/{id}", axum::routing::delete(admin::delete_ip_rule))
        .route("/channels/{id}/attachments", post(files::upload))
        .route("/attachments/{id}", axum::routing::delete(files::delete_unsent))
        .route("/files/{id}/{variant}/{name}", get(files::serve))
        .route("/me/storage", get(files::my_storage))
        .route("/storage", get(storage::get))
        .route("/host/resources", get(resources::get).patch(resources::patch))
        .route("/host/backups", get(backups::get).post(backups::run))
        .route("/host/update", get(update::get).post(update::apply))
        .route("/host/servers", get(servers::list).post(servers::create))
        .route("/host/discovery", get(discovery::get_settings).patch(discovery::patch_settings))
        .route("/bans", get(moderation::list_bans).post(moderation::ban))
        .route("/bans/{id}", axum::routing::delete(moderation::unban))
        .route("/members/{user_id}/timeout", put(moderation::timeout).delete(moderation::clear_timeout))
        .route("/members/{user_id}/kick", post(moderation::kick))
        .route("/audit-log", get(moderation::audit_log))
        .route("/ai/settings", get(ai::get_settings).patch(ai::patch_settings))
        .route("/ai/test", post(ai::test))
        .route("/ai/models", get(ai::models))
        .route("/ai/flags", get(ai::list_flags))
        .route("/ai/flags/{id}/resolve", post(ai::resolve))
        .route("/ws", get(ws::handler))
        .fallback(not_found);

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST, Method::PATCH, Method::PUT, Method::DELETE])
        .allow_headers([AUTHORIZATION, CONTENT_TYPE])
        // Lets a client loaded from a public address use a server on a LAN host (Chrome asks first).
        .allow_private_network(true);

    // The web client, with every unknown path falling back to index.html (SPA routing).
    // index.html is never cached, so a new build shows up on the next load; the hashed
    // files under assets/ can be cached for good.
    let web = tower::ServiceBuilder::new()
        .map_response(|mut res: axum::http::Response<_>| {
            let html = res.headers().get(CONTENT_TYPE).and_then(|v| v.to_str().ok()).is_some_and(|v| v.starts_with("text/html"));
            let value = if html { "no-cache" } else { "public, max-age=604800" };
            res.headers_mut().insert(axum::http::header::CACHE_CONTROL, axum::http::HeaderValue::from_static(value));
            res
        })
        .service(ServeDir::new(web_dir).fallback(ServeFile::new(web_dir.join("index.html"))));

    Router::new()
        .route("/api/host", get(server::host_info))
        .route("/api/discovery", get(discovery::document))
        .route("/api/discovery/announce", post(discovery::announce))
        .route("/api/discovery/browse", get(discovery::browse))
        .route("/api/{*rest}", any(not_found))
        .nest("/s/{slug}/api", server_api)
        .fallback_service(web)
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(host)
}

async fn not_found() -> ApiError {
    ApiError::not_found("not_found", "No such endpoint.")
}

/// Install only on a TLS listener: browsers remember to use HTTPS for this host.
/// Do not include subdomains: a host can live beneath a shared DNS provider.
pub fn with_transport_security(app: Router) -> Router {
    app.layer(axum::middleware::map_response(|mut response: axum::response::Response| async move {
        response.headers_mut().insert(axum::http::header::STRICT_TRANSPORT_SECURITY, axum::http::HeaderValue::from_static("max-age=31536000"));
        response
    }))
}

/// Sends page loads that use the canonical domain with another port (e.g. `:8090` instead of
/// the default 443) to the canonical address. API calls, WebSockets and file links are left
/// alone, and so are IPs and other names (LAN, Tailscale).
pub fn with_canonical_redirect(app: Router, url: &str) -> anyhow::Result<Router> {
    use axum::response::{IntoResponse, Redirect};
    let parsed = reqwest::Url::parse(url)?;
    let host = parsed.host_str().ok_or_else(|| anyhow::anyhow!("--canonical-url needs a host name"))?.to_lowercase();
    let port = parsed.port_or_known_default();
    let base = url.trim_end_matches('/').to_string();
    let canonical = Arc::new((host, port, base));
    Ok(app.layer(axum::middleware::from_fn(move |req: axum::extract::Request, next: axum::middleware::Next| {
        let canonical = canonical.clone();
        async move {
            let (host, port, base) = &*canonical;
            let target = req.headers().get(axum::http::header::HOST).and_then(|h| h.to_str().ok()).and_then(|h| {
                let (name, p) = match h.rsplit_once(':') {
                    Some((n, p)) if !n.contains(']') || n.ends_with(']') => (n, p.parse::<u16>().ok()),
                    _ => (h, None),
                };
                let path = req.uri().path();
                let page = matches!(*req.method(), Method::GET | Method::HEAD) && !path.starts_with("/api/") && !path.contains("/api/") && !path.starts_with("/assets/");
                (page && name.eq_ignore_ascii_case(host) && p.is_some() && p != *port).then(|| format!("{base}{}", req.uri().path_and_query().map_or("/", |pq| pq.as_str())))
            });
            match target {
                Some(to) => Redirect::permanent(&to).into_response(),
                None => next.run(req).await,
            }
        }
    })))
}
