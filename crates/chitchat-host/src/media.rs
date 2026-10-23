//! Uploaded images, videos and audio: type detection by content, re-encoding (which also strips metadata
//! such as GPS location), thumbnails, deduplicated storage by BLAKE3 hash, signed links and cleanup.

use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use image::{DynamicImage, ImageFormat, ImageReader, Limits};

/// Images larger than this on their longest side are scaled down.
const MAX_IMAGE_SIDE: u32 = 4096;
const THUMB_SIDE: u32 = 320;
const IMAGE_QUALITY: f32 = 85.0;
const THUMB_QUALITY: f32 = 70.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Sniffed {
    Png,
    Jpeg,
    Gif,
    Webp { animated: bool },
    Mp4,
    Webm,
    Mp3,
    Ogg,
    Wav,
    Flac,
}

/// Detects the real file type from its first bytes (the file name and Content-Type are not trusted).
pub fn sniff(b: &[u8]) -> Option<Sniffed> {
    if b.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(Sniffed::Png)
    } else if b.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some(Sniffed::Jpeg)
    } else if b.starts_with(b"GIF87a") || b.starts_with(b"GIF89a") {
        Some(Sniffed::Gif)
    } else if b.len() >= 21 && &b[0..4] == b"RIFF" && &b[8..12] == b"WEBP" {
        let animated = &b[12..16] == b"VP8X" && b[20] & 0x02 != 0;
        Some(Sniffed::Webp { animated })
    } else if b.len() >= 12 && &b[4..8] == b"ftyp" {
        Some(Sniffed::Mp4)
    } else if b.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
        Some(Sniffed::Webm)
    } else if b.starts_with(b"ID3") || (b.len() >= 2 && b[0] == 0xFF && b[1] & 0xE6 == 0xE2) {
        // An ID3 tag, or an MPEG audio layer III frame header.
        Some(Sniffed::Mp3)
    } else if b.starts_with(b"OggS") {
        Some(Sniffed::Ogg)
    } else if b.len() >= 12 && &b[0..4] == b"RIFF" && &b[8..12] == b"WAVE" {
        Some(Sniffed::Wav)
    } else if b.starts_with(b"fLaC") {
        Some(Sniffed::Flac)
    } else {
        None
    }
}

pub struct Processed {
    pub bytes: Vec<u8>,
    pub mime: &'static str,
    pub kind: &'static str,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub thumb: Option<Vec<u8>>,
    pub duration_ms: Option<i64>,
}

#[derive(Debug)]
pub enum MediaError {
    Unsupported,
    Broken,
}

fn decode(bytes: &[u8], format: ImageFormat) -> Result<DynamicImage, MediaError> {
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    let mut limits = Limits::default();
    limits.max_image_width = Some(16_384);
    limits.max_image_height = Some(16_384);
    limits.max_alloc = Some(512 * 1024 * 1024);
    reader.limits(limits);
    reader.decode().map_err(|_| MediaError::Broken)
}

fn encode_webp(img: &DynamicImage, quality: f32) -> Vec<u8> {
    let (w, h) = (img.width(), img.height());
    if img.color().has_alpha() {
        let rgba = img.to_rgba8();
        webp::Encoder::from_rgba(&rgba, w, h).encode(quality).to_vec()
    } else {
        let rgb = img.to_rgb8();
        webp::Encoder::from_rgb(&rgb, w, h).encode(quality).to_vec()
    }
}

fn thumbnail(img: &DynamicImage) -> Vec<u8> {
    encode_webp(&img.thumbnail(THUMB_SIDE, THUMB_SIDE), THUMB_QUALITY)
}

/// Avatars are always a still WebP, including the first frame of GIF/WebP animations.
/// Tighter decode limits keep small compressed images from allocating excessive memory.
pub fn process_avatar(bytes: &[u8]) -> Result<Vec<u8>, MediaError> {
    process_small_picture(bytes, 256)
}

pub fn process_emoji(bytes: &[u8]) -> Result<Vec<u8>, MediaError> {
    process_small_picture(bytes, 128)
}

