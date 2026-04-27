//! Minimal telnet client for talking to the running 7DTD server.
//!
//! 7DTD's "telnet" interface is plain TCP with line-oriented text — no
//! actual telnet option negotiation. The protocol is:
//!
//!   1. Connect to TelnetPort (default 8081).
//!   2. If TelnetPassword is set, the server prompts:
//!        "Please enter password:"
//!      Send the password followed by `\r\n`.
//!   3. Server replies with "Logon successful." on success.
//!   4. Send commands as `<cmd>\r\n`. Server emits response lines.
//!
//! We keep the connection open for the lifetime of the server and expose
//! a `send_command` / `recv_line` interface. Each line we read is also
//! pushed into the shared log buffer so it shows up in the console UI.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::sync::{Mutex, RwLock};
use tokio::time::timeout;

use crate::state::{AppState, LogKind};

/// Resolve telnet host/port/password from the live serverconfig.xml on disk.
fn resolve_telnet_settings(state: &AppState) -> Result<(u16, Option<String>)> {
    let path = state
        .settings
        .resolve_config_path()
        .ok_or_else(|| anyhow!("server config path not set"))?;
    if !path.exists() {
        bail!("serverconfig.xml not found at {}", path.display());
    }
    let cfg = crate::config::ServerConfig::load(&path)?;

    let enabled = cfg.get("TelnetEnabled").map(|v| v.eq_ignore_ascii_case("true")).unwrap_or(false);
    if !enabled {
        bail!("TelnetEnabled is false in serverconfig.xml — enable it and restart the server");
    }
    let port = cfg
        .get("TelnetPort")
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(8081);
    let pw = cfg.get("TelnetPassword").map(|s| s.to_string()).filter(|s| !s.is_empty());
    Ok((port, pw))
}

/// A connection handle. Cloneable: holds an Arc<Mutex<writer>>.
#[derive(Clone, Debug)]
pub struct TelnetClient {
    writer: Arc<Mutex<OwnedWriteHalf>>,
}

impl TelnetClient {
    /// Send a raw command. The trailing CRLF is added for you.
    pub async fn send(&self, cmd: &str) -> Result<()> {
        let mut w = self.writer.lock().await;
        w.write_all(cmd.as_bytes()).await?;
        w.write_all(b"\r\n").await?;
        w.flush().await?;
        Ok(())
    }
}

/// Connect to the running server's telnet port, authenticate if needed,
/// and start a background task that pumps server output into the log
/// buffer. Returns a `TelnetClient` you can use to send commands.
pub async fn connect(state: Arc<RwLock<AppState>>) -> Result<TelnetClient> {
    let (port, password) = {
        let s = state.read().await;
        resolve_telnet_settings(&s)?
    };

    // Up to ~10s of retries — the game takes a while to open the port.
    let stream = {
        let mut last_err: Option<anyhow::Error> = None;
        let mut connected = None;
        for _ in 0..20 {
            match TcpStream::connect(("127.0.0.1", port)).await {
                Ok(s) => { connected = Some(s); break; }
                Err(e) => last_err = Some(e.into()),
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        connected.ok_or_else(|| {
            last_err.unwrap_or_else(|| anyhow!("could not connect to telnet port"))
        }).context(format!("connecting to localhost:{port}"))?
    };

    let (read_half, write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);

    // Auth handshake. The server emits a few greeting lines, then the
    // password prompt if one is configured.
    if let Some(pw) = password {
        // Read greeting lines until we see the password prompt or hit a
        // 5s timeout. We push every line into the log buffer regardless.
        let mut auth_done = false;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        let mut writer = write_half;
        while tokio::time::Instant::now() < deadline && !auth_done {
            let mut line = String::new();
            match timeout(Duration::from_millis(500), reader.read_line(&mut line)).await {
                Ok(Ok(0)) => bail!("telnet closed during auth"),
                Ok(Ok(_)) => {
                    let trimmed = line.trim_end_matches(['\r', '\n']).to_string();
                    state.write().await.push_log(LogKind::Manager, format!("telnet> {}", trimmed));
                    if trimmed.to_ascii_lowercase().contains("password") {
                        writer.write_all(pw.as_bytes()).await?;
                        writer.write_all(b"\r\n").await?;
                        writer.flush().await?;
                        auth_done = true;
                    }
                }
                Ok(Err(e)) => return Err(e.into()),
                Err(_) => { /* read timed out — keep trying until deadline */ }
            }
        }
        if !auth_done {
            bail!("never saw a telnet password prompt — wrong port?");
        }

        let client = TelnetClient { writer: Arc::new(Mutex::new(writer)) };
        spawn_reader(state, reader);
        Ok(client)
    } else {
        let client = TelnetClient { writer: Arc::new(Mutex::new(write_half)) };
        spawn_reader(state, reader);
        Ok(client)
    }
}

/// Pump telnet output into the shared log buffer (with kind=Manager so
/// it renders in the accent color).
fn spawn_reader(state: Arc<RwLock<AppState>>, mut reader: BufReader<OwnedReadHalf>) {
    tokio::spawn(async move {
        loop {
            let mut line = String::new();
            match reader.read_line(&mut line).await {
                Ok(0) => {
                    state.write().await.log_manager("telnet connection closed");
                    state.write().await.telnet = None;
                    return;
                }
                Ok(_) => {
                    let trimmed = line.trim_end_matches(['\r', '\n']).to_string();
                    if trimmed.is_empty() { continue; }
                    state.write().await.push_log(LogKind::Manager, format!("telnet> {}", trimmed));
                }
                Err(e) => {
                    state.write().await.log_manager(format!("telnet read error: {e}"));
                    state.write().await.telnet = None;
                    return;
                }
            }
        }
    });
}
