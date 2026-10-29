//! MCP (Model Context Protocol) server over HTTP (`POST /mcp`, JSON-RPC 2.0) with personal tokens.
//! `tt mcp` bridges stdio clients to this endpoint.
//!
//! Safety model: nothing here changes a device on its own.
//!  - `suggest_command` only queues a suggestion the technician must approve in the UI;
//!  - `run_readonly` runs only commands from the device profile's read-only allowlist and needs an `exec` token;
//!  - everything returned to the AI is passed through secret redaction;
//!  - every call is recorded in the audit log.
use std::time::Duration;

use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};
use tt_core::{redact::redact, ssh};

use crate::{app::*, conn::connect_trusted, db::User};

const PROTOCOL: &str = "2025-06-18";

fn tools() -> Value {
    json!([
        {"name":"list_hosts","description":"List the technician's saved hosts (no secrets): id, name, folder, address, device type.",
         "inputSchema":{"type":"object","properties":{}}},
        {"name":"list_sessions","description":"List terminals currently open in TT (session_id, host).",
         "inputSchema":{"type":"object","properties":{}}},
        {"name":"get_device_profile","description":"Device-type notes, quick commands and which commands are read-only for a host.",
         "inputSchema":{"type":"object","properties":{"host_id":{"type":"integer"}},"required":["host_id"]}},
        {"name":"read_terminal","description":"Last lines of an open terminal session (what the technician sees). Secrets are masked.",
         "inputSchema":{"type":"object","properties":{"session_id":{"type":"integer"},"host_id":{"type":"integer"},"lines":{"type":"integer","default":60}}}},
        {"name":"suggest_command","description":"Propose a command to the technician. It is NOT executed: it appears in TT and the technician approves, pastes or runs it.",
         "inputSchema":{"type":"object","properties":{"session_id":{"type":"integer"},"host_id":{"type":"integer"},"command":{"type":"string"},"reason":{"type":"string"}},"required":["command"]}},
        {"name":"run_readonly","description":"Run ONE read-only command on a host (only commands allowed by its device profile; requires an exec-scope token).",
         "inputSchema":{"type":"object","properties":{"host_id":{"type":"integer"},"command":{"type":"string"}},"required":["host_id","command"]}}
    ])
}

fn tool_result(text: String, is_error: bool) -> Value {
    json!({"content":[{"type":"text","text":text}],"isError":is_error})
}

fn find_session(s: &Shared, user: &User, args: &Value) -> Option<std::sync::Arc<LiveSession>> {
    let live = s.live.lock().unwrap_or_else(|e| e.into_inner());
    let sid = args["session_id"].as_u64();
    let hid = args["host_id"].as_i64();
    let mut best: Option<std::sync::Arc<LiveSession>> = None;
    for l in live.values().filter(|l| l.user_id == user.id) {
        if sid.is_some_and(|x| x == l.id) || (sid.is_none() && hid.is_some_and(|x| x == l.host_id)) {
            if best.as_ref().is_none_or(|b| l.id > b.id) {
                best = Some(l.clone());
            }
        }
    }
    best
}