fn process_small_picture(bytes: &[u8], side: u32) -> Result<Vec<u8>, MediaError> {
    let format = image::guess_format(bytes).map_err(|_| MediaError::Unsupported)?;
    if !matches!(format, ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::Gif | ImageFormat::WebP) {
        return Err(MediaError::Unsupported);
    }
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    let mut limits = Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    let mut img = reader.decode().map_err(|_| MediaError::Broken)?;
    if format == ImageFormat::Jpeg {
        img = apply_exif_orientation(img, bytes);
    }
    Ok(encode_webp(&img.thumbnail(side, side), IMAGE_QUALITY))
}

/// Processes an image (CPU heavy: call from `spawn_blocking`).
/// Still images are re-encoded as WebP: smaller files, and no hidden metadata survives.
/// Animated GIF/WebP are kept as they are (they carry no camera/GPS data) with a still thumbnail.
pub fn process_image(bytes: Vec<u8>, kind: Sniffed) -> Result<Processed, MediaError> {
    match kind {
        Sniffed::Gif | Sniffed::Webp { animated: true } => {
            let format = if kind == Sniffed::Gif { ImageFormat::Gif } else { ImageFormat::WebP };
            let first = decode(&bytes, format).ok();
            Ok(Processed {
                mime: if kind == Sniffed::Gif { "image/gif" } else { "image/webp" },
                kind: "image",
                width: first.as_ref().map(|i| i.width()),
                height: first.as_ref().map(|i| i.height()),
                thumb: first.as_ref().map(thumbnail),
                duration_ms: None,
                bytes,
            })
        }
        Sniffed::Png | Sniffed::Jpeg | Sniffed::Webp { animated: false } => {
            let format = match kind {
                Sniffed::Png => ImageFormat::Png,
                Sniffed::Jpeg => ImageFormat::Jpeg,
                _ => ImageFormat::WebP,
            };
            let mut img = decode(&bytes, format)?;
            if kind == Sniffed::Jpeg {
                img = apply_exif_orientation(img, &bytes);
            }
            if img.width().max(img.height()) > MAX_IMAGE_SIDE {
                img = img.resize(MAX_IMAGE_SIDE, MAX_IMAGE_SIDE, image::imageops::FilterType::Lanczos3);
            }
            Ok(Processed {
                bytes: encode_webp(&img, IMAGE_QUALITY),
                mime: "image/webp",
                kind: "image",
                width: Some(img.width()),
                height: Some(img.height()),
                thumb: Some(thumbnail(&img)),
                duration_ms: None,
            })
        }
        _ => Err(MediaError::Unsupported),
    }
}

/// Phone photos are often stored sideways with an EXIF "rotate me" flag. The flag is lost when
/// re-encoding, so the rotation is applied to the pixels first.
fn apply_exif_orientation(img: DynamicImage, jpeg: &[u8]) -> DynamicImage {
    match exif_orientation(jpeg) {
        Some(3) => img.rotate180(),
        Some(6) => img.rotate90(),
        Some(8) => img.rotate270(),
        Some(2) => img.fliph(),
        Some(4) => img.flipv(),
        Some(5) => img.rotate90().fliph(),
        Some(7) => img.rotate270().fliph(),
        _ => img,
    }
}

/// Reads the EXIF orientation tag (0x0112) from a JPEG's APP1 segment.
fn exif_orientation(b: &[u8]) -> Option<u16> {
    let mut i = 2;
    while i + 4 <= b.len() && b[i] == 0xFF {
        let marker = b[i + 1];
        let len = u16::from_be_bytes([b[i + 2], b[i + 3]]) as usize;
        if marker == 0xE1 && b.get(i + 4..i + 10) == Some(b"Exif\0\0") {
            let t = b.get(i + 10..i + 2 + len)?;
            let le = t.get(0..2)? == b"II";
            let rd16 = |o: usize| t.get(o..o + 2).map(|s| if le { u16::from_le_bytes([s[0], s[1]]) } else { u16::from_be_bytes([s[0], s[1]]) });
            let rd32 = |o: usize| t.get(o..o + 4).map(|s| if le { u32::from_le_bytes([s[0], s[1], s[2], s[3]]) } else { u32::from_be_bytes([s[0], s[1], s[2], s[3]]) });
            let ifd = rd32(4)? as usize;
            let n = rd16(ifd)? as usize;
            for k in 0..n.min(64) {
                let e = ifd + 2 + k * 12;
                if rd16(e)? == 0x0112 {
                    return rd16(e + 8);
                }
            }
            return None;
        }
        if marker == 0xDA {
            break;
        }
        i += 2 + len;
    }
    None
}

