//! Shared application state, authentication helpers and error type.
use std::{collections::HashMap, net::IpAddr, path::PathBuf, sync::Arc, sync::Mutex, time::Instant};

use argon2::{
    Argon2,
    password_hash::{PasswordHasher, PasswordVerifier},
};
use axum::{
    Json,
    extract::FromRequestParts,
    http::{HeaderMap, StatusCode, header, request::Parts},
    response::{IntoResponse, Response},
};
use rand::RngExt;
use sha2::{Digest, Sha256};
use tt_core::{profiles::DeviceProfile, tunnel::RunningTunnel, vault::Vault};

use crate::db::{Db, User};

pub const COOKIE: &str = "tt_session";
pub const SESSION_SECS: i64 = 7 * 24 * 3600;

pub struct AppState {
    pub db: Db,
    pub vault: Vault,
    pub profiles: Vec<DeviceProfile>,
    pub data_dir: PathBuf,
    /// Set in local mode: one-time token that logs the local browser in.
    pub local_token: Option<String>,
    pub secure_cookies: bool,
    pub attempts: Mutex<HashMap<IpAddr, Attempts>>,
    /// Valid hash verified for unknown users so timing doesn't reveal account existence.
    pub dummy_hash: String,
    /// Allow tunnels to listen on non-loopback addresses (server admin decision).
    pub allow_public_tunnels: bool,
    pub tunnels: tokio::sync::Mutex<HashMap<i64, RunningTunnel>>,
    pub live: Mutex<HashMap<u64, Arc<LiveSession>>>,
    pub next_live: std::sync::atomic::AtomicU64,
}

/// An open interactive terminal: its recent output (for AI/MCP) and a channel to push suggestions.
pub struct LiveSession {
    pub id: u64,
    pub user_id: i64,
    pub host_id: i64,
    pub host_name: String,
    pub profile_id: String,
    pub buffer: Mutex<String>,
    pub push: tokio::sync::mpsc::UnboundedSender<serde_json::Value>,
}

impl LiveSession {
    pub fn append(&self, text: &str) {
        let mut b = self.buffer.lock().unwrap_or_else(|e| e.into_inner());
        b.push_str(text);
        if b.len() > 65536 {
            let cut = b.len() - 49152;
            let cut = (cut..b.len()).find(|i| b.is_char_boundary(*i)).unwrap_or(b.len());
            b.drain(..cut);
        }
    }

    /// Last `lines` lines of output with ANSI escapes removed and secrets masked.
    pub fn tail_clean(&self, lines: usize) -> String {
        let raw = self.buffer.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let clean = strip_ansi(&raw);
        let all: Vec<&str> = clean.lines().collect();
        let start = all.len().saturating_sub(lines);
        tt_core::redact::redact(&all[start..].join("\n"))
    }
}

pub fn strip_ansi(s: &str) -> String {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| regex::Regex::new(r"\x1b\[[0-?]*[ -/]*[@-~]|\x1b\][^\x07\x1b]*(\x07|\x1b\\)|\x1b[()][A-Za-z0-9]|\x1b[=>MD78]|\r").expect("regex"));
    re.replace_all(s, "").into_owned()
}

pub type Shared = Arc<AppState>;

#[derive(Default)]
pub struct Attempts {
    pub count: u32,
    pub window_start: Option<Instant>,
    pub locked_until: Option<Instant>,
}

pub struct ApiError(pub StatusCode, pub String);

impl ApiError {
    pub fn bad(msg: impl Into<String>) -> Self {
        Self(StatusCode::BAD_REQUEST, msg.into())
    }
    pub fn forbidden() -> Self {
        Self(StatusCode::FORBIDDEN, "forbidden".into())
    }
    pub fn not_found() -> Self {
        Self(StatusCode::NOT_FOUND, "not found".into())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(serde_json::json!({ "error": self.1 }))).into_response()
    }
}

impl<E: Into<anyhow::Error>> From<E> for ApiError {
    fn from(e: E) -> Self {
        let e: anyhow::Error = e.into();
        tracing::warn!("request failed: {e:#}");
        Self(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}"))
    }
}

pub type ApiResult<T> = Result<T, ApiError>;

