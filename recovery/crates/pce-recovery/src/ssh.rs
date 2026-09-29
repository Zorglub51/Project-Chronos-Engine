//! Minimal SSH client wrapper around `russh`.
//!
//! Configured for the PCE Mini recovery image: connects to
//! `169.254.13.37:22` as `root` with an empty password, accepts any host
//! key (the recovery initrd regenerates one each boot, so pinning is
//! pointless). Exposes a single `exec(cmd)` that runs a remote command and
//! returns its stdout/stderr/exit_status.

use std::sync::Arc;
use std::time::Duration;

use russh::client::{Config, Handler, Msg};
use russh::keys::ssh_key::PublicKey;
use russh::{ChannelMsg, Disconnect};
use tokio::runtime::Runtime;

#[derive(Debug, thiserror::Error)]
pub enum SshError {
    #[error("connect: {0}")]
    Connect(String),
    #[error("auth failed (password)")]
    AuthFailed,
    #[error("ssh: {0}")]
    Ssh(#[from] russh::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, SshError>;

#[derive(Debug)]
pub struct ExecOutput {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub exit_status: Option<u32>,
}

/// Run `command` on the recovery initrd and return its output.
///
/// Connect/auth/exec/disconnect, all in one. Synchronous: spins a tokio
/// runtime under the hood so it composes cleanly with the rest of the CLI's
/// blocking style.
pub fn exec(host: &str, command: &str) -> Result<ExecOutput> {
    let rt = Runtime::new().map_err(SshError::Io)?;
    rt.block_on(exec_async(host, command))
}

async fn exec_async(host: &str, command: &str) -> Result<ExecOutput> {
    // russh client: minimal config, accept any host key.
    let config = Arc::new(Config {
        inactivity_timeout: Some(Duration::from_secs(600)),
        ..Default::default()
    });

    let target = if host.contains(':') {
        host.to_string()
    } else {
        format!("{host}:22")
    };

    let mut session = tokio::time::timeout(
        Duration::from_secs(10),
        russh::client::connect(config, target.as_str(), AcceptAny),
    )
    .await
    .map_err(|_| SshError::Connect("connection timed out".into()))?
    .map_err(|e| SshError::Connect(e.to_string()))?;

    // The recovery dropbear is set up so that root has no password and
    // accepts the "none" authentication method (verified with `ssh -v`:
    // "Authenticated using 'none'"). Try that first; fall back to an empty
    // password attempt for robustness.
    let auth = session.authenticate_none("root").await?;
    if !auth.success() {
        let auth = session.authenticate_password("root", "").await?;
        if !auth.success() {
            return Err(SshError::AuthFailed);
        }
    }

    let mut channel: russh::Channel<Msg> = session.channel_open_session().await?;
    channel.exec(true, command).await?;

    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let mut exit_status: Option<u32> = None;

    while let Some(msg) = channel.wait().await {
        match msg {
            ChannelMsg::Data { ref data } => stdout.extend_from_slice(data),
            ChannelMsg::ExtendedData { ref data, ext: 1 } => stderr.extend_from_slice(data),
            ChannelMsg::ExitStatus { exit_status: code } => exit_status = Some(code),
            ChannelMsg::Eof => {}
            ChannelMsg::Close => break,
            _ => {}
        }
    }

    let _ = session
        .disconnect(Disconnect::ByApplication, "", "en")
        .await;
    Ok(ExecOutput {
        stdout,
        stderr,
        exit_status,
    })
}

/// Handler that accepts any server host key. The recovery initrd regenerates
/// keys on every boot so verification can't anchor on anything stable.
struct AcceptAny;

impl Handler for AcceptAny {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        _key: &PublicKey,
    ) -> std::result::Result<bool, Self::Error> {
        Ok(true)
    }
}
