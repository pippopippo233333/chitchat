//! Web Push: notifications that reach a phone or computer while ChitChat isn't open.
//!
//! The host signs each request with its own key (VAPID, RFC 8292) and encrypts the content for
//! the receiving browser (RFC 8291), then hands it to that browser's push service (Google,
//! Mozilla, Apple, Microsoft). Those services can't read it, and no ChitChat server sits in
//! between. Only those services' addresses are ever contacted.

use std::path::Path;
use std::sync::OnceLock;
use std::time::Duration;

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes128Gcm, Nonce};
use anyhow::{Context, Result, anyhow, bail};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use hkdf::Hkdf;
use p256::ecdsa::signature::Signer;
use p256::ecdsa::{Signature, SigningKey};
use p256::elliptic_curve::sec1::ToEncodedPoint;
use p256::{PublicKey, SecretKey};
use rand::RngCore;
use sha2::Sha256;

/// Encrypted records are padded into one record of up to this size.
const RECORD_SIZE: u32 = 4096;
/// Longest payload we send (well under what push services accept).
pub const MAX_PAYLOAD: usize = 3000;

struct Vapid {
    key: SigningKey,
    /// Uncompressed public key, base64url: what browsers subscribe with.
    public: String,
    /// The `sub` claim: how push services can reach whoever runs this host.
    subject: String,
}

static VAPID: OnceLock<Vapid> = OnceLock::new();
static HTTP: OnceLock<reqwest::Client> = OnceLock::new();

/// Loads (or creates) this host's push key in `<data>/push-vapid.key`. `public_url` is the
/// host's public address, used as the contact for push services.
pub fn init(data_dir: &Path, public_url: Option<&str>) -> Result<()> {
    let file = data_dir.join("push-vapid.key");
    let secret = match std::fs::read_to_string(&file) {
        Ok(text) => SecretKey::from_slice(&B64.decode(text.trim()).context("reading push-vapid.key")?).context("push-vapid.key is not a valid key")?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let secret = SecretKey::random(&mut rand::rngs::OsRng);
            write_private(&file, &B64.encode(secret.to_bytes()))?;
            secret
        }
        Err(e) => return Err(e).context("reading push-vapid.key"),
    };
    let public = B64.encode(secret.public_key().to_encoded_point(false).as_bytes());
    let subject = public_url.filter(|u| u.starts_with("https://")).unwrap_or("https://github.com/pippopippo233333/chitchat").to_string();
    let _ = VAPID.set(Vapid { key: SigningKey::from(secret), public, subject });
    Ok(())
}

fn write_private(file: &Path, text: &str) -> Result<()> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(file)?;
        f.write_all(text.as_bytes())?;
    }
    #[cfg(not(unix))]
    std::fs::write(file, text)?;
    Ok(())
}

/// The key browsers subscribe with, or None before `init`.
pub fn public_key() -> Option<&'static str> {
    VAPID.get().map(|v| v.public.as_str())
}

/// Push services we deliver to. Anything else is refused, so a subscription can't make the
/// host send requests to arbitrary (or private) addresses.
pub fn allowed_endpoint(endpoint: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(endpoint) else { return false };
    let Some(host) = url.host_str() else { return false };
    url.scheme() == "https"
        && url.port().is_none()
        && url.username().is_empty()
        && url.password().is_none()
        && endpoint.len() <= 1024
        && (host == "fcm.googleapis.com" || host == "updates.push.services.mozilla.com" || host.ends_with(".push.apple.com") || host.ends_with(".notify.windows.com"))
}

/// RFC 8291 `aes128gcm` body for a browser's keys (`p256dh` and `auth`, base64url).
pub fn encrypt(payload: &[u8], p256dh: &str, auth: &str) -> Result<Vec<u8>> {
    let mut salt = [0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut salt);
    let sender = SecretKey::random(&mut rand::rngs::OsRng);
    encrypt_with(payload, p256dh, auth, &sender, salt)
}