pub fn hash_password(pw: &str) -> anyhow::Result<String> {
    Ok(Argon2::default()
        .hash_password(pw.as_bytes())
        .map_err(|e| anyhow::anyhow!("hash error: {e}"))?
        .to_string())
}

pub fn verify_password(pw: &str, hash: &str) -> bool {
    Argon2::default().verify_password(pw.as_bytes(), hash).is_ok()
}

pub fn validate_username(u: &str) -> Result<(), ApiError> {
    let ok = (3..=32).contains(&u.len())
        && u.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if ok { Ok(()) } else { Err(ApiError::bad("username: 3-32 chars, letters, digits, . _ -")) }
}

pub fn validate_password(p: &str) -> Result<(), ApiError> {
    if p.len() >= 10 { Ok(()) } else { Err(ApiError::bad("password must have at least 10 characters")) }
}

pub fn token_hash(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

pub fn new_token() -> String {
    let mut b = [0u8; 32];
    rand::rng().fill(&mut b);
    hex::encode(b)
}

pub fn cookie_value(headers: &HeaderMap, name: &str) -> Option<String> {
    let raw = headers.get(header::COOKIE)?.to_str().ok()?;
    raw.split(';').find_map(|kv| {
        let (k, v) = kv.trim().split_once('=')?;
        (k == name).then(|| v.to_string())
    })
}

pub fn session_cookie(state: &AppState, token: &str) -> String {
    format!(
        "{COOKIE}={token}; Path=/; HttpOnly; SameSite=Strict; Max-Age={SESSION_SECS}{}",
        if state.secure_cookies { "; Secure" } else { "" }
    )
}

pub fn clear_cookie() -> String {
    format!("{COOKIE}=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0")
}

/// Rejects cross-site requests: when `Origin` is present its host must match `Host`.
pub fn origin_ok(headers: &HeaderMap) -> bool {
    let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) else {
        return true;
    };
    let host = headers.get(header::HOST).and_then(|v| v.to_str().ok()).unwrap_or("");
    let o = origin.split_once("://").map(|x| x.1).unwrap_or(origin);
    o == host
}

pub fn ct_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Extractor: the authenticated user (from the session cookie).
pub struct Auth(pub User);
/// Extractor: authenticated admin.
pub struct Admin(pub User);

impl FromRequestParts<Shared> for Auth {
    type Rejection = ApiError;
    async fn from_request_parts(parts: &mut Parts, state: &Shared) -> Result<Self, ApiError> {
        let token = cookie_value(&parts.headers, COOKIE)
            .ok_or(ApiError(StatusCode::UNAUTHORIZED, "login required".into()))?;
        let user = state
            .db
            .user_by_session(&token_hash(&token))?
            .ok_or(ApiError(StatusCode::UNAUTHORIZED, "login required".into()))?;
        Ok(Auth(user))
    }
}

impl FromRequestParts<Shared> for Admin {
    type Rejection = ApiError;
    async fn from_request_parts(parts: &mut Parts, state: &Shared) -> Result<Self, ApiError> {
        let Auth(u) = Auth::from_request_parts(parts, state).await?;
        if u.is_admin() { Ok(Admin(u)) } else { Err(ApiError::forbidden()) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_roundtrip() {
        let h = hash_password("correct horse battery").unwrap();
        assert!(verify_password("correct horse battery", &h));
        assert!(!verify_password("wrong", &h));
    }

    #[test]
    fn origin_check() {
        let mut h = HeaderMap::new();
        h.insert(header::HOST, "tt.local:8443".parse().unwrap());
        assert!(origin_ok(&h));
        h.insert(header::ORIGIN, "https://tt.local:8443".parse().unwrap());
        assert!(origin_ok(&h));
        h.insert(header::ORIGIN, "https://evil.example".parse().unwrap());
        assert!(!origin_ok(&h));
    }

    #[test]
    fn ansi_stripping() {
        assert_eq!(strip_ansi("\x1b[31mred\x1b[0m text\r\n"), "red text\n");
    }

    #[test]
    fn username_rules() {
        assert!(validate_username("emerson.assis").is_ok());
        assert!(validate_username("a b").is_err());
        assert!(validate_username("ab").is_err());
    }
}