// ---------------------------------------------------------------- video (optional ffmpeg)

static FFMPEG: OnceLock<Option<(PathBuf, PathBuf)>> = OnceLock::new();

/// Finds `ffmpeg` and `ffprobe` on PATH once. Without them, videos are stored as uploaded.
pub fn ffmpeg() -> Option<&'static (PathBuf, PathBuf)> {
    FFMPEG
        .get_or_init(|| {
            let works = |name: &str| {
                std::process::Command::new(name).arg("-version").stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status().is_ok_and(|s| s.success())
            };
            (works("ffmpeg") && works("ffprobe")).then(|| (PathBuf::from("ffmpeg"), PathBuf::from("ffprobe")))
        })
        .as_ref()
}

/// What ffprobe says about a file: stream types and duration.
async fn probe(ffprobe: &Path, input: &Path) -> (Vec<String>, Option<i64>) {
    let out = tokio::process::Command::new(ffprobe).args(["-v", "error", "-show_entries", "stream=codec_type:format=duration", "-of", "default=nw=1"]).arg(input).output().await;
    let Ok(o) = out else { return (Vec::new(), None) };
    let text = String::from_utf8_lossy(&o.stdout);
    let mut types = Vec::new();
    let mut duration = None;
    for line in text.lines() {
        if let Some(t) = line.strip_prefix("codec_type=") {
            types.push(t.trim().to_string());
        } else if let Some(d) = line.strip_prefix("duration=") {
            duration = d.trim().parse::<f64>().ok().filter(|d| d.is_finite() && *d >= 0.0).map(|d| (d * 1000.0) as i64);
        }
    }
    (types, duration)
}

/// The audio container a sniffed type is stored as: (mime, extension).
fn audio_format(kind: Sniffed) -> (&'static str, &'static str) {
    match kind {
        Sniffed::Mp3 => ("audio/mpeg", "mp3"),
        Sniffed::Ogg => ("audio/ogg", "ogg"),
        Sniffed::Wav => ("audio/wav", "wav"),
        Sniffed::Flac => ("audio/flac", "flac"),
        Sniffed::Webm => ("audio/webm", "webm"),
        _ => ("audio/mp4", "m4a"),
    }
}

pub fn is_audio(kind: Sniffed) -> bool {
    matches!(kind, Sniffed::Mp3 | Sniffed::Ogg | Sniffed::Wav | Sniffed::Flac)
}

/// MP4 or WebM: a video, or (with no video stream, e.g. a recorded voice message or an M4A) audio.
/// Without ffmpeg, only `audio_hint` (a voice message upload) tells them apart.
pub async fn process_av(bytes: Vec<u8>, kind: Sniffed, tmp_dir: &Path, audio_hint: bool) -> Result<Processed, MediaError> {
    let Some((_, ffprobe)) = ffmpeg() else {
        return Ok(if audio_hint { stored_as_audio(bytes, kind) } else { process_video(bytes, kind, tmp_dir).await });
    };
    let id = crate::ids::next_id();
    let input = tmp_dir.join(format!("{id}-probe"));
    if tokio::fs::write(&input, &bytes).await.is_err() {
        return Ok(process_video(bytes, kind, tmp_dir).await);
    }
    let (types, _) = probe(ffprobe, &input).await;
    let _ = tokio::fs::remove_file(&input).await;
    if !types.iter().any(|t| t == "video") && types.iter().any(|t| t == "audio") {
        process_audio(bytes, kind, tmp_dir, audio_hint).await
    } else {
        Ok(process_video(bytes, kind, tmp_dir).await)
    }
}

