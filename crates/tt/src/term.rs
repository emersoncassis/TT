//! WebSocket bridges: interactive SSH terminal and NETCONF console.
use std::{collections::VecDeque, sync::Arc, time::{Duration, Instant}};

use anyhow::{Result, anyhow};
use axum::{
    extract::{
        Path, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::HeaderMap,
    response::Response,
};
use russh::ChannelMsg;
use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot};
use tt_core::{
    netconf,
    profiles::{DeviceProfile, PostLoginStep},
    ssh::{self, Auth as SshAuth, ConnectParams, HostKeyCheck, HostKeyInfo},
};

use crate::{app::*, db::HostRow};

/// Builds connection parameters from a stored host (decrypts secrets).
pub fn connect_params(s: &AppState, row: &HostRow) -> Result<ConnectParams> {
    let profile = s.profiles.iter().find(|p| p.id == row.view.profile_id);
    let username = match profile {
        Some(p) => p.effective_username(&row.view.username),
        None => row.view.username.clone(),
    };
    if row.secret_enc.is_empty() {
        return Err(anyhow!("this host has no saved password/key"));
    }
    let secret = s.vault.open(&row.secret_enc)?;
    let auth = if row.view.auth_kind == "key" {
        let passphrase = if row.passphrase_enc.is_empty() { None } else { Some(s.vault.open(&row.passphrase_enc)?) };
        SshAuth::PrivateKey { pem: secret, passphrase }
    } else {
        SshAuth::Password(secret)
    };
    Ok(ConnectParams {
        host: row.view.host.clone(),
        port: row.view.port,
        username,
        auth,
        legacy: row.view.legacy,
        connect_timeout: Duration::from_secs(15),
    })
}

type Prompt = (HostKeyInfo, Option<String>, oneshot::Sender<bool>);

fn host_key_check(s: &Shared, user_id: i64, prompts: mpsc::UnboundedSender<Prompt>) -> HostKeyCheck {
    let s = s.clone();
    Arc::new(move |info: HostKeyInfo| {
        let s = s.clone();
        let prompts = prompts.clone();
        Box::pin(async move {
            let known = s.db.known_fingerprint(user_id, &info.host, info.port).ok().flatten();
            match known {
                Some(fp) if fp == info.fingerprint => true,
                other => {
                    let (tx, rx) = oneshot::channel();
                    if prompts.send((info.clone(), other, tx)).is_err() {
                        return false;
                    }
                    let trusted = matches!(tokio::time::timeout(Duration::from_secs(90), rx).await, Ok(Ok(true)));
                    if trusted {
                        let _ = s.db.trust_host(user_id, &info.host, info.port, &info.key_type, &info.fingerprint);
                    }
                    trusted
                }
            }
        })
    })
}

async fn send_json(ws: &mut WebSocket, v: Value) -> bool {
    ws.send(Message::Text(v.to_string().into())).await.is_ok()
}

/// Connects while relaying host-key questions to the browser.
/// A *changed* key is only accepted after an explicit second confirmation.
async fn connect_with_prompts(
    s: &Shared,
    user_id: i64,
    params: &ConnectParams,
    ws: &mut WebSocket,
) -> Result<ssh::SshHandle> {
    let (ptx, mut prx) = mpsc::unbounded_channel::<Prompt>();
    let check = host_key_check(s, user_id, ptx);
    let fut = ssh::connect(params, check);
    tokio::pin!(fut);
    loop {
        tokio::select! {
            r = &mut fut => return r,
            Some((info, previous, decision)) = prx.recv() => {
                let changed = previous.is_some();
                send_json(ws, json!({"t":"hostkey","host":info.host,"port":info.port,"key_type":info.key_type,
                    "fingerprint":info.fingerprint,"changed":changed,"previous":previous})).await;
                let answer = loop {
                    match tokio::time::timeout(Duration::from_secs(90), ws.recv()).await {
                        Ok(Some(Ok(Message::Text(t)))) => {
                            let v: Value = serde_json::from_str(&t).unwrap_or_default();
                            match v["t"].as_str() {
                                Some("trust") => break true,
                                Some("reject") => break false,
                                _ => continue,
                            }
                        }
                        Ok(Some(Ok(_))) => continue,
                        _ => break false,
                    }
                };
                let _ = decision.send(answer);
            }
        }
    }
}

struct Scripted {
    steps: VecDeque<PostLoginStep>,
    step_started: Instant,
}

