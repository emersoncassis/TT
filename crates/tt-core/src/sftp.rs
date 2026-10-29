//! SFTP helpers on top of an authenticated SSH handle.
use anyhow::{Context, Result, bail};
use russh_sftp::client::SftpSession;
use serde::Serialize;

use crate::ssh::SshHandle;

#[derive(Debug, Serialize)]
pub struct Entry {
    pub name: String,
    pub is_dir: bool,
    pub is_symlink: bool,
    pub size: u64,
    pub mtime: Option<u64>,
}

pub async fn open(handle: &SshHandle) -> Result<SftpSession> {
    let ch = handle.channel_open_session().await?;
    ch.request_subsystem(true, "sftp")
        .await
        .context("this device refused the SFTP subsystem (e.g. Dropbear without openssh-sftp-server)")?;
    Ok(SftpSession::new(ch.into_stream()).await?)
}

pub async fn list(s: &SftpSession, path: &str) -> Result<(String, Vec<Entry>)> {
    let canon = s.canonicalize(path).await?;
    let mut out = Vec::new();
    for e in s.read_dir(canon.clone()).await? {
        let name = e.file_name();
        if name == "." || name == ".." {
            continue;
        }
        let m = e.metadata();
        let ft = e.file_type();
        out.push(Entry {
            name,
            is_dir: ft.is_dir(),
            is_symlink: ft.is_symlink(),
            size: m.len(),
            mtime: m
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs()),
        });
    }
    out.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    Ok((canon, out))
}

fn check_path(p: &str) -> Result<()> {
    if p.is_empty() || p.contains('\0') {
        bail!("invalid path");
    }
    Ok(())
}

pub async fn download(s: &SftpSession, path: &str) -> Result<Vec<u8>> {
    check_path(path)?;
    Ok(s.read(path).await?)
}

pub async fn upload(s: &SftpSession, path: &str, data: &[u8]) -> Result<()> {
    use russh_sftp::protocol::OpenFlags;
    use tokio::io::AsyncWriteExt;
    check_path(path)?;
    let mut f = s
        .open_with_flags(path, OpenFlags::CREATE | OpenFlags::TRUNCATE | OpenFlags::WRITE)
        .await?;
    f.write_all(data).await?;
    f.shutdown().await?;
    Ok(())
}

pub async fn mkdir(s: &SftpSession, path: &str) -> Result<()> {
    check_path(path)?;
    Ok(s.create_dir(path).await?)
}

pub async fn remove(s: &SftpSession, path: &str, is_dir: bool) -> Result<()> {
    check_path(path)?;
    if is_dir { Ok(s.remove_dir(path).await?) } else { Ok(s.remove_file(path).await?) }
}

pub async fn rename(s: &SftpSession, from: &str, to: &str) -> Result<()> {
    check_path(from)?;
    check_path(to)?;
    Ok(s.rename(from, to).await?)
}