fn stored_as_audio(bytes: Vec<u8>, kind: Sniffed) -> Processed {
    Processed { bytes, mime: audio_format(kind).0, kind: "audio", width: None, height: None, thumb: None, duration_ms: None }
}

/// Keeps only the sound (no tags, no cover picture, no hidden metadata) without re-encoding it.
/// With ffmpeg, a file it can't read is refused; without it, audio is stored as uploaded.
///
/// Voice messages are re-encoded with timestamps counted from the samples: browsers sometimes
/// record them starting minutes in (e.g. while the mic is also in a voice call), which made a
/// 2-second message claim to last 4 minutes.
pub async fn process_audio(bytes: Vec<u8>, kind: Sniffed, tmp_dir: &Path, voice: bool) -> Result<Processed, MediaError> {
    let Some((ffmpeg, ffprobe)) = ffmpeg() else { return Ok(stored_as_audio(bytes, kind)) };
    let (mime, ext) = audio_format(kind);
    let id = crate::ids::next_id();
    let input = tmp_dir.join(format!("{id}-in.{ext}"));
    let cleaned = tmp_dir.join(format!("{id}-out.{ext}"));
    if tokio::fs::write(&input, &bytes).await.is_err() {
        return Ok(stored_as_audio(bytes, kind));
    }
    let mut remux = tokio::process::Command::new(ffmpeg);
    remux.args(["-y", "-loglevel", "error", "-i"]).arg(&input).args(["-map", "0:a:0", "-map_metadata", "-1"]);
    match (voice, ext) {
        (true, "m4a") => remux.args(["-af", "asetpts=N/SR/TB", "-c:a", "aac", "-b:a", "64k"]),
        (true, "webm" | "ogg") => remux.args(["-af", "asetpts=N/SR/TB", "-c:a", "libopus", "-b:a", "48k"]),
        _ => remux.args(["-c", "copy"]),
    };
    if ext == "m4a" {
        remux.args(["-movflags", "+faststart"]);
    }
    let ok = remux.arg(&cleaned).status().await.is_ok_and(|s| s.success());
    let out = if ok { tokio::fs::read(&cleaned).await.ok().filter(|b| !b.is_empty()) } else { None };
    let duration = match &out {
        Some(_) => probe(ffprobe, &cleaned).await.1,
        None => None,
    };
    for p in [&input, &cleaned] {
        let _ = tokio::fs::remove_file(p).await;
    }
    let bytes = out.ok_or(MediaError::Broken)?;
    Ok(Processed { bytes, mime, kind: "audio", width: None, height: None, thumb: None, duration_ms: duration })
}

/// Strips metadata (e.g. phone GPS) from a video without re-encoding it, makes MP4s start playing
/// before they finish downloading, and grabs a thumbnail. Falls back to the original on any problem.
pub async fn process_video(bytes: Vec<u8>, kind: Sniffed, tmp_dir: &Path) -> Processed {
    let (mime, ext) = if kind == Sniffed::Webm { ("video/webm", "webm") } else { ("video/mp4", "mp4") };
    let mut out = Processed { bytes, mime, kind: "video", width: None, height: None, thumb: None, duration_ms: None };
    let Some((ffmpeg, ffprobe)) = ffmpeg() else { return out };
    let id = crate::ids::next_id();
    let input = tmp_dir.join(format!("{id}-in.{ext}"));
    let cleaned = tmp_dir.join(format!("{id}-out.{ext}"));
    let thumb = tmp_dir.join(format!("{id}-thumb.png"));
    if tokio::fs::write(&input, &out.bytes).await.is_err() {
        return out;
    }

    let mut remux = tokio::process::Command::new(ffmpeg);
    remux.args(["-y", "-loglevel", "error", "-i"]).arg(&input).args(["-map", "0", "-map_metadata", "-1", "-c", "copy"]);
    if ext == "mp4" {
        remux.args(["-movflags", "+faststart"]);
    }
    if remux.arg(&cleaned).status().await.is_ok_and(|s| s.success())
        && let Ok(b) = tokio::fs::read(&cleaned).await
        && !b.is_empty()
    {
        out.bytes = b;
    }

    let size = tokio::process::Command::new(ffprobe)
        .args(["-v", "error", "-select_streams", "v:0", "-show_entries", "stream=width,height", "-of", "csv=p=0:s=x"])
        .arg(&input)
        .output()
        .await;
    if let Ok(o) = size {
        let text = String::from_utf8_lossy(&o.stdout);
        if let Some((w, h)) = text.trim().split_once('x') {
            out.width = w.trim().parse().ok();
            out.height = h.trim().parse().ok();
        }
    }

    out.duration_ms = probe(ffprobe, &input).await.1;
    let grabbed = tokio::process::Command::new(ffmpeg)
        .args(["-y", "-loglevel", "error", "-ss", "0.5", "-i"])
        .arg(&input)
        .args(["-frames:v", "1", "-vf", "scale=640:-2"])
        .arg(&thumb)
        .status()
        .await;
    if grabbed.is_ok_and(|s| s.success())
        && let Ok(png) = tokio::fs::read(&thumb).await
    {
        out.thumb = tokio::task::spawn_blocking(move || decode(&png, ImageFormat::Png).ok().map(|i| thumbnail(&i))).await.ok().flatten();
    }
    for p in [&input, &cleaned, &thumb] {
        let _ = tokio::fs::remove_file(p).await;
    }
    out
}

