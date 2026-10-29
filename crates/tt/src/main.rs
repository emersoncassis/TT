//! TT - Terminal Tools. Free and open SSH workstation.
//!   tt local   -> single-user mode on this machine (opens the browser, no password)
//!   tt serve   -> multi-user web server with login (first account becomes admin)
mod api;
mod app;
mod conn;
mod db;
mod features;
mod mcp;
mod term;
mod ui;

use std::{collections::HashMap, net::SocketAddr, path::PathBuf, sync::{Arc, Mutex}};

use anyhow::{Context, Result};
use axum::{Router, routing::get};
use clap::{Parser, Subcommand};
use tt_core::{profiles, vault::Vault};

use crate::app::{AppState, Shared, hash_password, new_token};

#[derive(Parser)]
#[command(name = "tt", version, about = "TT - Terminal Tools: free SSH workstation with local and web-server modes")]
struct Cli {
    /// Where TT keeps its database, master key and custom profiles.
    /// Default: `./tt-data` for `serve`; a per-user folder for `local` (so a double-click always finds your data).
    #[arg(long, env = "TT_DATA", global = true)]
    data: Option<PathBuf>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Multi-user web server. The first login creates the administrator.
    Serve {
        #[arg(long, env = "TT_BIND", default_value = "127.0.0.1:8443")]
        bind: SocketAddr,
        /// Mark cookies Secure (use when TT sits behind an HTTPS reverse proxy).
        #[arg(long, env = "TT_SECURE_COOKIES")]
        secure_cookies: bool,
        /// Let users create tunnels that listen on non-loopback addresses.
        #[arg(long, env = "TT_ALLOW_PUBLIC_TUNNELS")]
        allow_public_tunnels: bool,
    },
    /// MCP bridge over stdio for AI clients that launch a process (talks to a running TT server).
    Mcp {
        #[arg(long, env = "TT_URL", default_value = "http://127.0.0.1:8443")]
        url: String,
        /// Personal token created in TT (AI & MCP).
        #[arg(long, env = "TT_TOKEN")]
        token: String,
    },
    /// Single-user mode on this machine (listens on 127.0.0.1 only).
    Local {
        #[arg(long, default_value_t = 0)]
        port: u16,
        /// Do not try to open a window.
        #[arg(long)]
        no_open: bool,
        /// Open in a normal browser tab instead of a standalone app window.
        #[arg(long)]
        tab: bool,
    },
}

fn load_master_key(dir: &std::path::Path) -> Result<[u8; 32]> {
    let path = dir.join("master.key");
    if path.exists() {
        let raw = std::fs::read_to_string(&path)?;
        let bytes = hex::decode(raw.trim()).context("master.key is not valid hex")?;
        return bytes.try_into().map_err(|_| anyhow::anyhow!("master.key must be 32 bytes"));
    }
    let key = Vault::generate_key();
    std::fs::write(&path, hex::encode(key))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(key)
}

/// Chromium-based browsers can show a page as a standalone app window (no tabs or address bar),
/// which makes `tt local` feel like a desktop program. Falls back to the default browser.
fn open_browser(url: &str, as_tab: bool) {
    if !as_tab {
        for exe in app_window_candidates() {
            if std::process::Command::new(&exe).arg(format!("--app={url}")).spawn().is_ok() {
                return;
            }
        }
    }
    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("cmd").args(["/C", "start", "", url]).spawn();
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(url).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let _ = std::process::Command::new("xdg-open").arg(url).spawn();
}

fn user_data_dir() -> PathBuf {
    let home = || std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    #[cfg(target_os = "windows")]
    return std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("TT")).unwrap_or_else(|| PathBuf::from(".\\tt-data"));
    #[cfg(target_os = "macos")]
    return home().join("Library/Application Support/TT");
    #[cfg(all(unix, not(target_os = "macos")))]
    return std::env::var_os("XDG_DATA_HOME").map(|d| PathBuf::from(d).join("tt")).unwrap_or_else(|| home().join(".local/share/tt"));
}

