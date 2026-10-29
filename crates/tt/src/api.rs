//! REST API: setup/login, users (admin), saved hosts, profiles, known hosts, SFTP.
use std::{net::SocketAddr, time::{Duration, Instant}};

use axum::{
    Json, Router,
    body::Bytes,
    extract::{ConnectInfo, DefaultBodyLimit, Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Redirect, Response},
    routing::{delete, get, post, put},
};
use serde::Deserialize;
use serde_json::{Value, json};
use tt_core::{sftp, ssh};

use crate::{app::*, db::*, term};

pub fn router(state: Shared) -> Router {
    Router::new()
        .route("/api/status", get(status))
        .route("/api/setup", post(setup))
        .route("/api/login", post(login))
        .route("/api/logout", post(logout))
        .route("/api/me", get(me))
        .route("/api/branding", get(branding))
        .route("/api/users", get(list_users).post(create_user))
        .route("/api/users/{id}", delete(delete_user))
        .route("/api/users/{id}/password", post(set_user_password))
        .route("/api/me/password", post(change_own_password))
        .route("/api/profiles", get(profiles))
        .route("/api/hosts", get(list_hosts).post(create_host))
        .route("/api/hosts/{id}", put(update_host).delete(delete_host))
        .route("/api/known-hosts", delete(forget_known_host))
        .route("/api/term/{id}", get(term::terminal_ws))
        .route("/api/netconf/{id}", get(term::netconf_ws))
        .route("/api/sftp/{id}/list", get(sftp_list))
        .route("/api/sftp/{id}/download", get(sftp_download))
        .route("/api/sftp/{id}/upload", put(sftp_upload).layer(DefaultBodyLimit::max(512 * 1024 * 1024)))
        .route("/api/sftp/{id}/mkdir", post(sftp_mkdir))
        .route("/api/sftp/{id}/delete", post(sftp_delete))
        .route("/api/sftp/{id}/rename", post(sftp_rename))
        .route("/local-login", get(local_login))
        .layer(axum::middleware::from_fn(csrf_guard))
        .with_state(state)
}

async fn csrf_guard(req: axum::extract::Request, next: axum::middleware::Next) -> Response {
    let safe = matches!(req.method().as_str(), "GET" | "HEAD" | "OPTIONS");
    if !safe && !origin_ok(req.headers()) {
        return ApiError(StatusCode::FORBIDDEN, "cross-site request blocked".into()).into_response();
    }
    next.run(req).await
}

// ---- setup / login --------------------------------------------------------
async fn status(State(s): State<Shared>) -> ApiResult<Json<Value>> {
    Ok(Json(json!({ "needs_setup": s.db.user_count()? == 0, "local_mode": s.local_token.is_some() })))
}

#[derive(Deserialize)]
struct Creds {
    username: String,
    password: String,
}

fn login_response(s: &AppState, user_id: i64) -> ApiResult<Response> {
    let token = new_token();
    s.db.create_session(&token_hash(&token), user_id, now() + SESSION_SECS)?;
    let mut res = Json(json!({ "ok": true })).into_response();
    res.headers_mut().insert(header::SET_COOKIE, HeaderValue::from_str(&session_cookie(s, &token)).map_err(anyhow::Error::from)?);
    Ok(res)
}

/// First run only: the first account created is the administrator.
async fn setup(State(s): State<Shared>, Json(c): Json<Creds>) -> ApiResult<Response> {
    validate_username(&c.username)?;
    validate_password(&c.password)?;
    let hash = hash_password(&c.password)?;
    let id = s.db.create_first_admin(&c.username, &hash).map_err(|e| ApiError(StatusCode::CONFLICT, e.to_string()))?;
    login_response(&s, id)
}

fn throttle_check(s: &AppState, ip: std::net::IpAddr) -> Result<(), ApiError> {
    let mut m = s.attempts.lock().unwrap_or_else(|e| e.into_inner());
    let a = m.entry(ip).or_default();
    if let Some(until) = a.locked_until {
        if Instant::now() < until {
            return Err(ApiError(StatusCode::TOO_MANY_REQUESTS, "too many attempts, try again in a few minutes".into()));
        }
        *a = Default::default();
    }
    Ok(())
}