// ---------------------------------------------------------------- storage

/// Where a blob lives: `files/ab/abcdef…`.
pub fn blob_path(files_dir: &Path, hash: &str) -> PathBuf {
    files_dir.join(&hash[0..2]).join(hash)
}

/// Saves bytes under their BLAKE3 hash (identical files are stored once). Returns the hash.
pub async fn store(files_dir: &Path, bytes: &[u8]) -> std::io::Result<String> {
    let hash = blake3::hash(bytes).to_hex().to_string();
    let path = blob_path(files_dir, &hash);
    if tokio::fs::try_exists(&path).await.unwrap_or(false) {
        return Ok(hash);
    }
    tokio::fs::create_dir_all(path.parent().unwrap()).await?;
    let tmp = files_dir.join("tmp").join(format!("{}.part", crate::ids::next_id()));
    tokio::fs::create_dir_all(tmp.parent().unwrap()).await?;
    tokio::fs::write(&tmp, bytes).await?;
    tokio::fs::rename(&tmp, &path).await?;
    Ok(hash)
}

// ---------------------------------------------------------------- signed links

/// Links stay valid until the end of the day after tomorrow (UTC), so browsers can cache them.
pub fn link_expiry(now_ms: i64) -> i64 {
    let day = 86_400;
    (now_ms / 1000 / day + 3) * day
}

pub fn sign(key: &[u8; 32], attachment_id: i64, variant: &str, exp: i64) -> String {
    let mut h = blake3::Hasher::new_keyed(key);
    h.update(format!("{attachment_id}:{variant}:{exp}").as_bytes());
    h.finalize().to_hex()[..32].to_string()
}

pub fn verify(key: &[u8; 32], attachment_id: i64, variant: &str, exp: i64, sig: &str, now_ms: i64) -> bool {
    exp * 1000 > now_ms && constant_eq(sign(key, attachment_id, variant, exp).as_bytes(), sig.as_bytes())
}

fn constant_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// A file name that is safe to put in a URL path and a download header.
pub fn clean_filename(raw: &str, fallback_ext: &str) -> String {
    let base = raw.rsplit(['/', '\\']).next().unwrap_or("");
    let mut name: String = base.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') { c } else { '_' }).take(80).collect();
    name = name.trim_matches(['.', '_']).to_string();
    if name.is_empty() {
        name = "file".into();
    }
    // Use the extension of what we actually store (e.g. photo.jpg → photo.webp).
    let stem = name.rsplit_once('.').map(|(s, _)| s.to_string()).unwrap_or(name);
    format!("{stem}.{fallback_ext}")
}

