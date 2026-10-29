//! SSH client built on russh: connection, host-key verification (TOFU via callback),
//! password / private-key / keyboard-interactive auth, optional legacy algorithms.
use std::{borrow::Cow, sync::Arc, time::Duration};

use anyhow::{Result, anyhow, bail};
use futures::future::BoxFuture;
use russh::{
    Preferred, cipher, client,
    client::KeyboardInteractiveAuthResponse,
    kex, mac,
    keys::{HashAlg, PrivateKeyWithHashAlg, PublicKeyOrCertificate, decode_secret_key},
};

#[derive(Debug, Clone)]
pub enum Auth {
    Password(String),
    PrivateKey { pem: String, passphrase: Option<String> },
}

#[derive(Debug, Clone)]
pub struct ConnectParams {
    pub host: String,
    pub port: u16,
    /// Already adjusted by the device profile (e.g. `admin+ct`).
    pub username: String,
    pub auth: Auth,
    /// Enable old algorithms (DH group1/14-SHA1, AES-CBC, HMAC-SHA1) for aging gear.
    pub legacy: bool,
    pub connect_timeout: Duration,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct HostKeyInfo {
    pub host: String,
    pub port: u16,
    pub key_type: String,
    pub fingerprint: String,
}

/// Asked once per connection; return true to trust the key.
pub type HostKeyCheck = Arc<dyn Fn(HostKeyInfo) -> BoxFuture<'static, bool> + Send + Sync>;

pub struct ClientHandler {
    host: String,
    port: u16,
    check: HostKeyCheck,
}

impl client::Handler for ClientHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        key: &PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        let PublicKeyOrCertificate::PublicKey { key, .. } = key else {
            // Host certificates need a CA trust model; not supported yet.
            return Ok(false);
        };
        let info = HostKeyInfo {
            host: self.host.clone(),
            port: self.port,
            key_type: key.algorithm().to_string(),
            fingerprint: key.fingerprint(HashAlg::Sha256).to_string(),
        };
        Ok((self.check)(info).await)
    }
}

pub type SshHandle = client::Handle<ClientHandler>;

fn preferred(legacy: bool) -> Preferred {
    let mut p = Preferred::default();
    if legacy {
        let mut kex_list = p.kex.to_vec();
        kex_list.extend([kex::DH_G14_SHA1, kex::DH_G1_SHA1, kex::DH_GEX_SHA1]);
        p.kex = Cow::Owned(kex_list);
        let mut c = p.cipher.to_vec();
        c.extend([cipher::AES_128_CBC, cipher::AES_256_CBC]);
        p.cipher = Cow::Owned(c);
        let mut m = p.mac.to_vec();
        m.push(mac::HMAC_SHA1);
        p.mac = Cow::Owned(m);
    }
    p
}

pub async fn connect(params: &ConnectParams, check: HostKeyCheck) -> Result<SshHandle> {
    let config = Arc::new(client::Config {
        preferred: preferred(params.legacy),
        inactivity_timeout: None,
        keepalive_interval: Some(Duration::from_secs(30)),
        keepalive_max: 4,
        ..Default::default()
    });
    let handler = ClientHandler { host: params.host.clone(), port: params.port, check };
    let addr = (params.host.as_str(), params.port);
    let mut handle = tokio::time::timeout(params.connect_timeout, client::connect(config, addr, handler))
        .await
        .map_err(|_| anyhow!("timeout connecting to {}:{}", params.host, params.port))?
        .map_err(|e| anyhow!("connection failed: {e}"))?;

    let ok = match &params.auth {
        Auth::Password(pw) => {
            let r = handle.authenticate_password(&params.username, pw).await?;
            if r.success() {
                true
            } else {
                // Many Linux hosts and some devices only offer keyboard-interactive.
                keyboard_interactive(&mut handle, &params.username, pw).await?
            }
        }
        Auth::PrivateKey { pem, passphrase } => {
            let key = decode_secret_key(pem, passphrase.as_deref())
                .map_err(|e| anyhow!("invalid private key: {e}"))?;
            let hash = handle.best_supported_rsa_hash().await?.flatten();
            handle
                .authenticate_publickey(
                    &params.username,
                    PrivateKeyWithHashAlg::new(Arc::new(key), hash),
                )
                .await?
                .success()
        }
    };
    if !ok {
        bail!("authentication failed for user '{}'", params.username);
    }
    Ok(handle)
}

async fn keyboard_interactive(handle: &mut SshHandle, user: &str, password: &str) -> Result<bool> {
    let mut resp = handle.authenticate_keyboard_interactive_start(user, None).await?;
    for _ in 0..4 {
        match resp {
            KeyboardInteractiveAuthResponse::Success => return Ok(true),
            KeyboardInteractiveAuthResponse::Failure { .. } => return Ok(false),
            KeyboardInteractiveAuthResponse::InfoRequest { prompts, .. } => {
                let answers = prompts.iter().map(|_| password.to_string()).collect();
                resp = handle.authenticate_keyboard_interactive_respond(answers).await?;
            }
        }
    }
    Ok(false)
}

/// Opens an interactive shell channel with a PTY.
pub async fn open_shell(
    handle: &SshHandle,
    cols: u32,
    rows: u32,
) -> Result<russh::Channel<client::Msg>> {
    let ch = handle.channel_open_session().await?;
    ch.request_pty(false, "xterm-256color", cols, rows, 0, 0, &[]).await?;
    ch.request_shell(true).await?;
    Ok(ch)
}

/// Opens the `netconf` subsystem (no PTY).
pub async fn open_netconf(handle: &SshHandle) -> Result<russh::Channel<client::Msg>> {
    let ch = handle.channel_open_session().await?;
    ch.request_subsystem(true, "netconf").await?;
    Ok(ch)
}

/// Runs one command on a fresh exec channel; returns (output, exit status).
/// Output is capped at `max_bytes`; the call is bounded by `timeout`.
pub async fn exec_command(
    handle: &SshHandle,
    command: &str,
    timeout: Duration,
    max_bytes: usize,
) -> Result<(String, Option<u32>)> {
    use russh::ChannelMsg;
    let mut ch = handle.channel_open_session().await?;
    ch.exec(true, command.as_bytes().to_vec()).await?;
    let mut out: Vec<u8> = Vec::new();
    let mut status = None;
    let run = async {
        while let Some(msg) = ch.wait().await {
            match msg {
                ChannelMsg::Data { data } | ChannelMsg::ExtendedData { data, .. } => {
                    if out.len() < max_bytes {
                        let room = max_bytes - out.len();
                        out.extend_from_slice(&data[..data.len().min(room)]);
                    }
                }
                ChannelMsg::ExitStatus { exit_status } => status = Some(exit_status),
                ChannelMsg::Close => break,
                _ => {}
            }
        }
    };
    if tokio::time::timeout(timeout, run).await.is_err() {
        bail!("command timed out after {}s", timeout.as_secs());
    }
    Ok((String::from_utf8_lossy(&out).into_owned(), status))
}
