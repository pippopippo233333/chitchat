//! chitchat-host: runs one or more self-hosted ChitChat servers.

mod ai;
mod api;
mod db;
mod discovery;
mod events;
mod host;
mod ids;
mod ipfilter;
mod media;
mod models;
mod perms;
mod push;
mod spam;
mod tls;
mod voice;

use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
use tracing_subscriber::EnvFilter;

use host::Host;

#[derive(Parser)]
#[command(name = "chitchat-host", version, about = "Host your own ChitChat servers")]
struct Cli {
    /// Where servers, databases and files are stored.
    #[arg(long, global = true, default_value = "data")]
    data_dir: PathBuf,
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Start the host (the default).
    Run(RunArgs),
    /// Manage servers on this host.
    #[command(subcommand)]
    Server(ServerCmd),
    /// Manage accounts (run on the host machine, e.g. when someone forgot their password).
    #[command(subcommand)]
    User(UserCmd),
}

#[derive(Subcommand)]
enum UserCmd {
    /// Give an account a new temporary password and log it out everywhere.
    ResetPassword {
        /// The server's slug.
        slug: String,
        username: String,
    },
}

#[derive(Args)]
struct RunArgs {
    /// Address and port to listen on. 0.0.0.0 accepts connections from other machines.
    #[arg(long, default_value = "0.0.0.0:8080")]
    bind: String,
    /// The built web client.
    #[arg(long, default_value = "web/dist")]
    web_dir: PathBuf,
    /// Serve HTTPS with a self-signed certificate (made on first run, kept in <data-dir>/tls/).
    /// Needed for voice from other machines; browsers show a warning once.
    #[arg(long)]
    https: bool,
    /// Serve HTTPS with your own certificate (PEM), e.g. from Let's Encrypt or `tailscale cert`.
    #[arg(long, requires = "tls_key")]
    tls_cert: Option<PathBuf>,
    /// The private key (PEM) for --tls-cert.
    #[arg(long, requires = "tls_cert")]
    tls_key: Option<PathBuf>,
    /// Extra names for the self-signed certificate (a domain or Tailscale name), repeatable.
    #[arg(long = "tls-name")]
    tls_names: Vec<String>,
    /// The public address people should use, e.g. https://chat.example.org. Pages opened with
    /// the same domain on another port are sent there, so a login is kept in one place
    /// (browsers keep logins per address). IPs and other names are left alone.
    #[arg(long)]
    canonical_url: Option<String>,
}

impl Default for RunArgs {
    fn default() -> Self {
        RunArgs { bind: "0.0.0.0:8080".into(), web_dir: "web/dist".into(), https: false, tls_cert: None, tls_key: None, tls_names: vec![], canonical_url: None }
    }
}

impl RunArgs {
    fn tls(&self) -> Option<tls::Tls> {
        match (&self.tls_cert, &self.tls_key) {
            (Some(cert), Some(key)) => Some(tls::Tls::Files { cert: cert.clone(), key: key.clone() }),
            _ if self.https => Some(tls::Tls::SelfSigned { extra_names: self.tls_names.clone() }),
            _ => None,
        }
    }
}

#[derive(Subcommand)]
enum ServerCmd {
    /// Create a new server and print its owner setup code.
    Create {
        /// URL name: 2-32 characters a-z, 0-9 and '-'.
        slug: String,
        /// Display name (defaults to the slug).
        #[arg(long)]
        name: Option<String>,
    },
    /// List servers on this host.
    List,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt().with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,tower_http=warn".into())).init();
    let cli = Cli::parse();
    let host = Host::load(&cli.data_dir).await.context("loading the data directory")?;

    match cli.cmd.unwrap_or(Cmd::Run(RunArgs::default())) {
        Cmd::Run(args) => run(host, args).await,
        Cmd::Server(ServerCmd::Create { slug, name }) => {
            let code = host.create_server(&slug, name.as_deref().unwrap_or(&slug)).await?;
            println!("Created server '{slug}'.");
            println!("Owner setup code: {code}");
            println!("Register on the server with this code to become its owner. Restart the host if it is running.");
            Ok(())
        }
        Cmd::User(UserCmd::ResetPassword { slug, username }) => {
            let srv = host.get(&slug).await.with_context(|| format!("no server '{slug}'"))?;
            let temp = db::new_setup_code().replace('-', "").to_lowercase() + &db::new_setup_code().replace('-', "").to_lowercase();
            let hash = api::hash_password(temp.clone()).await.map_err(|e| anyhow::anyhow!(e.message))?;
            let user: Option<i64> = sqlx::query_scalar("SELECT id FROM users WHERE username = ?").bind(username.trim().to_lowercase()).fetch_optional(&srv.db).await?;
            let id = user.with_context(|| format!("no user '{username}' on '{slug}'"))?;
            sqlx::query("UPDATE users SET password_hash = ? WHERE id = ?").bind(hash).bind(id).execute(&srv.db).await?;
            sqlx::query("DELETE FROM sessions WHERE user_id = ?").bind(id).execute(&srv.db).await?;
            println!("New temporary password for '{username}' on '{slug}': {temp}");
            println!("They should log in and change it in their settings.");
            Ok(())
        }
        Cmd::Server(ServerCmd::List) => {
            let servers = host.servers.read().await;
            if servers.is_empty() {
                println!("No servers yet. Create one with: chitchat-host server create <slug>");
            }
            let mut slugs: Vec<_> = servers.keys().collect();
            slugs.sort();
            for slug in slugs {
                let row = db::server_row(&servers[slug].db).await?;
                let owner = if row.owner_id.is_some() { "has owner" } else { "no owner yet" };
                println!("{slug:<20} {:<30} {owner}", row.name);
            }
            Ok(())
        }
    }
}