fn app_window_candidates() -> Vec<std::path::PathBuf> {
    use std::path::PathBuf;
    let mut v: Vec<PathBuf> = Vec::new();
    #[cfg(target_os = "windows")]
    {
        for var in ["ProgramFiles(x86)", "ProgramFiles", "LOCALAPPDATA"] {
            if let Some(base) = std::env::var_os(var) {
                let base = PathBuf::from(base);
                for rel in ["Microsoft\\Edge\\Application\\msedge.exe", "Google\\Chrome\\Application\\chrome.exe", "BraveSoftware\\Brave-Browser\\Application\\brave.exe"] {
                    let c = base.join(rel);
                    if c.exists() { v.push(c); }
                }
            }
        }
    }
    #[cfg(target_os = "macos")]
    for c in ["/Applications/Google Chrome.app/Contents/MacOS/Google Chrome", "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge", "/Applications/Brave Browser.app/Contents/MacOS/Brave Browser"] {
        if std::path::Path::new(c).exists() { v.push(c.into()); }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    for c in ["google-chrome", "google-chrome-stable", "chromium", "chromium-browser", "microsoft-edge", "brave-browser"] {
        v.push(c.into());
    }
    v
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into())).init();
    // Double-clicking the program (no arguments) starts local mode.
    let mut args: Vec<std::ffi::OsString> = std::env::args_os().collect();
    if args.len() == 1 { args.push("local".into()); }
    let mut cli = Cli::parse_from(args);
    if let Cmd::Mcp { url, token } = &cli.cmd {
        return mcp::stdio_bridge(url.clone(), token.clone()).await;
    }
    let data_dir: PathBuf = cli.data.take().unwrap_or_else(|| if matches!(cli.cmd, Cmd::Local { .. }) { user_data_dir() } else { PathBuf::from("./tt-data") });
    println!("TT data folder: {}", data_dir.display());
    std::fs::create_dir_all(&data_dir)?;
    let db = db::Db::open(&data_dir.join("tt.db"))?;
    let vault = Vault::new(&load_master_key(&data_dir)?);
    let profiles = profiles::load_profiles(Some(&data_dir.join("profiles")));

    let mut tab = false;
    let (bind, local_token, secure_cookies, open, public_tunnels) = match &cli.cmd {
        Cmd::Mcp { .. } => unreachable!(),
        Cmd::Serve { bind, secure_cookies, allow_public_tunnels } => (*bind, None, *secure_cookies, false, *allow_public_tunnels),
        Cmd::Local { port, no_open, tab: t } => {
            tab = *t;
            if db.user_by_name("local")?.is_none() {
                // Unusable password: this account is only reachable through the launch token.
                db.create_user("local", &hash_password(&new_token())?, "admin")?;
            }
            (SocketAddr::from(([127, 0, 0, 1], *port)), Some(new_token()), false, !*no_open, false)
        }
    };

    let state: Shared = Arc::new(AppState {
        db,
        vault,
        profiles,
        data_dir: data_dir.clone(),
        local_token: local_token.clone(),
        secure_cookies,
        attempts: Mutex::new(HashMap::new()),
        dummy_hash: hash_password(&new_token())?,
        allow_public_tunnels: public_tunnels,
        tunnels: Default::default(),
        live: Default::default(),
        next_live: Default::default(),
    });

    let app = Router::new()
        .merge(api::router(state.clone()))
        .route("/branding/{name}", get(ui::logo).with_state(state.clone()))
        .fallback(ui::static_handler);

    let listener = tokio::net::TcpListener::bind(bind).await.with_context(|| format!("cannot listen on {bind}"))?;
    let addr = listener.local_addr()?;
    if let Some(tok) = &local_token {
        let url = format!("http://{addr}/local-login?token={tok}");
        println!("TT local mode: {url}");
        if open {
            open_browser(&url, tab);
        }
    } else {
        println!("TT server listening on http://{addr}");
        if !addr.ip().is_loopback() && !secure_cookies {
            eprintln!("WARNING: listening on a non-loopback address over plain HTTP. Put TT behind an HTTPS reverse proxy (and pass --secure-cookies) before exposing it.");
        }
    }
    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).await?;
    Ok(())
}