fn encrypt_with(payload: &[u8], p256dh: &str, auth: &str, sender: &SecretKey, salt: [u8; 16]) -> Result<Vec<u8>> {
    let receiver_bytes = B64.decode(p256dh.trim_end_matches('=')).context("p256dh")?;
    let receiver = PublicKey::from_sec1_bytes(&receiver_bytes).map_err(|_| anyhow!("p256dh is not a P-256 key"))?;
    let auth = B64.decode(auth.trim_end_matches('=')).context("auth")?;
    if auth.len() != 16 || payload.len() > MAX_PAYLOAD {
        bail!("bad auth secret or payload too large");
    }
    let sender_public = sender.public_key().to_encoded_point(false);
    let shared = p256::ecdh::diffie_hellman(sender.to_nonzero_scalar(), receiver.as_affine());

    // IKM = HKDF(auth, ECDH secret, "WebPush: info" || receiver key || sender key)
    let mut info = b"WebPush: info\0".to_vec();
    info.extend_from_slice(receiver.to_encoded_point(false).as_bytes());
    info.extend_from_slice(sender_public.as_bytes());
    let mut ikm = [0u8; 32];
    Hkdf::<Sha256>::new(Some(&auth), shared.raw_secret_bytes()).expand(&info, &mut ikm).map_err(|_| anyhow!("hkdf"))?;

    let prk = Hkdf::<Sha256>::new(Some(&salt), &ikm);
    let mut cek = [0u8; 16];
    let mut nonce = [0u8; 12];
    prk.expand(b"Content-Encoding: aes128gcm\0", &mut cek).map_err(|_| anyhow!("hkdf"))?;
    prk.expand(b"Content-Encoding: nonce\0", &mut nonce).map_err(|_| anyhow!("hkdf"))?;

    // One record: the payload, then the last-record delimiter 0x02.
    let mut plain = payload.to_vec();
    plain.push(2);
    let sealed = Aes128Gcm::new_from_slice(&cek).map_err(|_| anyhow!("aes key"))?.encrypt(Nonce::from_slice(&nonce), plain.as_slice()).map_err(|_| anyhow!("aes"))?;

    let mut body = Vec::with_capacity(16 + 4 + 1 + 65 + sealed.len());
    body.extend_from_slice(&salt);
    body.extend_from_slice(&RECORD_SIZE.to_be_bytes());
    body.push(sender_public.as_bytes().len() as u8);
    body.extend_from_slice(sender_public.as_bytes());
    body.extend_from_slice(&sealed);
    Ok(body)
}