/// Documents people may share as downloads: UTF-8 text files and PDFs, by extension and checked
/// content. Returns (mime, extension). They're stored as uploaded and only ever served as downloads.
pub fn document(filename: &str, b: &[u8]) -> Option<(&'static str, &'static str)> {
    let ext = filename.rsplit_once('.')?.1.to_ascii_lowercase();
    let text = || std::str::from_utf8(b).is_ok() && !b.contains(&0);
    match ext.as_str() {
        "txt" if text() => Some(("text/plain", "txt")),
        "md" if text() => Some(("text/plain", "md")),
        "csv" if text() => Some(("text/plain", "csv")),
        "log" if text() => Some(("text/plain", "log")),
        "json" if text() => Some(("text/plain", "json")),
        "pdf" if b.starts_with(b"%PDF-") => Some(("application/pdf", "pdf")),
        _ => None,
    }
}

pub fn ext_for(mime: &str) -> &'static str {
    match mime {
        "image/webp" => "webp",
        "image/gif" => "gif",
        "video/webm" | "audio/webm" => "webm",
        "audio/mpeg" => "mp3",
        "audio/ogg" => "ogg",
        "audio/wav" => "wav",
        "audio/flac" => "flac",
        "audio/mp4" => "m4a",
        _ => "mp4",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn documents() {
        assert_eq!(document("notes.TXT", "héllo".as_bytes()), Some(("text/plain", "txt")));
        assert_eq!(document("a.pdf", b"%PDF-1.7 ..."), Some(("application/pdf", "pdf")));
        assert_eq!(document("a.pdf", b"MZ..."), None);
        assert_eq!(document("a.txt", b"bin\0ary"), None);
        assert_eq!(document("a.txt", &[0xff, 0xfe]), None);
        assert_eq!(document("run.exe", b"hello"), None);
        assert_eq!(document("page.html", b"<script>"), None);
        assert_eq!(document("noext", b"hello"), None);
    }

    #[test]
    fn sniffing() {
        assert_eq!(sniff(b"\x89PNG\r\n\x1a\nrest"), Some(Sniffed::Png));
        assert_eq!(sniff(&[0xFF, 0xD8, 0xFF, 0xE0]), Some(Sniffed::Jpeg));
        assert_eq!(sniff(b"GIF89a...."), Some(Sniffed::Gif));
        assert_eq!(sniff(b"\0\0\0\x18ftypmp42\0\0\0\0"), Some(Sniffed::Mp4));
        assert_eq!(sniff(b"<html>"), None);
        assert_eq!(sniff(b"MZ\x90\0"), None);
    }

    #[test]
    fn png_becomes_small_webp_with_thumb() {
        let img = DynamicImage::ImageRgb8(image::RgbImage::from_fn(5000, 100, |x, _| image::Rgb([(x % 256) as u8, 0, 0])));
        let mut png = Vec::new();
        img.write_to(&mut Cursor::new(&mut png), ImageFormat::Png).unwrap();
        let out = process_image(png, Sniffed::Png).unwrap();
        assert_eq!(out.mime, "image/webp");
        assert_eq!(out.width, Some(4096));
        assert!(sniff(&out.bytes) == Some(Sniffed::Webp { animated: false }));
        assert!(out.thumb.is_some());
        assert!(matches!(process_image(b"\x89PNG\r\n\x1a\ngarbage".to_vec(), Sniffed::Png), Err(MediaError::Broken)));
    }

    #[test]
    fn signatures_and_names() {
        let key = [7u8; 32];
        let exp = link_expiry(1_000_000_000_000);
        let sig = sign(&key, 42, "file", exp);
        assert!(verify(&key, 42, "file", exp, &sig, 1_000_000_000_000));
        assert!(!verify(&key, 43, "file", exp, &sig, 1_000_000_000_000));
        assert!(!verify(&key, 42, "thumb", exp, &sig, 1_000_000_000_000));
        assert!(!verify(&key, 42, "file", exp, &sig, exp * 1000 + 1));
        assert_eq!(clean_filename("C:\\Users\\me\\My Photo.JPG", "webp"), "My_Photo.webp");
        assert_eq!(clean_filename("../../etc/passwd", "webp"), "passwd.webp");
        assert_eq!(clean_filename("", "mp4"), "file.mp4");
    }
}