async fn call_tool(s: &Shared, user: &User, scope: &str, name: &str, args: &Value) -> Value {
    match name {
        "list_hosts" => match s.db.list_hosts(user.id) {
            Ok(h) => {
                let v: Vec<Value> = h.iter().map(|x| json!({"id":x.id,"name":x.name,"folder":x.folder,"address":format!("{}:{}",x.host,x.port),"device_type":x.profile_id})).collect();
                tool_result(serde_json::to_string_pretty(&v).unwrap_or_default(), false)
            }
            Err(e) => tool_result(e.to_string(), true),
        },
        "list_sessions" => {
            let live = s.live.lock().unwrap_or_else(|e| e.into_inner());
            let v: Vec<Value> = live.values().filter(|l| l.user_id == user.id).map(|l| json!({"session_id":l.id,"host_id":l.host_id,"host":l.host_name,"device_type":l.profile_id})).collect();
            tool_result(serde_json::to_string_pretty(&v).unwrap_or_default(), false)
        }
        "get_device_profile" => {
            let Some(id) = args["host_id"].as_i64() else { return tool_result("host_id is required".into(), true) };
            match s.db.get_host(user.id, id) {
                Ok(Some(h)) => match s.profiles.iter().find(|p| p.id == h.view.profile_id) {
                    Some(p) => tool_result(serde_json::to_string_pretty(&json!({"name":p.name,"description":p.description,"notes":p.notes,
                        "readonly_command_prefixes":p.readonly,"quick_commands":p.quick_commands.iter().map(|q| &q.command).collect::<Vec<_>>()})).unwrap_or_default(), false),
                    None => tool_result("unknown device type".into(), true),
                },
                _ => tool_result("host not found".into(), true),
            }
        }
        "read_terminal" => {
            let lines = args["lines"].as_u64().unwrap_or(60).clamp(1, 400) as usize;
            match find_session(s, user, args) {
                Some(l) => {
                    s.db.audit(Some(user.id), "mcp", Some(l.host_id), "read_terminal", &format!("lines={lines}"));
                    tool_result(l.tail_clean(lines), false)
                }
                None => tool_result("no open terminal for that session/host: ask the technician to open it in TT".into(), true),
            }
        }
        "suggest_command" => {
            let cmd = args["command"].as_str().unwrap_or("").trim();
            if cmd.is_empty() || cmd.len() > 1000 {
                return tool_result("command is required (max 1000 chars)".into(), true);
            }
            match find_session(s, user, args) {
                Some(l) => {
                    let profile = s.profiles.iter().find(|p| p.id == l.profile_id);
                    let ro = profile.is_some_and(|p| p.is_readonly_command(cmd));
                    let reason: String = args["reason"].as_str().unwrap_or("").chars().take(500).collect();
                    let _ = l.push.send(json!({"t":"suggestion","command":cmd,"reason":reason,"readonly":ro,"source":"MCP"}));
                    s.db.audit(Some(user.id), "mcp", Some(l.host_id), "suggest_command", cmd);
                    tool_result("Suggestion delivered. It is NOT executed: the technician must approve it in TT.".into(), false)
                }
                None => tool_result("no open terminal for that session/host: ask the technician to open it in TT".into(), true),
            }
        }
        "run_readonly" => {
            if scope != "exec" {
                return tool_result("this token has read scope only; create an exec token to run read-only commands".into(), true);
            }
            let (Some(id), Some(cmd)) = (args["host_id"].as_i64(), args["command"].as_str()) else {
                return tool_result("host_id and command are required".into(), true);
            };
            let row = match s.db.get_host(user.id, id) {
                Ok(Some(r)) => r,
                _ => return tool_result("host not found".into(), true),
            };
            let Some(profile) = s.profiles.iter().find(|p| p.id == row.view.profile_id) else { return tool_result("unknown device type".into(), true) };
            if profile.is_netconf() {
                return tool_result("NETCONF hosts do not take shell commands".into(), true);
            }
            if !profile.is_readonly_command(cmd) {
                s.db.audit(Some(user.id), "mcp", Some(id), "run_readonly_denied", cmd);
                return tool_result(format!("denied: '{cmd}' is not in the read-only allowlist of {}. Use suggest_command so the technician can decide.", profile.name), true);
            }
            let handle = match connect_trusted(s, user.id, &row).await {
                Ok(h) => h,
                Err(e) => return tool_result(e.1, true),
            };
            s.db.audit(Some(user.id), "mcp", Some(id), "run_readonly", cmd);
            let res = ssh::exec_command(&handle, cmd, Duration::from_secs(20), 64 * 1024).await;
            let _ = handle.disconnect(russh::Disconnect::ByApplication, "", "en").await;
            match res {
                Ok((out, status)) => tool_result(format!("exit status: {}\n{}", status.map(|x| x.to_string()).unwrap_or_else(|| "?".into()), redact(&strip_ansi(&out))), false),
                Err(e) => tool_result(format!("{e:#}"), true),
            }
        }
        _ => tool_result(format!("unknown tool {name}"), true),
    }
}