async fn run(host: std::sync::Arc<Host>, args: RunArgs) -> Result<()> {
    if host.servers.read().await.is_empty() {
        host.create_server("dev", "Dev Server").await?;
        tracing::info!("created the first server 'dev'");
    }
    {
        let servers = host.servers.read().await;
        let mut slugs: Vec<_> = servers.keys().cloned().collect();
        slugs.sort();
        for slug in slugs {
            let row = db::server_row(&servers[&slug].db).await?;
            match (&row.owner_id, &row.setup_code) {
                (None, Some(code)) => {
                    tracing::info!("server '{slug}' has no owner yet. Owner setup code: {code}");
                }
                _ => tracing::info!("server '{slug}' ready"),
            }
        }
    }
    if !args.web_dir.join("index.html").exists() {
        tracing::warn!("web client not found in {} (build it with `npm run build` in web/)", args.web_dir.display());
    }

    if media::ffmpeg().is_some() {
        tracing::info!("ffmpeg found: video metadata will be stripped and thumbnails made");
    } else {
        tracing::info!("ffmpeg not found: videos are stored as uploaded (install ffmpeg to strip their metadata)");
    }
    // The host admin (CPU/RAM limits) is the owner of the server created first.
    let mut first: Option<(i64, String)> = None;
    for (slug, srv) in host.servers.read().await.iter() {
        api::spawn_cleanup(srv.clone());
        let created = db::server_row(&srv.db).await?.created_at;
        if first.as_ref().is_none_or(|(c, s)| (created, slug) < (*c, s)) {
            first = Some((created, slug.clone()));
        }
    }
    if let Some((_, slug)) = first {
        api::start_resources(&host.data_dir, slug);
    }
    discovery::start(host.clone());
    push::init(&host.data_dir, args.canonical_url.as_deref()).context("setting up web push")?;
    let data_dir = host.data_dir.clone();
    let mut app = api::router(host, &args.web_dir);
    if let Some(url) = &args.canonical_url {
        app = api::with_canonical_redirect(app, url)?;
        tracing::info!("pages opened on {url}'s domain at another port are redirected to {url}");
    }
    let tls = match args.tls() {
        Some(t) => Some(tls::load(&t, &data_dir).await?),
        None => None,
    };
    let listener = std::net::TcpListener::bind(&args.bind).with_context(|| format!("can't listen on {}", args.bind))?;
    listener.set_nonblocking(true)?;
    let local = listener.local_addr()?;
    // Voice uses UDP on the same port number.
    match voice::start(local) {
        Ok(()) => tracing::info!("voice: UDP {local} (open this UDP port too for voice from other machines; networks that block UDP fall back to TCP on the same port)"),
        Err(e) => tracing::warn!("voice disabled: can't bind UDP {local}: {e}"),
    }
    let service = app.into_make_service_with_connect_info::<SocketAddr>();
    let handle = axum_server::Handle::new();
    let h = handle.clone();
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        tracing::info!("shutting down");
        h.graceful_shutdown(Some(std::time::Duration::from_secs(5)));
    });
    // Voice connections over TCP (ICE-TCP) share the port with the web server.
    let demux = voice::Demux(axum_server::accept::DefaultAcceptor);
    match tls {
        Some(tls) => {
            tracing::info!("ChitChat host listening on https://{local}");
            if !tls.names.is_empty() {
                tracing::info!(
                    "self-signed certificate for: {}. Browsers warn once (\"Advanced\" → continue); the certificate is in {}",
                    tls.names.join(", "),
                    data_dir.join("tls").display()
                );
            }
            let acceptor = axum_server::tls_rustls::RustlsAcceptor::new(tls.config).acceptor(demux);
            axum_server::from_tcp(listener).acceptor(acceptor).handle(handle).serve(service).await?;
        }
        None => {
            tracing::info!("ChitChat host listening on http://{local}");
            axum_server::from_tcp(listener).acceptor(demux).handle(handle).serve(service).await?;
        }
    }
    Ok(())
}
