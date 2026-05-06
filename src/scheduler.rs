//! Scheduled tasks. Simple model: each task fires daily at HH:MM local
//! time. Persisted in `AppSettings::scheduled_tasks`.
//!
//! Supported actions:
//!   - `restart` — graceful stop, then start
//!   - `stop`    — graceful stop only
//!   - `start`   — start (if not running)
//!   - `backup`  — snapshot the current save
//!
//! For more elaborate schedules (specific weekdays, cron expressions),
//! extend `should_fire`.

use std::sync::Arc;
use std::time::Duration;

use chrono::{Datelike, Local, NaiveTime, Timelike};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::state::AppState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledTask {
    pub id: String,
    pub name: String,
    /// 24-hour clock, "HH:MM".
    pub at: String,
    pub action: ScheduledAction,
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Last fire timestamp (RFC3339), used to avoid double-firing.
    #[serde(default)]
    pub last_fired_iso: Option<String>,
}

fn default_true() -> bool { true }

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ScheduledAction {
    Restart,
    Stop,
    Start,
    Backup,
}

/// Spawn the scheduler loop. Wakes every 30s, fires due tasks, persists
/// `last_fired_iso` so a restart of the manager doesn't re-fire what
/// already happened today.
pub fn spawn(state: Arc<RwLock<AppState>>) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(30));
        loop {
            tick.tick().await;
            tick_once(&state).await;
        }
    });
}

async fn tick_once(state: &Arc<RwLock<AppState>>) {
    let now = Local::now();
    let now_min_key = format!("{:04}-{:02}-{:02}_{:02}:{:02}",
        now.year(), now.month(), now.day(), now.hour(), now.minute());

    // Snapshot the task list under read lock so we don't hold the write
    // lock during async work.
    let due: Vec<ScheduledTask> = {
        let s = state.read().await;
        s.settings
            .scheduled_tasks
            .iter()
            .filter(|t| t.enabled && is_due(t, &now_min_key, &now.time()))
            .cloned()
            .collect()
    };

    for task in due {
        run_task(state, &task).await;
        // Mark fired so we don't re-fire this minute.
        let mut s = state.write().await;
        if let Some(t) = s.settings.scheduled_tasks.iter_mut().find(|t| t.id == task.id) {
            t.last_fired_iso = Some(now_min_key.clone());
        }
        let _ = s.settings.save();
    }
}

fn is_due(task: &ScheduledTask, now_min_key: &str, now_time: &chrono::NaiveTime) -> bool {
    if task.last_fired_iso.as_deref() == Some(now_min_key) {
        return false; // already fired this minute
    }
    let parsed = NaiveTime::parse_from_str(&task.at, "%H:%M").ok();
    match parsed {
        Some(t) => t.hour() == now_time.hour() && t.minute() == now_time.minute(),
        None => false,
    }
}

async fn run_task(state: &Arc<RwLock<AppState>>, task: &ScheduledTask) {
    {
        let mut s = state.write().await;
        s.log_manager(format!("scheduled '{}' firing: {:?}", task.name, task.action));
    }
    match task.action {
        ScheduledAction::Restart => {
            let _ = crate::server::stop(state.clone()).await;
            // Wait briefly for the supervisor to mark Stopped.
            for _ in 0..40 {
                tokio::time::sleep(Duration::from_millis(500)).await;
                if !state.read().await.status.is_alive() { break; }
            }
            if let Err(e) = crate::server::start(state.clone()).await {
                state.write().await.log_manager(format!("scheduled restart failed: {e}"));
            }
        }
        ScheduledAction::Stop => {
            if let Err(e) = crate::server::stop(state.clone()).await {
                state.write().await.log_manager(format!("scheduled stop failed: {e}"));
            }
        }
        ScheduledAction::Start => {
            if let Err(e) = crate::server::start(state.clone()).await {
                state.write().await.log_manager(format!("scheduled start failed: {e}"));
            }
        }
        ScheduledAction::Backup => {
            let (saves_dir, backup_dir, save_name) = {
                let s = state.read().await;
                let saves = s.settings.saves_dir.clone();
                let backup_dir = s.settings.backup_dir.clone();
                let cfg_path = s.settings.resolve_config_path();
                let save_name = cfg_path.and_then(|p| {
                    crate::config::ServerConfig::load(&p).ok().and_then(|c| {
                        c.get("GameName").map(|v| v.to_string())
                    })
                });
                (saves, backup_dir, save_name)
            };
            match (saves_dir, backup_dir, save_name) {
                (Some(saves), Some(bdir), Some(name)) => {
                    match crate::backup::create(&saves, &name, &bdir, Some("scheduled".into())) {
                        Ok(b) => state.write().await.log_manager(
                            format!("scheduled backup ok: {}", b.path.display())),
                        Err(e) => state.write().await.log_manager(
                            format!("scheduled backup failed: {e}")),
                    }
                }
                _ => state.write().await.log_manager(
                    "scheduled backup skipped: saves_dir/backup_dir/GameName not all set"),
            }
        }
    }
}