fn throttle_fail(s: &AppState, ip: std::net::IpAddr) {
    let mut m = s.attempts.lock().unwrap_or_else(|e| e.into_inner());
    let a = m.entry(ip).or_default();
    let start = *a.window_start.get_or_insert_with(Instant::now);
    if start.elapsed() > Duration::from_secs(600) {
        *a = Default::default();
        a.window_start = Some(Instant::now());
    }
    a.count += 1;
    if a.count >= 8 {
        a.locked_until = Some(Instant::now() + Duration::from_secs(300));
    }
}

async fn login(State(s): State<Shared>, ConnectInfo(addr): ConnectInfo<SocketAddr>, Json(c): Json<Creds>) -> ApiResult<Response> {
    throttle_check(&s, addr.ip())?;
    let found = s.db.user_by_name(&c.username)?;
    // Always verify against a hash so timing doesn't reveal whether the user exists.
    let ok = match &found {
        Some((_, h)) => verify_password(&c.password, h),
        None => {
            let _ = verify_password(&c.password, &s.dummy_hash);
            false
        }
    };
    if !ok {
        throttle_fail(&s, addr.ip());
        return Err(ApiError(StatusCode::UNAUTHORIZED, "invalid username or password".into()));
    }
    login_response(&s, found.unwrap().0.id)
}

async fn logout(State(s): State<Shared>, headers: HeaderMap) -> ApiResult<Response> {
    if let Some(t) = cookie_value(&headers, COOKIE) {
        s.db.delete_session(&token_hash(&t))?;
    }
    let mut res = Json(json!({ "ok": true })).into_response();
    res.headers_mut().insert(header::SET_COOKIE, HeaderValue::from_str(&clear_cookie()).map_err(anyhow::Error::from)?);
    Ok(res)
}

async fn local_login(State(s): State<Shared>, Query(q): Query<std::collections::HashMap<String, String>>) -> ApiResult<Response> {
    let (Some(expected), Some(given)) = (&s.local_token, q.get("token")) else {
        return Err(ApiError::forbidden());
    };
    if !ct_eq(expected, given) {
        return Err(ApiError::forbidden());
    }
    let (user, _) = s.db.user_by_name("local")?.ok_or_else(ApiError::not_found)?;
    let token = new_token();
    s.db.create_session(&token_hash(&token), user.id, now() + SESSION_SECS)?;
    let mut res = Redirect::to("/").into_response();
    res.headers_mut().insert(header::SET_COOKIE, HeaderValue::from_str(&session_cookie(&s, &token)).map_err(anyhow::Error::from)?);
    Ok(res)
}

async fn me(Auth(u): Auth) -> Json<Value> {
    Json(json!({ "id": u.id, "username": u.username, "role": u.role }))
}

async fn branding(State(s): State<Shared>) -> Json<Value> {
    let mut b = json!({ "name": "TT", "tagline": "Terminal Tools", "accent": null, "logo": null });
    if let Ok(raw) = std::fs::read_to_string(s.data_dir.join("branding.json")) {
        if let Ok(Value::Object(o)) = serde_json::from_str::<Value>(&raw) {
            for (k, v) in o {
                b[k] = v;
            }
        }
    }
    Json(b)
}

// ---- users (admin) --------------------------------------------------------
async fn list_users(Admin(_): Admin, State(s): State<Shared>) -> ApiResult<Json<Vec<User>>> {
    Ok(Json(s.db.list_users()?))
}

#[derive(Deserialize)]
struct NewUser {
    username: String,
    password: String,
    #[serde(default)]
    role: String,
}

async fn create_user(Admin(_): Admin, State(s): State<Shared>, Json(n): Json<NewUser>) -> ApiResult<Json<Value>> {
    validate_username(&n.username)?;
    validate_password(&n.password)?;
    let role = if n.role == "admin" { "admin" } else { "user" };
    let id = s
        .db
        .create_user(&n.username, &hash_password(&n.password)?, role)
        .map_err(|_| ApiError(StatusCode::CONFLICT, "username already exists".into()))?;
    Ok(Json(json!({ "id": id })))
}

