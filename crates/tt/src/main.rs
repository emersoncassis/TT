//! TT - Terminal Tools. Free and open SSH workstation.
//!   tt local   -> single-user mode on this machine (opens the browser, no password)
//!   tt serve   -> multi-user web server with login (first account becomes admin)
mod api;
mod app;
mod db;
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
    #[arg(long, env = "TT_DATA", default_value = "./tt-data", global = true)]
    data: PathBuf,
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
    },
    /// Single-user mode on this machine (listens on 127.0.0.1 only).
    Local {
        #[arg(long, default_value_t = 0)]
        port: u16,
        /// Do not try to open the browser.
        #[arg(long)]
        no_open: bool,
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

fn open_browser(url: &str) {
    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("cmd").args(["/C", "start", "", url]).spawn();
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(url).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let _ = std::process::Command::new("xdg-open").arg(url).spawn();
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into())).init();
    let cli = Cli::parse();
    std::fs::create_dir_all(&cli.data)?;
    let db = db::Db::open(&cli.data.join("tt.db"))?;
    let vault = Vault::new(&load_master_key(&cli.data)?);
    let profiles = profiles::load_profiles(Some(&cli.data.join("profiles")));

    let (bind, local_token, secure_cookies, open) = match &cli.cmd {
        Cmd::Serve { bind, secure_cookies } => (*bind, None, *secure_cookies, false),
        Cmd::Local { port, no_open } => {
            if db.user_by_name("local")?.is_none() {
                // Unusable password: this account is only reachable through the launch token.
                db.create_user("local", &hash_password(&new_token())?, "admin")?;
            }
            (SocketAddr::from(([127, 0, 0, 1], *port)), Some(new_token()), false, !*no_open)
        }
    };

    let state: Shared = Arc::new(AppState {
        db,
        vault,
        profiles,
        data_dir: cli.data.clone(),
        local_token: local_token.clone(),
        secure_cookies,
        attempts: Mutex::new(HashMap::new()),
        dummy_hash: hash_password(&new_token())?,
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
            open_browser(&url);
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