fn build_script(profile: Option<&DeviceProfile>, enable: Option<String>) -> Scripted {
    let mut steps = VecDeque::new();
    if let Some(p) = profile {
        steps.extend(p.post_login.iter().cloned());
        for c in &p.init_commands {
            steps.push_back(PostLoginStep { expect: String::new(), send: format!("{c}\n"), timeout_secs: 5 });
        }
        if p.enable_support {
            if let Some(pw) = enable {
                steps.push_back(PostLoginStep { expect: String::new(), send: "enable\n".into(), timeout_secs: 5 });
                steps.push_back(PostLoginStep { expect: "assword".into(), send: format!("{pw}\n"), timeout_secs: 5 });
            }
        }
    }
    Scripted { steps, step_started: Instant::now() }
}

fn origin_or_forbid(headers: &HeaderMap) -> Result<(), ApiError> {
    if origin_ok(headers) { Ok(()) } else { Err(ApiError::forbidden()) }
}

pub async fn terminal_ws(
    ws: WebSocketUpgrade,
    State(s): State<Shared>,
    Auth(user): Auth,
    Path(id): Path<i64>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    origin_or_forbid(&headers)?;
    let row = s.db.get_host(user.id, id)?.ok_or_else(ApiError::not_found)?;
    Ok(ws.on_upgrade(move |sock| async move {
        if let Err(e) = run_terminal(s, user.id, row, sock).await {
            tracing::info!("terminal ended: {e:#}");
        }
    }))
}

async fn run_terminal(s: Shared, user_id: i64, row: HostRow, mut ws: WebSocket) -> Result<()> {
    let profile = s.profiles.iter().find(|p| p.id == row.view.profile_id).cloned();
    let params = match connect_params(&s, &row) {
        Ok(p) => p,
        Err(e) => {
            send_json(&mut ws, json!({"t":"error","msg":format!("{e:#}")})).await;
            return Ok(());
        }
    };
    send_json(&mut ws, json!({"t":"status","msg":format!("Connecting to {}:{} ...", params.host, params.port)})).await;
    let handle = match connect_with_prompts(&s, user_id, &params, &mut ws).await {
        Ok(h) => h,
        Err(e) => {
            send_json(&mut ws, json!({"t":"error","msg":format!("{e:#}")})).await;
            return Ok(());
        }
    };
    // First message from the browser after connect: initial size (default 80x24).
    let (mut cols, mut rows) = (80u32, 24u32);
    if let Ok(Some(Ok(Message::Text(t)))) = tokio::time::timeout(Duration::from_secs(2), ws.recv()).await {
        let v: Value = serde_json::from_str(&t).unwrap_or_default();
        if v["t"] == "resize" {
            cols = v["cols"].as_u64().unwrap_or(80) as u32;
            rows = v["rows"].as_u64().unwrap_or(24) as u32;
        }
    }
    let mut ch = ssh::open_shell(&handle, cols, rows).await?;
    send_json(&mut ws, json!({"t":"ready"})).await;

    let enable = if row.enable_enc.is_empty() { None } else { s.vault.open(&row.enable_enc).ok() };
    let mut script = build_script(profile.as_ref(), enable);
    let mut tail = String::new();
    let mut last_output = Instant::now();
    let mut seen_output = false;
    let started = Instant::now();
    let mut tick = tokio::time::interval(Duration::from_millis(100));

    loop {
        tokio::select! {
            msg = ch.wait() => {
                match msg {
                    Some(ChannelMsg::Data { data }) | Some(ChannelMsg::ExtendedData { data, .. }) => {
                        last_output = Instant::now();
                        seen_output = true;
                        tail.push_str(&String::from_utf8_lossy(&data));
                        if tail.len() > 4096 {
                            let cut = tail.len() - 2048;
                            let cut = (cut..tail.len()).find(|i| tail.is_char_boundary(*i)).unwrap_or(tail.len());
                            tail.drain(..cut);
                        }
                        if ws.send(Message::Binary(data.to_vec().into())).await.is_err() { break; }
                    }
                    Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) | None => break,
                    Some(ChannelMsg::ExitStatus { .. }) => {}
                    _ => {}
                }
            }
            incoming = ws.recv() => {
                match incoming {
                    Some(Ok(Message::Binary(b))) => { ch.data(&b[..]).await?; }
                    Some(Ok(Message::Text(t))) => {
                        let v: Value = serde_json::from_str(&t).unwrap_or_default();
                        if v["t"] == "resize" {
                            let c = v["cols"].as_u64().unwrap_or(80) as u32;
                            let r = v["rows"].as_u64().unwrap_or(24) as u32;
                            ch.window_change(c, r, 0, 0).await?;
                        }
                    }
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                    _ => {}
                }
            }
            _ = tick.tick() => {
                if let Some(step) = script.steps.front().cloned() {
                    let waited = script.step_started.elapsed();
                    let timed_out = waited > Duration::from_secs(step.timeout_secs);
                    let ready = if step.expect.is_empty() {
                        (seen_output && last_output.elapsed() > Duration::from_millis(350)) || started.elapsed() > Duration::from_secs(4)
                    } else {
                        tail.contains(&step.expect)
                    };
                    if ready {
                        ch.data(step.send.as_bytes()).await?;
                        tail.clear();
                        last_output = Instant::now();
                        script.steps.pop_front();
                        script.step_started = Instant::now();
                    } else if timed_out && !step.expect.is_empty() {
                        // Expected text never appeared (e.g. pfSense user already has a plain shell).
                        script.steps.pop_front();
                        script.step_started = Instant::now();
                    }
                }
            }
        }
    }
    let _ = handle.disconnect(russh::Disconnect::ByApplication, "", "en").await;
    send_json(&mut ws, json!({"t":"closed"})).await;
    Ok(())
}

