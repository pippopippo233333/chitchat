//! HTTPS for the host: the owner's own certificate files (Let's Encrypt, `tailscale cert`…),
//! or a self-signed certificate made on first run (also served next to the owner's one for IP/LAN links).
//! Browsers only allow the microphone and screen sharing on HTTPS pages (or localhost), so voice from other machines needs this.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use axum_server::tls_rustls::RustlsConfig;
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::server::{ClientHello, ResolvesServerCert};
use rustls::sign::CertifiedKey;

pub enum Tls {
    Files {
        cert: PathBuf,
        key: PathBuf,
    },
    /// Extra names (a domain, a Tailscale name) besides this machine's own names and IPs.
    SelfSigned {
        extra_names: Vec<String>,
    },
}

pub struct Loaded {
    pub config: RustlsConfig,
    /// For the startup log: the names the certificate covers (self-signed only).
    pub names: Vec<String>,
}

pub async fn load(tls: &Tls, data_dir: &Path) -> Result<Loaded> {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    match tls {
        Tls::Files { cert, key } => {
            let main =
                certified_key(&std::fs::read(cert)?, &std::fs::read(key)?).with_context(|| format!("can't load the certificate {} / key {}", cert.display(), key.display()))?;
            // A domain certificate doesn't cover this machine's IPs and LAN names, so those
            // get the self-signed one instead (links like https://192.168.1.10:8090 keep working).
            let (lcert, lkey, names) = self_signed(&data_dir.join("tls"), &[])?;
            let resolver = BySni { main, local: certified_key(&lcert, &lkey)?, local_names: names.iter().cloned().collect() };
            let mut config = rustls::ServerConfig::builder().with_no_client_auth().with_cert_resolver(Arc::new(resolver));
            config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
            Ok(Loaded { config: RustlsConfig::from_config(Arc::new(config)), names })
        }
        Tls::SelfSigned { extra_names } => {
            let (cert, key, names) = self_signed(&data_dir.join("tls"), extra_names)?;
            Ok(Loaded { config: RustlsConfig::from_pem(cert, key).await?, names })
        }
    }
}

fn certified_key(cert: &[u8], key: &[u8]) -> Result<Arc<CertifiedKey>> {
    let chain = CertificateDer::pem_slice_iter(cert).collect::<Result<Vec<_>, _>>()?;
    anyhow::ensure!(!chain.is_empty(), "no certificate in the file");
    let key = PrivateKeyDer::from_pem_slice(key)?;
    let provider = rustls::crypto::CryptoProvider::get_default().context("no TLS crypto provider")?;
    Ok(Arc::new(CertifiedKey::new(chain, provider.key_provider.load_private_key(key)?)))
}

/// Picks the certificate by the name the browser asked for: this machine's own names and IPs
/// (IP links send no name at all) get the self-signed one, anything else the owner's one.
#[derive(Debug)]
struct BySni {
    main: Arc<CertifiedKey>,
    local: Arc<CertifiedKey>,
    local_names: BTreeSet<String>,
}

impl ResolvesServerCert for BySni {
    fn resolve(&self, hello: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        match hello.server_name() {
            Some(name) if !self.local_names.contains(&name.to_ascii_lowercase()) => Some(self.main.clone()),
            _ => Some(self.local.clone()),
        }
    }
}

fn hostname() -> Option<String> {
    std::env::var("COMPUTERNAME").ok().or_else(|| std::fs::read_to_string("/etc/hostname").ok()).map(|h| h.trim().to_lowercase()).filter(|h| !h.is_empty())
}

/// This machine's names and addresses.
fn own_names() -> BTreeSet<String> {
    let mut names = BTreeSet::from(["localhost".to_string()]);
    if let Some(h) = hostname() {
        names.insert(format!("{h}.local"));
        names.insert(h);
    }
    for iface in if_addrs::get_if_addrs().unwrap_or_default() {
        let ip = iface.ip();
        let link_local_v6 = matches!(ip, std::net::IpAddr::V6(v6) if (v6.segments()[0] & 0xffc0) == 0xfe80);
        if !link_local_v6 {
            names.insert(ip.to_string());
        }
    }
    names
}

/// Reuses `data/tls/self-signed.*` unless this machine gained a name or address it doesn't
/// cover (a new certificate means browsers ask to accept it again).
fn self_signed(dir: &Path, extra: &[String]) -> Result<(Vec<u8>, Vec<u8>, Vec<String>)> {
    let (cert_path, key_path, names_path) = (dir.join("self-signed.crt"), dir.join("self-signed.key"), dir.join("names.txt"));
    let mut wanted = own_names();
    wanted.extend(extra.iter().map(|n| n.trim().to_lowercase()).filter(|n| !n.is_empty()));
    if let (Ok(cert), Ok(key), Ok(stored)) = (std::fs::read(&cert_path), std::fs::read(&key_path), std::fs::read_to_string(&names_path)) {
        let stored: BTreeSet<String> = stored.lines().map(str::to_string).collect();
        if wanted.is_subset(&stored) {
            return Ok((cert, key, stored.into_iter().collect()));
        }
        // Keep covering the old names too, so earlier links keep working.
        wanted.extend(stored);
    }
    let names: Vec<String> = wanted.into_iter().collect();
    let certified = rcgen::generate_simple_self_signed(names.clone()).context("making a self-signed certificate")?;
    let (cert, key) = (certified.cert.pem(), certified.key_pair.serialize_pem());
    std::fs::create_dir_all(dir)?;
    std::fs::write(&cert_path, &cert)?;
    std::fs::write(&key_path, &key)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600))?;
    }
    std::fs::write(&names_path, names.join("\n"))?;
    Ok((cert.into_bytes(), key.into_bytes(), names))
}
