//! Non-interactive connections (SFTP, tunnels, MCP exec): the host key must already be trusted.
use std::sync::Arc;

use axum::http::StatusCode;
use tt_core::ssh::{self, HostKeyCheck, HostKeyInfo, SshHandle};

use crate::{app::*, db::HostRow};

pub async fn connect_trusted(s: &Shared, user_id: i64, row: &HostRow) -> Result<SshHandle, ApiError> {
    let params = crate::term::connect_params(s, row)?;
    let s2 = s.clone();
    let check: HostKeyCheck = Arc::new(move |info: HostKeyInfo| {
        let s2 = s2.clone();
        Box::pin(async move {
            matches!(s2.db.known_fingerprint(user_id, &info.host, info.port), Ok(Some(fp)) if fp == info.fingerprint)
        })
    });
    ssh::connect(&params, check).await.map_err(|e| {
        ApiError(StatusCode::BAD_GATEWAY, format!("{e:#} (open a terminal to this host first to trust its key)"))
    })
}