async fn delete_user(Admin(me): Admin, State(s): State<Shared>, Path(id): Path<i64>) -> ApiResult<Json<Value>> {
    if id == me.id {
        return Err(ApiError::bad("you cannot delete your own account"));
    }
    let target = s.db.get_user(id)?.ok_or_else(ApiError::not_found)?;
    if target.is_admin() && s.db.admin_count()? <= 1 {
        return Err(ApiError::bad("cannot delete the last administrator"));
    }
    s.db.delete_user(id)?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct NewPassword {
    password: String,
}

async fn set_user_password(Admin(_): Admin, State(s): State<Shared>, Path(id): Path<i64>, Json(p): Json<NewPassword>) -> ApiResult<Json<Value>> {
    validate_password(&p.password)?;
    s.db.get_user(id)?.ok_or_else(ApiError::not_found)?;
    s.db.set_password(id, &hash_password(&p.password)?)?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct OwnPassword {
    current: String,
    password: String,
}

async fn change_own_password(Auth(u): Auth, State(s): State<Shared>, Json(p): Json<OwnPassword>) -> ApiResult<Response> {
    validate_password(&p.password)?;
    let (_, hash) = s.db.user_by_name(&u.username)?.ok_or_else(ApiError::not_found)?;
    if !verify_password(&p.current, &hash) {
        return Err(ApiError(StatusCode::UNAUTHORIZED, "current password is wrong".into()));
    }
    s.db.set_password(u.id, &hash_password(&p.password)?)?; // also ends all sessions
    login_response(&s, u.id)
}

// ---- profiles and hosts ---------------------------------------------------
async fn profiles(Auth(_): Auth, State(s): State<Shared>) -> Json<Value> {
    Json(serde_json::to_value(&s.profiles).unwrap_or(Value::Null))
}

async fn list_hosts(Auth(u): Auth, State(s): State<Shared>) -> ApiResult<Json<Vec<HostView>>> {
    Ok(Json(s.db.list_hosts(u.id)?))
}

fn validate_host(s: &AppState, h: &mut HostInput) -> Result<(), ApiError> {
    h.name = h.name.trim().to_string();
    h.host = h.host.trim().to_string();
    h.username = h.username.trim().to_string();
    if h.name.is_empty() || h.host.is_empty() || h.username.is_empty() || h.port == 0 {
        return Err(ApiError::bad("name, host, port and username are required"));
    }
    if !matches!(h.auth_kind.as_str(), "password" | "key") {
        return Err(ApiError::bad("auth_kind must be 'password' or 'key'"));
    }
    if h.profile_id.is_empty() {
        h.profile_id = "generic".into();
    }
    if !s.profiles.iter().any(|p| p.id == h.profile_id) {
        return Err(ApiError::bad("unknown device profile"));
    }
    Ok(())
}

fn seal_opt(s: &AppState, v: &str) -> Result<Option<String>, ApiError> {
    if v.is_empty() { Ok(None) } else { Ok(Some(s.vault.seal(v)?)) }
}

async fn create_host(Auth(u): Auth, State(s): State<Shared>, Json(mut h): Json<HostInput>) -> ApiResult<Json<Value>> {
    validate_host(&s, &mut h)?;
    let id = s.db.insert_host(
        u.id,
        &h,
        &seal_opt(&s, &h.secret)?.unwrap_or_default(),
        &seal_opt(&s, &h.passphrase)?.unwrap_or_default(),
        &seal_opt(&s, &h.enable_secret)?.unwrap_or_default(),
    )?;
    Ok(Json(json!({ "id": id })))
}

async fn update_host(Auth(u): Auth, State(s): State<Shared>, Path(id): Path<i64>, Json(mut h): Json<HostInput>) -> ApiResult<Json<Value>> {
    validate_host(&s, &mut h)?;
    let ok = s.db.update_host(
        u.id,
        id,
        &h,
        seal_opt(&s, &h.secret)?.as_deref(),
        seal_opt(&s, &h.passphrase)?.as_deref(),
        seal_opt(&s, &h.enable_secret)?.as_deref(),
    )?;
    if !ok {
        return Err(ApiError::not_found());
    }
    Ok(Json(json!({ "ok": true })))
}

async fn delete_host(Auth(u): Auth, State(s): State<Shared>, Path(id): Path<i64>) -> ApiResult<Json<Value>> {
    if !s.db.delete_host(u.id, id)? {
        return Err(ApiError::not_found());
    }
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct KnownQ {
    host: String,
    port: u16,
}

async fn forget_known_host(Auth(u): Auth, State(s): State<Shared>, Query(q): Query<KnownQ>) -> ApiResult<Json<Value>> {
    s.db.forget_host(u.id, &q.host, q.port)?;
    Ok(Json(json!({ "ok": true })))
}

// ---- SFTP -----------------------------------------------------------------
/// Connects for a file operation. The host key must already be trusted (open a
/// terminal to the host once); unknown or changed keys are refused here.
async fn sftp_session(s: &Shared, user: &User, id: i64) -> ApiResult<(ssh::SshHandle, russh_sftp::client::SftpSession)> {
    let row = s.db.get_host(user.id, id)?.ok_or_else(ApiError::not_found)?;
    let profile = s.profiles.iter().find(|p| p.id == row.view.profile_id).cloned();
    if let Some(p) = &profile {
        if p.sftp == tt_core::profiles::SftpSupport::None {
            return Err(ApiError::bad(format!("{} does not offer SFTP", p.name)));
        }
    }
    let params = term::connect_params(s, &row)?;
    let db = s.clone();
    let uid = user.id;
    let check: ssh::HostKeyCheck = std::sync::Arc::new(move |info: ssh::HostKeyInfo| {
        let db = db.clone();
        Box::pin(async move {
            matches!(db.db.known_fingerprint(uid, &info.host, info.port), Ok(Some(fp)) if fp == info.fingerprint)
        })
    });
    let handle = ssh::connect(&params, check)
        .await
        .map_err(|e| ApiError(StatusCode::BAD_GATEWAY, format!("{e:#} (open a terminal to this host first to trust its key)")))?;
    let sf = sftp::open(&handle).await.map_err(|e| ApiError(StatusCode::BAD_GATEWAY, format!("{e:#}")))?;
    Ok((handle, sf))
}

#[derive(Deserialize)]
struct PathQ {
    #[serde(default)]
    path: String,
}

async fn sftp_list(Auth(u): Auth, State(s): State<Shared>, Path(id): Path<i64>, Query(q): Query<PathQ>) -> ApiResult<Json<Value>> {
    let (_h, sf) = sftp_session(&s, &u, id).await?;
    let p = if q.path.is_empty() { "." } else { &q.path };
    let (cwd, entries) = sftp::list(&sf, p).await?;
    Ok(Json(json!({ "path": cwd, "entries": entries })))
}

async fn sftp_download(Auth(u): Auth, State(s): State<Shared>, Path(id): Path<i64>, Query(q): Query<PathQ>) -> ApiResult<Response> {
    let (_h, sf) = sftp_session(&s, &u, id).await?;
    let data = sftp::download(&sf, &q.path).await?;
    let name = q.path.rsplit('/').next().unwrap_or("download").replace(['"', '\\', '\r', '\n'], "_");
    Ok((
        [
            (header::CONTENT_TYPE, "application/octet-stream".to_string()),
            (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{name}\"")),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff".to_string()),
        ],
        data,
    )
        .into_response())
}

async fn sftp_upload(Auth(u): Auth, State(s): State<Shared>, Path(id): Path<i64>, Query(q): Query<PathQ>, body: Bytes) -> ApiResult<Json<Value>> {
    let (_h, sf) = sftp_session(&s, &u, id).await?;
    sftp::upload(&sf, &q.path, &body).await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct PathBody {
    path: String,
    #[serde(default)]
    is_dir: bool,
}

async fn sftp_mkdir(Auth(u): Auth, State(s): State<Shared>, Path(id): Path<i64>, Json(b): Json<PathBody>) -> ApiResult<Json<Value>> {
    let (_h, sf) = sftp_session(&s, &u, id).await?;
    sftp::mkdir(&sf, &b.path).await?;
    Ok(Json(json!({ "ok": true })))
}

async fn sftp_delete(Auth(u): Auth, State(s): State<Shared>, Path(id): Path<i64>, Json(b): Json<PathBody>) -> ApiResult<Json<Value>> {
    let (_h, sf) = sftp_session(&s, &u, id).await?;
    sftp::remove(&sf, &b.path, b.is_dir).await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct RenameBody {
    from: String,
    to: String,
}

async fn sftp_rename(Auth(u): Auth, State(s): State<Shared>, Path(id): Path<i64>, Json(b): Json<RenameBody>) -> ApiResult<Json<Value>> {
    let (_h, sf) = sftp_session(&s, &u, id).await?;
    sftp::rename(&sf, &b.from, &b.to).await?;
    Ok(Json(json!({ "ok": true })))
}