/// RFC 8292 `Authorization: vapid t=…, k=…` for a push service.
fn authorization(endpoint: &str) -> Result<String> {
    let v = VAPID.get().ok_or_else(|| anyhow!("push not initialized"))?;
    let url = reqwest::Url::parse(endpoint)?;
    let audience = url.origin().ascii_serialization();
    let exp = chrono::Utc::now().timestamp() + 12 * 3600;
    let header = B64.encode(br#"{"typ":"JWT","alg":"ES256"}"#);
    let claims = B64.encode(serde_json::json!({ "aud": audience, "exp": exp, "sub": v.subject }).to_string());
    let input = format!("{header}.{claims}");
    let signature: Signature = v.key.sign(input.as_bytes());
    Ok(format!("vapid t={input}.{}, k={}", B64.encode(signature.to_bytes()), v.public))
}

pub enum Outcome {
    Sent,
    /// The subscription no longer exists (the browser unsubscribed): forget it.
    Gone,
    Failed,
}

/// Delivers one encrypted payload. `topic` lets a newer push replace an undelivered one.
pub async fn send(endpoint: &str, p256dh: &str, auth: &str, payload: &[u8], topic: &str, urgent: bool) -> Outcome {
    if !allowed_endpoint(endpoint) {
        return Outcome::Gone;
    }
    let attempt = async {
        let body = encrypt(payload, p256dh, auth)?;
        let http = HTTP.get_or_init(|| reqwest::Client::builder().timeout(Duration::from_secs(15)).redirect(reqwest::redirect::Policy::none()).build().unwrap_or_default());
        let res = http
            .post(endpoint)
            .header("Authorization", authorization(endpoint)?)
            .header("Content-Encoding", "aes128gcm")
            .header("Content-Type", "application/octet-stream")
            .header("TTL", "86400")
            .header("Urgency", if urgent { "high" } else { "normal" })
            .header("Topic", topic)
            .body(body)
            .send()
            .await?;
        anyhow::Ok(res.status())
    };
    match attempt.await {
        Ok(s) if s.is_success() => Outcome::Sent,
        Ok(s) if s.as_u16() == 404 || s.as_u16() == 410 => Outcome::Gone,
        Ok(s) => {
            tracing::warn!(status = %s, "push service refused a notification");
            Outcome::Failed
        }
        Err(e) => {
            tracing::warn!("push failed: {e:#}");
            Outcome::Failed
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode(s: &str) -> Vec<u8> {
        B64.decode(s).unwrap()
    }

    /// The worked example in RFC 8291, section 5.
    #[test]
    fn rfc8291_example() {
        let sender = SecretKey::from_slice(&decode("yfWPiYE-n46HLnH0KqZOF1fJJU3MYrct3AELtAQ-oRw")).unwrap();
        let salt: [u8; 16] = decode("DGv6ra1nlYgDCS1FRnbzlw").try_into().unwrap();
        let body = encrypt_with(
            b"When I grow up, I want to be a watermelon",
            "BCVxsr7N_eNgVRqvHtD0zTZsEc6-VV-JvLexhqUzORcxaOzi6-AYWXvTBHm4bjyPjs7Vd8pZGH6SRpkNtoIAiw4",
            "BTBZMqHH6r4Tts7J_aSIgg",
            &sender,
            salt,
        )
        .unwrap();
        assert_eq!(
            B64.encode(body),
            "DGv6ra1nlYgDCS1FRnbzlwAAEABBBP4z9KsN6nGRTbVYI_c7VJSPQTBtkgcy27mlmlMoZIIgDll6e3vCYLocInmYWAmS6TlzAC8wEqKK6PBru3jl7A_yl95bQpu6cVPTpK4Mqgkf1CXztLVBSt2Ks3oZwbuwXPXLWyouBWLVWGNWQexSgSxsj_Qulcy4a-fN"
        );
    }

    #[test]
    fn endpoints() {
        assert!(allowed_endpoint("https://fcm.googleapis.com/fcm/send/abc"));
        assert!(allowed_endpoint("https://updates.push.services.mozilla.com/wpush/v2/abc"));
        assert!(allowed_endpoint("https://web.push.apple.com/QGm"));
        assert!(allowed_endpoint("https://wns2-par02p.notify.windows.com/w/?token=x"));
        assert!(!allowed_endpoint("http://fcm.googleapis.com/fcm/send/abc"));
        assert!(!allowed_endpoint("https://fcm.googleapis.com:8443/x"));
        assert!(!allowed_endpoint("https://evil.example/fcm.googleapis.com"));
        assert!(!allowed_endpoint("https://fcm.googleapis.com.evil.example/x"));
        assert!(!allowed_endpoint("https://user@fcm.googleapis.com/x"));
        assert!(!allowed_endpoint("https://127.0.0.1/x"));
    }

    #[test]
    fn vapid_header() {
        let dir = std::env::temp_dir().join(format!("chitchat-push-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        init(&dir, Some("https://chat.example.org")).unwrap();
        let h = authorization("https://fcm.googleapis.com/fcm/send/abc").unwrap();
        let (t, k) = h.strip_prefix("vapid t=").unwrap().split_once(", k=").unwrap();
        assert_eq!(k, public_key().unwrap());
        let parts: Vec<&str> = t.split('.').collect();
        let claims: serde_json::Value = serde_json::from_slice(&decode(parts[1])).unwrap();
        assert_eq!(claims["aud"], "https://fcm.googleapis.com");
        assert_eq!(claims["sub"], "https://chat.example.org");
        // The signature verifies with the advertised public key.
        use p256::ecdsa::VerifyingKey;
        use p256::ecdsa::signature::Verifier;
        let key = VerifyingKey::from_sec1_bytes(&decode(k)).unwrap();
        let sig = Signature::from_slice(&decode(parts[2])).unwrap();
        assert!(key.verify(format!("{}.{}", parts[0], parts[1]).as_bytes(), &sig).is_ok());
        let _ = std::fs::remove_dir_all(dir);
    }
}