pub async fn netconf_ws(
    ws: WebSocketUpgrade,
    State(s): State<Shared>,
    Auth(user): Auth,
    Path(id): Path<i64>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    origin_or_forbid(&headers)?;
    let row = s.db.get_host(user.id, id)?.ok_or_else(ApiError::not_found)?;
    Ok(ws.on_upgrade(move |sock| async move {
        if let Err(e) = run_netconf(s, user.id, row, sock).await {
            tracing::info!("netconf ended: {e:#}");
        }
    }))
}

async fn run_netconf(s: Shared, user_id: i64, row: HostRow, mut ws: WebSocket) -> Result<()> {
    let params = match connect_params(&s, &row) {
        Ok(p) => p,
        Err(e) => {
            send_json(&mut ws, json!({"t":"error","msg":format!("{e:#}")})).await;
            return Ok(());
        }
    };
    let handle = match connect_with_prompts(&s, user_id, &params, &mut ws).await {
        Ok(h) => h,
        Err(e) => {
            send_json(&mut ws, json!({"t":"error","msg":format!("{e:#}")})).await;
            return Ok(());
        }
    };
    let mut ch = ssh::open_netconf(&handle).await?;
    ch.data(netconf::encode(netconf::Framing::Eom, &netconf::client_hello()).as_slice()).await?;

    let mut framing = netconf::Framing::Eom;
    let mut got_hello = false;
    let mut buf: Vec<u8> = Vec::new();
    let mut msg_id = 100u32;
    loop {
        tokio::select! {
            msg = ch.wait() => {
                match msg {
                    Some(ChannelMsg::Data { data }) => {
                        buf.extend_from_slice(&data);
                        loop {
                            match netconf::decode(framing, &mut buf) {
                                Ok(Some(xml)) => {
                                    if !got_hello {
                                        got_hello = true;
                                        framing = netconf::negotiate(&xml);
                                        let f = if framing == netconf::Framing::Chunked { "1.1" } else { "1.0" };
                                        send_json(&mut ws, json!({"t":"hello","xml":xml,"framing":f})).await;
                                    } else {
                                        send_json(&mut ws, json!({"t":"reply","xml":xml})).await;
                                    }
                                }
                                Ok(None) => break,
                                Err(e) => { send_json(&mut ws, json!({"t":"error","msg":format!("framing error: {e}")})).await; return Ok(()); }
                            }
                        }
                    }
                    Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) | None => break,
                    _ => {}
                }
            }
            incoming = ws.recv() => {
                match incoming {
                    Some(Ok(Message::Text(t))) => {
                        let v: Value = serde_json::from_str(&t).unwrap_or_default();
                        let xml = match v["t"].as_str() {
                            Some("rpc") => v["xml"].as_str().map(str::to_string),
                            Some("op") => { msg_id += 1; v["body"].as_str().map(|b| netconf::rpc(msg_id, b)) }
                            _ => None,
                        };
                        if let Some(xml) = xml {
                            ch.data(netconf::encode(framing, &xml).as_slice()).await?;
                        }
                    }
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                    _ => {}
                }
            }
        }
    }
    let _ = handle.disconnect(russh::Disconnect::ByApplication, "", "en").await;
    send_json(&mut ws, json!({"t":"closed"})).await;
    Ok(())
}