fn rpc_error(id: Value, code: i64, msg: &str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":msg}})
}

async fn handle_message(s: &Shared, user: &User, scope: &str, req: &Value) -> Option<Value> {
    let id = req.get("id").cloned();
    let method = req["method"].as_str().unwrap_or("");
    let Some(id) = id else { return None }; // notification: no response
    Some(match method {
        "initialize" => {
            let v = req["params"]["protocolVersion"].as_str().unwrap_or(PROTOCOL);
            json!({"jsonrpc":"2.0","id":id,"result":{"protocolVersion":v,"capabilities":{"tools":{}},
                "serverInfo":{"name":"tt","version":env!("CARGO_PKG_VERSION")},
                "instructions":"TT assists a technician working on SSH terminals. You can read the screen, suggest commands (the technician approves) and, with an exec token, run allow-listed read-only commands. Never ask for passwords."}})
        }
        "ping" => json!({"jsonrpc":"2.0","id":id,"result":{}}),
        "tools/list" => json!({"jsonrpc":"2.0","id":id,"result":{"tools":tools()}}),
        "tools/call" => {
            let name = req["params"]["name"].as_str().unwrap_or("");
            let args = req["params"]["arguments"].clone();
            json!({"jsonrpc":"2.0","id":id,"result":call_tool(s, user, scope, name, &args).await})
        }
        _ => rpc_error(id, -32601, "method not found"),
    })
}

fn bearer(headers: &HeaderMap) -> Option<String> {
    let v = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    v.strip_prefix("Bearer ").map(|t| t.trim().to_string())
}

pub async fn mcp_http(State(s): State<Shared>, headers: HeaderMap, Json(body): Json<Value>) -> Response {
    let Some(tok) = bearer(&headers) else {
        return (StatusCode::UNAUTHORIZED, [(header::WWW_AUTHENTICATE, "Bearer")], "missing bearer token").into_response();
    };
    let (user, scope) = match s.db.user_by_token(&token_hash(&tok)) {
        Ok(Some(x)) => x,
        _ => return (StatusCode::UNAUTHORIZED, [(header::WWW_AUTHENTICATE, "Bearer")], "invalid token").into_response(),
    };
    if let Some(arr) = body.as_array() {
        let mut out = Vec::new();
        for r in arr {
            if let Some(x) = handle_message(&s, &user, &scope, r).await {
                out.push(x);
            }
        }
        return if out.is_empty() { StatusCode::ACCEPTED.into_response() } else { Json(Value::Array(out)).into_response() };
    }
    match handle_message(&s, &user, &scope, &body).await {
        Some(v) => Json(v).into_response(),
        None => StatusCode::ACCEPTED.into_response(),
    }
}

/// `tt mcp --url http://host:8443 --token tt_...`: stdio <-> HTTP bridge for MCP clients that spawn a process.
pub async fn stdio_bridge(url: String, token: String) -> anyhow::Result<()> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let client = reqwest::Client::builder().timeout(Duration::from_secs(60)).build()?;
    let endpoint = format!("{}/mcp", url.trim_end_matches('/'));
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut stdout = tokio::io::stdout();
    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let msg: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let res = client.post(&endpoint).bearer_auth(&token).json(&msg).send().await;
        let reply = match res {
            Ok(r) if r.status() == StatusCode::ACCEPTED || r.status() == StatusCode::NO_CONTENT => None,
            Ok(r) if r.status().is_success() => r.text().await.ok(),
            Ok(r) => msg.get("id").map(|id| rpc_error(id.clone(), -32000, &format!("TT server answered HTTP {}", r.status())).to_string()),
            Err(e) => msg.get("id").map(|id| rpc_error(id.clone(), -32000, &format!("cannot reach TT: {}", e.without_url())).to_string()),
        };
        if let Some(text) = reply {
            stdout.write_all(text.trim().as_bytes()).await?;
            stdout.write_all(b"\n").await?;
            stdout.flush().await?;
        }
    }
    Ok(())
}
