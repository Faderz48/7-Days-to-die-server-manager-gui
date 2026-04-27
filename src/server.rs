//! Start and stop the 7DTD dedicated server child process, and pump
//! its stdout/stderr into the shared log buffer.

use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::{mpsc, RwLock};

use crate::state::{AppState, LogKind, ServerStatus};

/// Launch the dedicated server using the configured paths in `state`.
///
/// Spawns a supervising task that reads stdout/stderr line by line into
/// the log buffer and watches for the child's exit. Returns immediately
/// once the child has been spawned.
pub async fn start(state: Arc<RwLock<AppState>>) -> Result<()> {
    {
        let s = state.read().await;
        if s.status.is_alive() {
            bail!("server is already {:?}", s.status);
        }
    }

    // Resolve paths.
    let (exe, config_path, install_dir) = {
        let s = state.read().await;
        let exe = s
            .settings
            .resolve_executable()
            .ok_or_else(|| anyhow!("server install dir not configured — set it in App Config"))?;
        let cfg = s
            .settings
            .resolve_config_path()
            .ok_or_else(|| anyhow!("serverconfig.xml path not configured"))?;
        let install_dir = s
            .settings
            .server_install_dir
            .clone()
            .ok_or_else(|| anyhow!("server install dir not configured"))?;
        (exe, cfg, install_dir)
    };

    if !exe.exists() {
        bail!("server executable not found at {}", exe.display());
    }
    if !config_path.exists() {
        bail!("serverconfig.xml not found at {}", config_path.display());
    }

    // Build the command. 7DTD's standard dedicated server invocation:
    //   <exe> -configfile=<path> -batchmode -nographics -dedicated
    // We forward the config path explicitly so users can keep multiple
    // configs side by side.
    let mut cmd = Command::new(&exe);
    cmd.current_dir(&install_dir)
        .arg(format!("-configfile={}", config_path.display()))
        .arg("-batchmode")
        .arg("-nographics")
        .arg("-dedicated")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null());

    let mut child = cmd
        .spawn()
        .with_context(|| format!("spawning {}", exe.display()))?;

    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");

    let (stop_tx, mut stop_rx) = mpsc::channel::<()>(1);

    {
        let mut s = state.write().await;
        s.child = Some(child);
        s.stop_tx = Some(stop_tx);
        s.status = ServerStatus::Starting;
        s.started_at = Some(chrono::Utc::now());
        s.log_manager(format!("starting server: {}", exe.display()));
    }

    // ── stdout pump ──────────────────────────────────────────────────────
    let s_out = state.clone();
    tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let became_ready = {
                let mut s = s_out.write().await;
                let was_starting = s.status == ServerStatus::Starting;
                let ready = was_starting && is_ready_line(&line);
                if ready {
                    s.status = ServerStatus::Running;
                    s.log_manager("server reports ready — status: running");
                }
                s.push_log(LogKind::Stdout, line);
                ready
            };
            // Once ready, try to attach telnet. We don't block log
            // pumping on this — connect runs in its own task.
            if became_ready {
                let s_attach = s_out.clone();
                tokio::spawn(async move {
                    match crate::telnet::connect(s_attach.clone()).await {
                        Ok(client) => {
                            s_attach.write().await.telnet = Some(client);
                            s_attach.write().await.log_manager("telnet attached");
                        }
                        Err(e) => {
                            s_attach.write().await.log_manager(
                                format!("telnet not attached: {e}. Set TelnetEnabled=true and TelnetPassword in serverconfig.xml for full features."));
                        }
                    }
                });
            }
        }
    });

    // ── stderr pump ──────────────────────────────────────────────────────
    let s_err = state.clone();
    tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let mut s = s_err.write().await;
            s.push_log(LogKind::Stderr, line);
        }
    });

    // ── supervisor: wait for child exit OR a stop request ────────────────
    let s_sup = state.clone();
    tokio::spawn(async move {
        // We move the child out of state to wait on it, then put status
        // updates back when we're done.
        let mut child = match { s_sup.write().await.child.take() } {
            Some(c) => c,
            None => return,
        };

        tokio::select! {
            res = child.wait() => {
                let mut s = s_sup.write().await;
                let exit = res.map(|st| st.code()).ok().flatten();
                match exit {
                    Some(0) => s.status = ServerStatus::Stopped,
                    Some(code) => {
                        s.status = ServerStatus::Crashed;
                        s.log_manager(format!("server exited with code {code}"));
                    }
                    None => {
                        s.status = ServerStatus::Stopped;
                        s.log_manager("server exited (no code)");
                    }
                }
                s.child = None;
                s.stop_tx = None;
                s.telnet = None;
                s.started_at = None;
            }
            _ = stop_rx.recv() => {
                // Stop requested — terminate the child and clean up.
                {
                    let mut s = s_sup.write().await;
                    s.status = ServerStatus::Stopping;
                    s.log_manager("stop requested — terminating child");
                }
                let _ = child.kill().await;
                let _ = child.wait().await;
                let mut s = s_sup.write().await;
                s.status = ServerStatus::Stopped;
                s.child = None;
                s.stop_tx = None;
                s.telnet = None;
                s.started_at = None;
                s.log_manager("server stopped");
            }
        }
    });

    Ok(())
}

/// Ask the running server to shut down. Tries a graceful telnet
/// `shutdown` first; if the server hasn't exited within `STOP_GRACE`
/// seconds, falls back to terminating the child process.
pub async fn stop(state: Arc<RwLock<AppState>>) -> Result<()> {
    const STOP_GRACE: Duration = Duration::from_secs(20);

    let (tx, telnet) = {
        let mut s = state.write().await;
        if !s.status.is_alive() {
            bail!("server is not running");
        }
        s.status = ServerStatus::Stopping;
        s.log_manager("stop requested");
        (s.stop_tx.clone(), s.telnet.clone())
    };

    // Try graceful shutdown via telnet if the connection is up.
    if let Some(client) = telnet {
        match client.send("shutdown").await {
            Ok(_) => {
                state.write().await.log_manager("sent telnet 'shutdown' — waiting for clean exit");
                // Poll status: the supervisor flips to Stopped on natural
                // exit. If it doesn't happen in time, fall through to kill.
                let deadline = tokio::time::Instant::now() + STOP_GRACE;
                while tokio::time::Instant::now() < deadline {
                    tokio::time::sleep(Duration::from_millis(500)).await;
                    let st = state.read().await.status;
                    if !st.is_alive() {
                        return Ok(());
                    }
                }
                state.write().await.log_manager("graceful shutdown timed out — terminating");
            }
            Err(e) => {
                state.write().await.log_manager(format!("telnet shutdown failed: {e} — terminating"));
            }
        }
    }

    // Fallback: ask the supervisor to kill the child.
    if let Some(tx) = tx {
        let _ = tx.send(()).await;
    }
    Ok(())
}

/// Recognize log lines that mean the server is fully initialized.
fn is_ready_line(line: &str) -> bool {
    // The exact wording can shift between game versions. Match a handful
    // of phrases that all signal a ready dedicated server.
    let l = line.to_ascii_lowercase();
    l.contains("startgame done")
        || l.contains("gameserver started")
        || l.contains("gamemanager.start")
        || l.contains("server is ready")
        || (l.contains("steam") && l.contains("logged on"))
}
