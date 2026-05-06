//! Shared application state. Wrapped in Arc<RwLock<...>> by `main`.

use std::collections::VecDeque;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::process::Child;
use tokio::sync::mpsc;

use crate::settings::AppSettings;
use crate::telnet::TelnetClient;

/// High-level lifecycle status of the dedicated server process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ServerStatus {
    Stopped,
    Starting,
    Running,
    Stopping,
    Crashed,
}

impl ServerStatus {
    pub fn is_alive(self) -> bool {
        matches!(self, ServerStatus::Starting | ServerStatus::Running | ServerStatus::Stopping)
    }
}

/// One line of log output from the server process. The kind is preserved
/// so the front-end can color stderr differently from stdout.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogLine {
    pub at: DateTime<Utc>,
    pub kind: LogKind,
    pub line: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogKind {
    Stdout,
    Stderr,
    Manager, // emitted by us, not the server
}

const MAX_LOG_LINES: usize = 1000;

#[derive(Debug)]
pub struct AppState {
    pub settings: AppSettings,
    pub status: ServerStatus,
    pub started_at: Option<DateTime<Utc>>,

    /// Handle to the running server child process. None when stopped.
    pub child: Option<Child>,

    /// Sender used by `server::stop` to ask the supervising task to kill
    /// the child. Drained when stopped.
    pub stop_tx: Option<mpsc::Sender<()>>,

    /// Telnet client, present only while the server is running and the
    /// telnet port is reachable + authenticated.
    pub telnet: Option<TelnetClient>,

    /// Bounded log buffer (most recent N lines).
    pub logs: VecDeque<LogLine>,
}

impl AppState {
    pub fn new(settings: AppSettings) -> Self {
        Self {
            settings,
            status: ServerStatus::Stopped,
            started_at: None,
            child: None,
            stop_tx: None,
            telnet: None,
            logs: VecDeque::with_capacity(MAX_LOG_LINES),
        }
    }

    pub fn push_log(&mut self, kind: LogKind, line: String) {
        if self.logs.len() == MAX_LOG_LINES {
            self.logs.pop_front();
        }
        self.logs.push_back(LogLine {
            at: Utc::now(),
            kind,
            line,
        });
    }

    /// Convenience: log a message from the manager itself.
    pub fn log_manager(&mut self, line: impl Into<String>) {
        let line = line.into();
        tracing::info!(target: "manager", "{}", line);
        self.push_log(LogKind::Manager, line);
    }

    pub fn uptime_seconds(&self) -> Option<i64> {
        self.started_at.map(|t| (Utc::now() - t).num_seconds())
    }
}
