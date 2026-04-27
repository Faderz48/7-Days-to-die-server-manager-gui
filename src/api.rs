//! HTTP JSON API used by the front-end.

use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json};
use axum::routing::{get, post};
use axum::Router;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::config::ServerConfig;
use crate::server;
use crate::settings::{AppSettings, Preset};
use crate::state::{AppState, LogLine, ServerStatus};
type SharedState = Arc<RwLock<AppState>>;

pub fn routes() -> Router<SharedState> {
    Router::new()
        .route("/api/status", get(get_status))
        .route("/api/config", get(get_config).put(put_config))
        .route("/api/start", post(post_start))
        .route("/api/stop", post(post_stop))
        .route("/api/logs", get(get_logs))
        .route("/api/maps", get(get_maps))
        .route("/api/seed", get(get_seed))
        .route("/api/settings", get(get_settings).put(put_settings))
        .route("/api/presets", get(get_presets).post(post_preset))
        .route("/api/presets/:name", post(apply_preset).delete(delete_preset))
        .route("/api/presets/import",       post(import_preset))
        .route("/api/presets/:name/export", post(export_preset))
        // Native folder/file picker
        .route("/api/pick-path", get(pick_path))
        // Telnet console
        .route("/api/console/exec", post(post_console_exec))
        // Server admin (admins/whitelist/blacklist/permissions)
        .route("/api/admin", get(get_admin).put(put_admin))
        // Backups
        .route("/api/backups", get(list_backups).post(create_backup))
        .route("/api/backups/restore", post(restore_backup))
        .route("/api/backups/delete",  post(delete_backup))
        // Scheduled tasks
        .route("/api/schedule", get(list_schedule).post(post_schedule).put(put_schedule))
        .route("/api/schedule/:id", axum::routing::delete(delete_schedule))
        // UPnP auto-forward
        .route("/api/upnp/forward", post(post_upnp_forward))
        .route("/api/upnp/unmap",   post(post_upnp_unmap))
        // Aggregated connection info: LAN IP, public IP, VPN IPs, port.
        // Used by the "share with friends" card so users don't have to
        // click around to find the address.
        .route("/api/connection-info", get(get_connection_info))
        // Worlds: list, download as zip, upload+extract zip
        .route("/api/worlds",                get(list_worlds))
        .route("/api/worlds/download/:name", get(download_world))
        .route("/api/worlds/upload",         post(upload_world))
        .route("/api/worlds/:name",          axum::routing::delete(delete_world))
        // Mods: list, upload+extract zip, delete
        .route("/api/mods",                  get(list_mods))
        .route("/api/mods/upload",           post(upload_mod))
        .route("/api/mods/:name",            axum::routing::delete(delete_mod))
        // Windows Firewall rules for the configured ServerPort.
        .route("/api/firewall/status", get(get_firewall_status))
        .route("/api/firewall/allow",  post(post_firewall_allow))
        .route("/api/firewall/remove", post(post_firewall_remove))
        // Big multipart bodies — worlds can be hundreds of MB.
        .layer(axum::extract::DefaultBodyLimit::max(2 * 1024 * 1024 * 1024))
        // VPN/LAN-emulator fallback (Hamachi, Radmin, Tailscale, ZeroTier)
        .route("/api/vpn-adapters", get(get_vpn_adapters))
}

// ─── Errors ──────────────────────────────────────────────────────────────

struct ApiError(StatusCode, String);

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        let body = Json(serde_json::json!({ "error": self.1 }));
        (self.0, body).into_response()
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        ApiError(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}"))
    }
}

type ApiResult<T> = Result<T, ApiError>;

// ─── Status ──────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct StatusResponse {
    status: ServerStatus,
    uptime_seconds: Option<i64>,
    install_dir: Option<PathBuf>,
    config_path: Option<PathBuf>,
    paths_ok: bool,
    telnet_attached: bool,
}

async fn get_status(State(state): State<SharedState>) -> Json<StatusResponse> {
    let s = state.read().await;
    let install_dir = s.settings.server_install_dir.clone();
    let config_path = s.settings.resolve_config_path();
    let paths_ok = match (&install_dir, &config_path) {
        (Some(d), Some(c)) => d.exists() && c.exists(),
        _ => false,
    };
    Json(StatusResponse {
        status: s.status,
        uptime_seconds: s.uptime_seconds(),
        install_dir,
        config_path,
        paths_ok,
        telnet_attached: s.telnet.is_some(),
    })
}

// ─── Server lifecycle ────────────────────────────────────────────────────

async fn post_start(State(state): State<SharedState>) -> ApiResult<Json<serde_json::Value>> {
    server::start(state).await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

async fn post_stop(State(state): State<SharedState>) -> ApiResult<Json<serde_json::Value>> {
    server::stop(state).await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

// ─── Logs ────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct LogsQuery {
    /// If supplied, only return lines newer than this index.
    since: Option<usize>,
}

#[derive(Serialize)]
struct LogsResponse {
    /// First index in the buffer corresponds to this number of lines
    /// having already been evicted, plus all current entries; useful for
    /// the front-end to ask for `since=last_index_seen`.
    next_since: usize,
    lines: Vec<LogLine>,
}

async fn get_logs(
    State(state): State<SharedState>,
    Query(q): Query<LogsQuery>,
) -> Json<LogsResponse> {
    let s = state.read().await;
    let total = s.logs.len();
    let since = q.since.unwrap_or(0).min(total);
    let lines: Vec<LogLine> = s.logs.iter().skip(since).cloned().collect();
    Json(LogsResponse {
        next_since: total,
        lines,
    })
}

// ─── Server config (XML) ─────────────────────────────────────────────────

#[derive(Serialize)]
struct ConfigResponse {
    properties: Vec<PropertyEntry>,
    raw_xml: String,
    config_path: Option<PathBuf>,
}

#[derive(Serialize, Deserialize, Clone)]
struct PropertyEntry {
    name: String,
    value: String,
}

async fn get_config(State(state): State<SharedState>) -> ApiResult<Json<ConfigResponse>> {
    let s = state.read().await;
    let path = s
        .settings
        .resolve_config_path()
        .ok_or_else(|| anyhow::anyhow!("server config path not set"))?;
    if !path.exists() {
        return Err(ApiError(
            StatusCode::NOT_FOUND,
            format!("serverconfig.xml not found at {}", path.display()),
        ));
    }
    let cfg = ServerConfig::load(&path)?;
    let properties = cfg
        .items
        .iter()
        .filter_map(|i| match i {
            crate::config::ConfigItem::Property { name, value } => Some(PropertyEntry {
                name: name.clone(),
                value: value.clone(),
            }),
            _ => None,
        })
        .collect();
    let raw_xml = cfg.to_xml()?;
    Ok(Json(ConfigResponse {
        properties,
        raw_xml,
        config_path: Some(path),
    }))
}

#[derive(Deserialize)]
struct PutConfigBody {
    properties: Vec<PropertyEntry>,
}

async fn put_config(
    State(state): State<SharedState>,
    Json(body): Json<PutConfigBody>,
) -> ApiResult<Json<serde_json::Value>> {
    let path = {
        let s = state.read().await;
        s.settings
            .resolve_config_path()
            .ok_or_else(|| anyhow::anyhow!("server config path not set"))?
    };

    // Load existing (so we preserve comments) or create an empty one.
    let mut cfg = if path.exists() {
        ServerConfig::load(&path)?
    } else {
        ServerConfig::default()
    };
    cfg.merge_properties(
        body.properties
            .into_iter()
            .map(|p| (p.name, p.value))
            .collect(),
    );
    cfg.save(&path)?;

    state
        .write()
        .await
        .log_manager(format!("saved config to {}", path.display()));

    Ok(Json(serde_json::json!({ "ok": true, "path": path })))
}

// ─── Maps ────────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct MapEntry {
    name: String,
    kind: &'static str, // "premade" or "generated"
}

#[derive(Serialize)]
struct MapsResponse {
    maps: Vec<MapEntry>,
}

async fn get_maps(State(state): State<SharedState>) -> Json<MapsResponse> {
    let s = state.read().await;
    let mut maps: Vec<MapEntry> = vec![
        MapEntry { name: "Navezgane".into(),         kind: "premade" },
        MapEntry { name: "PREGEN01".into(),          kind: "premade" },
        MapEntry { name: "PREGEN02".into(),          kind: "premade" },
        MapEntry { name: "PREGEN03".into(),          kind: "premade" },
        MapEntry { name: "PREGEN10k".into(),         kind: "premade" },
        MapEntry { name: "PREGEN8k".into(),          kind: "premade" },
        MapEntry { name: "PREGEN6k".into(),          kind: "premade" },
        MapEntry { name: "RWG".into(),               kind: "premade" }, // generic random
    ];

    if let Some(dir) = s.settings.generated_worlds_dir.as_ref() {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    if let Some(name) = entry.file_name().to_str() {
                        maps.push(MapEntry {
                            name: name.to_string(),
                            kind: "generated",
                        });
                    }
                }
            }
        }
    }

    Json(MapsResponse { maps })
}

// ─── Seed generation ─────────────────────────────────────────────────────

#[derive(Deserialize)]
struct SeedQuery {
    count: Option<usize>,
}

#[derive(Serialize)]
struct SeedResponse {
    seeds: Vec<String>,
}

async fn get_seed(Query(q): Query<SeedQuery>) -> Json<SeedResponse> {
    let n = q.count.unwrap_or(5).clamp(1, 50);
    Json(SeedResponse {
        seeds: crate::seed::generate_many(n),
    })
}

// ─── App settings (paths, etc.) ──────────────────────────────────────────

async fn get_settings(State(state): State<SharedState>) -> Json<AppSettings> {
    Json(state.read().await.settings.clone())
}

#[derive(Deserialize)]
struct PutSettingsBody {
    server_install_dir: Option<PathBuf>,
    server_config_path: Option<PathBuf>,
    saves_dir: Option<PathBuf>,
    generated_worlds_dir: Option<PathBuf>,
    backup_dir: Option<PathBuf>,
}

async fn put_settings(
    State(state): State<SharedState>,
    Json(body): Json<PutSettingsBody>,
) -> ApiResult<Json<AppSettings>> {
    let mut s = state.write().await;
    s.settings.server_install_dir = body.server_install_dir;
    s.settings.server_config_path = body.server_config_path;
    s.settings.saves_dir = body.saves_dir;
    s.settings.generated_worlds_dir = body.generated_worlds_dir;
    s.settings.backup_dir = body.backup_dir;
    s.settings.save()?;
    s.log_manager("app settings updated");
    Ok(Json(s.settings.clone()))
}

// ─── Presets (named saved server-config snapshots) ───────────────────────

async fn get_presets(State(state): State<SharedState>) -> Json<Vec<String>> {
    let s = state.read().await;
    Json(s.settings.presets.iter().map(|p| p.name.clone()).collect())
}

#[derive(Deserialize)]
struct PostPresetBody {
    name: String,
}

async fn post_preset(
    State(state): State<SharedState>,
    Json(body): Json<PostPresetBody>,
) -> ApiResult<Json<serde_json::Value>> {
    let mut s = state.write().await;
    let path = s
        .settings
        .resolve_config_path()
        .ok_or_else(|| anyhow::anyhow!("server config path not set"))?;
    if !path.exists() {
        return Err(ApiError(
            StatusCode::NOT_FOUND,
            format!("serverconfig.xml not found at {}", path.display()),
        ));
    }
    let xml = std::fs::read_to_string(&path).map_err(anyhow::Error::from)?;

    // Replace if a preset with this name already exists.
    s.settings.presets.retain(|p| p.name != body.name);
    s.settings.presets.push(Preset {
        name: body.name.clone(),
        xml,
    });
    s.settings.save()?;
    s.log_manager(format!("saved preset '{}'", body.name));
    Ok(Json(serde_json::json!({ "ok": true })))
}

async fn apply_preset(
    State(state): State<SharedState>,
    axum::extract::Path(name): axum::extract::Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let (xml, path) = {
        let s = state.read().await;
        let xml = s
            .settings
            .presets
            .iter()
            .find(|p| p.name == name)
            .ok_or_else(|| {
                ApiError(StatusCode::NOT_FOUND, format!("no preset named '{name}'"))
            })?
            .xml
            .clone();
        let path = s
            .settings
            .resolve_config_path()
            .ok_or_else(|| anyhow::anyhow!("server config path not set"))?;
        (xml, path)
    };
    std::fs::write(&path, xml).map_err(anyhow::Error::from)?;
    state
        .write()
        .await
        .log_manager(format!("applied preset '{name}'"));
    Ok(Json(serde_json::json!({ "ok": true })))
}

async fn delete_preset(
    State(state): State<SharedState>,
    axum::extract::Path(name): axum::extract::Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let mut s = state.write().await;
    let before = s.settings.presets.len();
    s.settings.presets.retain(|p| p.name != name);
    if s.settings.presets.len() == before {
        return Err(ApiError(
            StatusCode::NOT_FOUND,
            format!("no preset named '{name}'"),
        ));
    }
    s.settings.save()?;
    s.log_manager(format!("deleted preset '{name}'"));
    Ok(Json(serde_json::json!({ "ok": true })))
}

// ─── Telnet console ──────────────────────────────────────────────────────

#[derive(Deserialize)]
struct ConsoleExecBody { command: String }

async fn post_console_exec(
    State(state): State<SharedState>,
    Json(body): Json<ConsoleExecBody>,
) -> ApiResult<Json<serde_json::Value>> {
    let cmd = body.command.trim().to_string();
    if cmd.is_empty() {
        return Err(ApiError(StatusCode::BAD_REQUEST, "empty command".into()));
    }
    let client = state.read().await.telnet.clone();
    let client = client.ok_or_else(|| ApiError(
        StatusCode::SERVICE_UNAVAILABLE,
        "telnet not attached — start the server with TelnetEnabled=true and a TelnetPassword".into(),
    ))?;
    state.write().await.log_manager(format!("> {cmd}"));
    client.send(&cmd).await.map_err(anyhow::Error::from)?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

// ─── serveradmin.xml ─────────────────────────────────────────────────────

fn admin_path(state: &AppState) -> Option<PathBuf> {
    let cfg_path = state.settings.resolve_config_path()?;
    // serveradmin.xml lives in the SaveGameFolder. The default is to
    // sit alongside the save data — for our purposes we put it next to
    // serverconfig.xml as a sensible fallback. The game looks at
    // AdminFileName in the config.
    let install_dir = cfg_path.parent()?.to_path_buf();
    let cfg = crate::config::ServerConfig::load(&cfg_path).ok()?;
    let name = cfg.get("AdminFileName").unwrap_or("serveradmin.xml").to_string();
    Some(install_dir.join(name))
}

async fn get_admin(State(state): State<SharedState>) -> ApiResult<Json<crate::admin::AdminFile>> {
    let path = {
        let s = state.read().await;
        admin_path(&s).ok_or_else(|| anyhow::anyhow!("could not resolve serveradmin.xml path"))?
    };
    let af = crate::admin::AdminFile::load(&path)?;
    Ok(Json(af))
}

async fn put_admin(
    State(state): State<SharedState>,
    Json(body): Json<crate::admin::AdminFile>,
) -> ApiResult<Json<serde_json::Value>> {
    let path = {
        let s = state.read().await;
        admin_path(&s).ok_or_else(|| anyhow::anyhow!("could not resolve serveradmin.xml path"))?
    };
    body.save(&path)?;
    state.write().await.log_manager(format!("saved {}", path.display()));
    Ok(Json(serde_json::json!({ "ok": true, "path": path })))
}

// ─── Backups ─────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct ListBackupsResponse { backups: Vec<crate::backup::BackupEntry> }

async fn list_backups(State(state): State<SharedState>) -> ApiResult<Json<ListBackupsResponse>> {
    let dir = {
        let s = state.read().await;
        s.settings.backup_dir.clone()
            .ok_or_else(|| anyhow::anyhow!("backup directory not configured"))?
    };
    let backups = crate::backup::list(&dir)?;
    Ok(Json(ListBackupsResponse { backups }))
}

#[derive(Deserialize)]
struct CreateBackupBody {
    /// Optional override; defaults to current GameName from serverconfig.xml.
    save_name: Option<String>,
    note: Option<String>,
}

async fn create_backup(
    State(state): State<SharedState>,
    Json(body): Json<CreateBackupBody>,
) -> ApiResult<Json<crate::backup::BackupEntry>> {
    let (saves_dir, backup_dir, default_name) = {
        let s = state.read().await;
        let saves = s.settings.saves_dir.clone()
            .ok_or_else(|| anyhow::anyhow!("saves directory not configured"))?;
        let bdir = s.settings.backup_dir.clone()
            .ok_or_else(|| anyhow::anyhow!("backup directory not configured"))?;
        let default_name = s.settings.resolve_config_path().and_then(|p| {
            crate::config::ServerConfig::load(&p).ok()
                .and_then(|c| c.get("GameName").map(|v| v.to_string()))
        });
        (saves, bdir, default_name)
    };
    let name = body.save_name.or(default_name)
        .ok_or_else(|| ApiError(StatusCode::BAD_REQUEST, "save_name required and GameName not set".into()))?;
    let entry = crate::backup::create(&saves_dir, &name, &backup_dir, body.note)?;
    state.write().await.log_manager(format!("backup created: {}", entry.path.display()));
    Ok(Json(entry))
}

#[derive(Deserialize)]
struct BackupPathBody { path: PathBuf }

async fn restore_backup(
    State(state): State<SharedState>,
    Json(body): Json<BackupPathBody>,
) -> ApiResult<Json<serde_json::Value>> {
    // Refuse to restore over a running server — that corrupts the save.
    {
        let s = state.read().await;
        if s.status.is_alive() {
            return Err(ApiError(
                StatusCode::CONFLICT,
                "stop the server before restoring a backup".into(),
            ));
        }
    }
    let saves_dir = {
        let s = state.read().await;
        s.settings.saves_dir.clone()
            .ok_or_else(|| anyhow::anyhow!("saves directory not configured"))?
    };
    crate::backup::restore(&saves_dir, &body.path)?;
    state.write().await.log_manager(format!("backup restored from {}", body.path.display()));
    Ok(Json(serde_json::json!({ "ok": true })))
}

async fn delete_backup(
    State(state): State<SharedState>,
    Json(body): Json<BackupPathBody>,
) -> ApiResult<Json<serde_json::Value>> {
    crate::backup::delete(&body.path)?;
    state.write().await.log_manager(format!("backup deleted: {}", body.path.display()));
    Ok(Json(serde_json::json!({ "ok": true })))
}

// ─── Scheduled tasks ─────────────────────────────────────────────────────

async fn list_schedule(State(state): State<SharedState>) -> Json<Vec<crate::scheduler::ScheduledTask>> {
    Json(state.read().await.settings.scheduled_tasks.clone())
}

async fn post_schedule(
    State(state): State<SharedState>,
    Json(mut task): Json<crate::scheduler::ScheduledTask>,
) -> ApiResult<Json<crate::scheduler::ScheduledTask>> {
    if task.id.is_empty() {
        // Random short id.
        use rand::Rng;
        let n: u32 = rand::thread_rng().gen();
        task.id = format!("t{:x}", n);
    }
    let mut s = state.write().await;
    s.settings.scheduled_tasks.retain(|t| t.id != task.id);
    s.settings.scheduled_tasks.push(task.clone());
    s.settings.save()?;
    s.log_manager(format!("scheduled task '{}' saved", task.name));
    Ok(Json(task))
}

async fn put_schedule(
    State(state): State<SharedState>,
    Json(task): Json<crate::scheduler::ScheduledTask>,
) -> ApiResult<Json<crate::scheduler::ScheduledTask>> {
    let mut s = state.write().await;
    let found = s.settings.scheduled_tasks.iter_mut().find(|t| t.id == task.id);
    match found {
        Some(slot) => *slot = task.clone(),
        None => return Err(ApiError(StatusCode::NOT_FOUND, "no such task".into())),
    }
    s.settings.save()?;
    Ok(Json(task))
}

async fn delete_schedule(
    State(state): State<SharedState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let mut s = state.write().await;
    let before = s.settings.scheduled_tasks.len();
    s.settings.scheduled_tasks.retain(|t| t.id != id);
    if s.settings.scheduled_tasks.len() == before {
        return Err(ApiError(StatusCode::NOT_FOUND, "no such task".into()));
    }
    s.settings.save()?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

// ─── Native file/folder picker ───────────────────────────────────────────

#[derive(Deserialize)]
struct PickQuery {
    /// "dir" (default) or "file".
    #[serde(default)]
    kind: Option<String>,
    /// Dialog title.
    #[serde(default)]
    title: Option<String>,
    /// Optional starting directory.
    #[serde(default)]
    start: Option<String>,
}

async fn pick_path(Query(q): Query<PickQuery>) -> ApiResult<Json<serde_json::Value>> {
    let kind  = q.kind.unwrap_or_else(|| "dir".into());
    let title = q.title.unwrap_or_else(|| "Select…".into());
    let start = q.start;

    // rfd is synchronous — run on a blocking thread so the tokio runtime
    // stays responsive while the user interacts with the OS dialog.
    let path = tokio::task::spawn_blocking(move || {
        crate::dialog_focus::focus_dialogs_for_this_thread();
        let mut dlg = rfd::FileDialog::new().set_title(&title);
        if let Some(s) = start.filter(|p| !p.is_empty()) {
            dlg = dlg.set_directory(s);
        }
        if kind == "file" { dlg.pick_file() } else { dlg.pick_folder() }
    })
    .await
    .map_err(|e| anyhow::anyhow!("dialog task panicked: {e}"))?;

    Ok(Json(serde_json::json!({
        "path": path.map(|p| p.to_string_lossy().to_string()),
    })))
}

// ─── Preset import / export ─────────────────────────────────────────────

#[derive(Deserialize)]
struct ImportPresetQuery {
    /// Optional new name. If omitted, we use the file's stem.
    #[serde(default)]
    name: Option<String>,
}

async fn import_preset(
    State(state): State<SharedState>,
    Query(q): Query<ImportPresetQuery>,
) -> ApiResult<Json<Preset>> {
    // Open a native file picker filtered to xml.
    let picked = tokio::task::spawn_blocking(|| {
        crate::dialog_focus::focus_dialogs_for_this_thread();
        rfd::FileDialog::new()
            .set_title("Pick a preset XML to import")
            .add_filter("Server config / preset", &["xml"])
            .add_filter("Any", &["*"])
            .pick_file()
    })
    .await
    .map_err(|e| anyhow::anyhow!("dialog task panicked: {e}"))?;

    let Some(path) = picked else {
        return Err(ApiError(StatusCode::BAD_REQUEST, "import cancelled".into()));
    };

    let xml = std::fs::read_to_string(&path)
        .map_err(|e| anyhow::anyhow!("could not read {}: {e}", path.display()))?;

    // Sanity-check it parses as a serverconfig.xml. We don't reject on
    // missing properties, just on hard XML errors.
    if let Err(e) = ServerConfig::parse(&xml) {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            format!("file does not look like a valid serverconfig.xml: {e}"),
        ));
    }

    // Decide the preset name: explicit > file stem > "imported".
    let name = q.name
        .filter(|s| !s.trim().is_empty())
        .or_else(|| {
            path.file_stem()
                .and_then(|s| s.to_str())
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| "imported".to_string());

    let preset = Preset { name: name.clone(), xml };

    let mut s = state.write().await;
    s.settings.presets.retain(|p| p.name != name);
    s.settings.presets.push(preset.clone());
    s.settings.save()?;
    s.log_manager(format!("preset '{}' imported from {}", name, path.display()));
    Ok(Json(preset))
}

async fn export_preset(
    State(state): State<SharedState>,
    axum::extract::Path(name): axum::extract::Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    // Look up the preset under read lock.
    let preset = {
        let s = state.read().await;
        s.settings.presets.iter().find(|p| p.name == name).cloned()
    };
    let Some(preset) = preset else {
        return Err(ApiError(StatusCode::NOT_FOUND, format!("no preset named '{name}'")));
    };

    // Suggest a sensible filename: <name>.xml.
    let suggested = format!("{}.xml", sanitize_filename(&preset.name));

    let dest = tokio::task::spawn_blocking(move || {
        crate::dialog_focus::focus_dialogs_for_this_thread();
        rfd::FileDialog::new()
            .set_title("Save preset XML…")
            .set_file_name(&suggested)
            .add_filter("Server config / preset", &["xml"])
            .save_file()
    })
    .await
    .map_err(|e| anyhow::anyhow!("dialog task panicked: {e}"))?;

    let Some(dest) = dest else {
        return Err(ApiError(StatusCode::BAD_REQUEST, "export cancelled".into()));
    };

    std::fs::write(&dest, preset.xml.as_bytes())
        .map_err(|e| anyhow::anyhow!("could not write {}: {e}", dest.display()))?;
    state.write().await.log_manager(format!("preset '{}' exported to {}", preset.name, dest.display()));

    Ok(Json(serde_json::json!({
        "ok": true,
        "path": dest.to_string_lossy(),
    })))
}

/// Strip characters that aren't safe across Windows/macOS/Linux file systems.
fn sanitize_filename(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect::<String>()
        .trim()
        .to_string()
}

// ─── UPnP auto port-forward ──────────────────────────────────────────────

fn resolve_server_port(state: &AppState) -> Result<u16, ApiError> {
    let path = state.settings.resolve_config_path().ok_or_else(|| {
        ApiError(StatusCode::BAD_REQUEST, "server config path not set".into())
    })?;
    let cfg = ServerConfig::load(&path).map_err(anyhow::Error::from)?;
    let port_str = cfg.get("ServerPort").ok_or_else(|| {
        ApiError(StatusCode::BAD_REQUEST, "ServerPort not set in serverconfig.xml".into())
    })?;
    port_str.parse::<u16>().map_err(|e| {
        ApiError(StatusCode::BAD_REQUEST, format!("ServerPort '{port_str}' is not a valid u16: {e}"))
    })
}

async fn post_upnp_forward(
    State(state): State<SharedState>,
) -> ApiResult<Json<crate::upnp::ForwardResult>> {
    let port = {
        let s = state.read().await;
        resolve_server_port(&s)?
    };
    state.write().await.log_manager(format!("UPnP: requesting forward of {port}/{}/{}", port + 1, port + 2));
    let result = crate::upnp::forward(port).await.map_err(anyhow::Error::from)?;
    {
        let mut s = state.write().await;
        s.log_manager(format!(
            "UPnP: mapped {} ports, public IP {}, CGNAT={}",
            result.mapped_ports.len(),
            result.public_ip.map(|i| i.to_string()).unwrap_or_else(|| "unknown".into()),
            result.cgnat,
        ));
        for n in &result.notes {
            s.log_manager(format!("UPnP: {n}"));
        }
    }
    Ok(Json(result))
}

async fn post_upnp_unmap(
    State(state): State<SharedState>,
) -> ApiResult<Json<crate::upnp::UnmapResult>> {
    let port = {
        let s = state.read().await;
        resolve_server_port(&s)?
    };
    state.write().await.log_manager(format!("UPnP: removing mapping for {port}/{}/{}", port + 1, port + 2));
    let result = crate::upnp::unmap(port).await.map_err(anyhow::Error::from)?;
    state.write().await.log_manager(format!("UPnP: removed {} mappings", result.removed_ports.len()));
    Ok(Json(result))
}

// ─── VPN adapter detection ──────────────────────────────────────────────
//
// Used by the front-end to surface Hamachi/Radmin/Tailscale/ZeroTier IPs
// as a fallback for users who can't get port forwarding to work.

async fn get_vpn_adapters() -> Json<Vec<crate::netinfo::VpnAdapter>> {
    // Could be slow on a machine with many adapters — run on the
    // blocking pool so we don't park the runtime.
    let adapters = tokio::task::spawn_blocking(crate::netinfo::detect_adapters)
        .await
        .unwrap_or_default();
    Json(adapters)
}

// ─── Connection info aggregator ─────────────────────────────────────────
//
// Surfaces every address a friend could potentially use to connect, so
// the user can copy them with one click instead of digging through the
// UPnP panel and the VPN panel separately.

#[derive(Serialize)]
struct ConnectionEndpoint {
    /// Short label, e.g. "Internet (public IP)" or "Hamachi".
    label: String,
    /// Where this address would be most useful. Helps the UI sort.
    /// One of: "internet", "lan", "vpn".
    kind: String,
    /// Bare IP, no port — the front-end formats `ip:port` for display.
    ip: String,
    /// Free-text caveat, e.g. "CGNAT — outsiders can't reach you".
    note: Option<String>,
}

#[derive(Serialize)]
struct ConnectionInfo {
    /// `ServerPort` from serverconfig.xml, or null if not configured.
    port: Option<u16>,
    /// Endpoints worth showing the user, friendliest first.
    endpoints: Vec<ConnectionEndpoint>,
}

async fn get_connection_info(State(state): State<SharedState>) -> Json<ConnectionInfo> {
    // Pull the configured ServerPort. Don't error out if anything's
    // missing — we still want to show LAN / VPN info.
    let port: Option<u16> = {
        let s = state.read().await;
        s.settings.resolve_config_path().and_then(|p| {
            ServerConfig::load(&p).ok().and_then(|c| {
                c.get("ServerPort").and_then(|v| v.parse::<u16>().ok())
            })
        })
    };

    let mut endpoints: Vec<ConnectionEndpoint> = Vec::new();

    // ── Internet / public IP via UPnP (best-effort, 3s timeout) ────────
    // We also annotate CGNAT here so users immediately see why their
    // public IP "doesn't work" if their ISP double-NATs them.
    if let Some(public) = crate::upnp::query_public_ip().await {
        let cgnat = is_cgnat_or_private(public);
        endpoints.push(ConnectionEndpoint {
            label: "Internet (public IP)".into(),
            kind: "internet".into(),
            ip: public.to_string(),
            note: if cgnat {
                Some("This IP is private/CGNAT — outside players can't reach \
                      you here. Use the VPN fallback below.".into())
            } else {
                Some("Share this with friends NOT on your home network. \
                      Requires port forwarding to be set up.".into())
            },
        });
    }

    // ── LAN IP for friends sitting on the same network ─────────────────
    if let Some(lan) = crate::upnp::detect_lan_ip() {
        endpoints.push(ConnectionEndpoint {
            label: "Local network (LAN)".into(),
            kind: "lan".into(),
            ip: lan.to_string(),
            note: Some("For people on the same Wi-Fi / LAN as you.".into()),
        });
    }

    // ── VPN adapters — give each one its own row ───────────────────────
    let adapters = tokio::task::spawn_blocking(crate::netinfo::detect_adapters)
        .await
        .unwrap_or_default();
    for a in adapters {
        endpoints.push(ConnectionEndpoint {
            label: a.display_name.clone(),
            kind: "vpn".into(),
            ip: a.ip.to_string(),
            note: Some(format!(
                "For friends on your {} network. No port forwarding needed.",
                a.display_name,
            )),
        });
    }

    Json(ConnectionInfo { port, endpoints })
}

/// Helper: same CGNAT/private check as in upnp.rs but kept local so the
/// api module doesn't need to import internals.
fn is_cgnat_or_private(ip: std::net::IpAddr) -> bool {
    let std::net::IpAddr::V4(v4) = ip else { return true; };
    if v4.is_private() || v4.is_loopback() || v4.is_link_local() || v4.is_unspecified() {
        return true;
    }
    // RFC 6598: 100.64.0.0/10
    let o = v4.octets();
    o[0] == 100 && (o[1] & 0xC0) == 0x40
}

// ─── Worlds: list / download / upload ────────────────────────────────────

async fn list_worlds(
    State(state): State<SharedState>,
) -> ApiResult<Json<Vec<crate::worlds::WorldEntry>>> {
    let saves_dir = {
        let s = state.read().await;
        s.settings.saves_dir.clone()
            .ok_or_else(|| anyhow::anyhow!("saves directory not configured"))?
    };
    // fs::read_dir is fast in absolute terms but blocking-thread-safe.
    let list = tokio::task::spawn_blocking(move || crate::worlds::list_saves(&saves_dir))
        .await
        .map_err(|e| anyhow::anyhow!("list task panicked: {e}"))??;
    Ok(Json(list))
}

async fn download_world(
    State(state): State<SharedState>,
    axum::extract::Path(name): axum::extract::Path<String>,
) -> ApiResult<axum::response::Response> {
    use axum::body::Body;
    use axum::http::header;

    // Resolve paths under the read lock, then drop it before we do
    // heavy work.
    let saves_dir = {
        let s = state.read().await;
        s.settings.saves_dir.clone()
            .ok_or_else(|| anyhow::anyhow!("saves directory not configured"))?
    };

    // Stage the zip under a temp file so the streaming response is just
    // a file read — way cheaper than buffering hundreds of MB in memory.
    let tmp_path = std::env::temp_dir()
        .join(format!("sdtd-export-{}-{}.zip",
            sanitize_filename(&name),
            chrono::Utc::now().format("%Y%m%d%H%M%S")));

    let pack_name = name.clone();
    let pack_tmp  = tmp_path.clone();
    tokio::task::spawn_blocking(move || -> anyhow::Result<u64> {
        let f = std::fs::File::create(&pack_tmp)
            .map_err(|e| anyhow::anyhow!("creating temp zip {}: {e}", pack_tmp.display()))?;
        let buf = std::io::BufWriter::new(f);
        crate::worlds::pack_save(&saves_dir, &pack_name, buf)
    })
    .await
    .map_err(|e| anyhow::anyhow!("pack task panicked: {e}"))?
    .map_err(anyhow::Error::from)?;

    state.write().await.log_manager(format!("world '{name}' packed for download"));

    // Stream the file back. We delete it after the response is built —
    // axum's `Body::from_stream` reads it lazily, so we use ReaderStream
    // and let the file handle close itself when the body ends.
    let file = tokio::fs::File::open(&tmp_path).await
        .map_err(|e| anyhow::anyhow!("opening packed zip: {e}"))?;
    let size = file.metadata().await.ok().map(|m| m.len()).unwrap_or(0);
    let stream = tokio_util::io::ReaderStream::new(file);
    let body = Body::from_stream(stream);

    // Best-effort cleanup. The temp file is unlinked but the OS keeps
    // the inode alive while our open handle reads from it.
    let _ = tokio::fs::remove_file(&tmp_path).await;

    let filename = format!("{}.zip", sanitize_filename(&name));
    let response = axum::response::Response::builder()
        .header(header::CONTENT_TYPE, "application/zip")
        .header(header::CONTENT_LENGTH, size)
        .header(header::CONTENT_DISPOSITION,
                format!(r#"attachment; filename="{filename}""#))
        .body(body)
        .map_err(|e| anyhow::anyhow!("building response: {e}"))?;
    Ok(response)
}

#[derive(Serialize)]
struct UploadWorldResponse {
    save_name: String,
    bytes_extracted: u64,
    file_count: u64,
}

async fn upload_world(
    State(state): State<SharedState>,
    mut multipart: axum::extract::Multipart,
) -> ApiResult<Json<UploadWorldResponse>> {
    use tokio::io::AsyncWriteExt;

    // Block uploads while the server is alive — extracting over a live
    // save will corrupt it.
    {
        let s = state.read().await;
        if s.status.is_alive() {
            return Err(ApiError(StatusCode::CONFLICT,
                "stop the server before uploading a world".into()));
        }
    }

    let saves_dir = {
        let s = state.read().await;
        s.settings.saves_dir.clone()
            .ok_or_else(|| anyhow::anyhow!("saves directory not configured"))?
    };

    // Multipart fields. We expect a `file` (the .zip), and two optional
    // text fields: `name` (override the save name) and `overwrite` ("true"/"false").
    let mut tmp_zip: Option<PathBuf> = None;
    let mut forced_name: Option<String> = None;
    let mut overwrite = false;

    while let Some(mut field) = multipart.next_field().await
        .map_err(|e| ApiError(StatusCode::BAD_REQUEST, format!("bad multipart: {e}")))?
    {
        let field_name = field.name().unwrap_or("").to_string();
        match field_name.as_str() {
            "file" => {
                // Use a high-entropy temp name to avoid collisions when
                // multiple uploads happen back-to-back.
                let stamp: u128 = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or(0);
                let dest = std::env::temp_dir().join(format!("sdtd-upload-{stamp}.zip"));
                let mut f = tokio::fs::File::create(&dest).await
                    .map_err(|e| anyhow::anyhow!("opening temp upload: {e}"))?;
                while let Some(chunk) = field.chunk().await
                    .map_err(|e| ApiError(StatusCode::BAD_REQUEST, format!("upload error: {e}")))?
                {
                    f.write_all(&chunk).await
                        .map_err(|e| anyhow::anyhow!("writing temp upload: {e}"))?;
                }
                f.flush().await.ok();
                tmp_zip = Some(dest);
            }
            "name" => {
                forced_name = Some(field.text().await
                    .map_err(|e| ApiError(StatusCode::BAD_REQUEST, format!("bad name field: {e}")))?
                    .trim().to_string())
                    .filter(|s| !s.is_empty());
            }
            "overwrite" => {
                let v = field.text().await.unwrap_or_default();
                overwrite = matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on");
            }
            _ => {} // ignore unknown fields
        }
    }

    let tmp_zip = tmp_zip.ok_or_else(|| ApiError(
        StatusCode::BAD_REQUEST,
        "no 'file' field in upload".into(),
    ))?;

    // Heavy work off the runtime.
    let saves = saves_dir.clone();
    let zip_path = tmp_zip.clone();
    let name_for_extract = forced_name.clone();
    let result = tokio::task::spawn_blocking(move || {
        crate::worlds::import_zip(
            &zip_path,
            &saves,
            name_for_extract.as_deref(),
            overwrite,
        )
    })
    .await
    .map_err(|e| anyhow::anyhow!("extract task panicked: {e}"))?
    .map_err(anyhow::Error::from)?;

    // Clean up the upload temp file.
    let _ = tokio::fs::remove_file(&tmp_zip).await;

    state.write().await.log_manager(format!(
        "world '{}' uploaded — {} files, {} bytes",
        result.save_name, result.file_count, result.bytes_extracted,
    ));

    Ok(Json(UploadWorldResponse {
        save_name: result.save_name,
        bytes_extracted: result.bytes_extracted,
        file_count: result.file_count,
    }))
}

// ─── Worlds: delete ──────────────────────────────────────────────────────

#[derive(Serialize)]
struct DeleteResponse {
    name: String,
    bytes_freed: u64,
}

async fn delete_world(
    State(state): State<SharedState>,
    axum::extract::Path(name): axum::extract::Path<String>,
) -> ApiResult<Json<DeleteResponse>> {
    // Refuse if the server is alive — deleting the active save folder
    // out from under the running game is a recipe for corrupted data.
    {
        let s = state.read().await;
        if s.status.is_alive() {
            return Err(ApiError(
                StatusCode::CONFLICT,
                "stop the server before deleting a world".into(),
            ));
        }
    }

    let saves_dir = {
        let s = state.read().await;
        s.settings.saves_dir.clone()
            .ok_or_else(|| anyhow::anyhow!("saves directory not configured"))?
    };

    let n = name.clone();
    let bytes = tokio::task::spawn_blocking(move || crate::worlds::delete_save(&saves_dir, &n))
        .await
        .map_err(|e| anyhow::anyhow!("delete task panicked: {e}"))?
        .map_err(anyhow::Error::from)?;

    state.write().await.log_manager(format!(
        "world '{name}' deleted ({} bytes freed)", bytes,
    ));
    Ok(Json(DeleteResponse { name, bytes_freed: bytes }))
}

// ─── Mods: list / upload / delete ────────────────────────────────────────

fn resolve_mods_dir_blocking(state: &AppState) -> Result<PathBuf, ApiError> {
    let install = state.settings.server_install_dir.clone()
        .ok_or_else(|| ApiError(
            StatusCode::BAD_REQUEST,
            "server install directory not configured — set it in APP CONFIG first".into(),
        ))?;
    crate::mods::resolve_mods_dir(&install).map_err(ApiError::from)
}

async fn list_mods(
    State(state): State<SharedState>,
) -> ApiResult<Json<Vec<crate::mods::ModEntry>>> {
    let mods_dir = {
        let s = state.read().await;
        resolve_mods_dir_blocking(&s)?
    };
    let list = tokio::task::spawn_blocking(move || crate::mods::list_mods(&mods_dir))
        .await
        .map_err(|e| anyhow::anyhow!("list task panicked: {e}"))??;
    Ok(Json(list))
}

#[derive(Serialize)]
struct InstallModResponse {
    mod_name: String,
    bytes_extracted: u64,
    file_count: u64,
    layout: String,
}

async fn upload_mod(
    State(state): State<SharedState>,
    mut multipart: axum::extract::Multipart,
) -> ApiResult<Json<InstallModResponse>> {
    use tokio::io::AsyncWriteExt;

    // Block while server is alive — extracting over loaded mod files is
    // unreliable. Windows actually denies the unlink; Linux lets it
    // happen but you'll get bizarre runtime errors.
    {
        let s = state.read().await;
        if s.status.is_alive() {
            return Err(ApiError(StatusCode::CONFLICT,
                "stop the server before installing a mod".into()));
        }
    }

    let mods_dir = {
        let s = state.read().await;
        resolve_mods_dir_blocking(&s)?
    };

    // Buffer the multipart body to a temp file before extracting. Same
    // shape as upload_world.
    let mut tmp_zip: Option<PathBuf> = None;
    let mut forced_name: Option<String> = None;
    let mut overwrite = false;

    while let Some(mut field) = multipart.next_field().await
        .map_err(|e| ApiError(StatusCode::BAD_REQUEST, format!("bad multipart: {e}")))?
    {
        let field_name = field.name().unwrap_or("").to_string();
        match field_name.as_str() {
            "file" => {
                let stamp: u128 = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or(0);
                let dest = std::env::temp_dir().join(format!("sdtd-mod-upload-{stamp}.zip"));
                let mut f = tokio::fs::File::create(&dest).await
                    .map_err(|e| anyhow::anyhow!("opening temp upload: {e}"))?;
                while let Some(chunk) = field.chunk().await
                    .map_err(|e| ApiError(StatusCode::BAD_REQUEST, format!("upload error: {e}")))?
                {
                    f.write_all(&chunk).await
                        .map_err(|e| anyhow::anyhow!("writing temp upload: {e}"))?;
                }
                f.flush().await.ok();
                tmp_zip = Some(dest);
            }
            "name" => {
                forced_name = Some(field.text().await
                    .map_err(|e| ApiError(StatusCode::BAD_REQUEST, format!("bad name field: {e}")))?
                    .trim().to_string())
                    .filter(|s| !s.is_empty());
            }
            "overwrite" => {
                let v = field.text().await.unwrap_or_default();
                overwrite = matches!(v.trim().to_ascii_lowercase().as_str(),
                                     "1" | "true" | "yes" | "on");
            }
            _ => {}
        }
    }

    let tmp_zip = tmp_zip.ok_or_else(|| ApiError(
        StatusCode::BAD_REQUEST,
        "no 'file' field in upload".into(),
    ))?;

    let target = mods_dir.clone();
    let zip_path = tmp_zip.clone();
    let name_for_extract = forced_name.clone();
    let result = tokio::task::spawn_blocking(move || {
        crate::mods::install_zip(&zip_path, &target, name_for_extract.as_deref(), overwrite)
    })
    .await
    .map_err(|e| anyhow::anyhow!("install task panicked: {e}"))?
    .map_err(anyhow::Error::from)?;

    let _ = tokio::fs::remove_file(&tmp_zip).await;

    state.write().await.log_manager(format!(
        "mod '{}' installed ({} layout, {} files, {} bytes)",
        result.mod_name, result.layout, result.file_count, result.bytes_extracted,
    ));

    Ok(Json(InstallModResponse {
        mod_name: result.mod_name,
        bytes_extracted: result.bytes_extracted,
        file_count: result.file_count,
        layout: result.layout,
    }))
}

async fn delete_mod(
    State(state): State<SharedState>,
    axum::extract::Path(name): axum::extract::Path<String>,
) -> ApiResult<Json<DeleteResponse>> {
    {
        let s = state.read().await;
        if s.status.is_alive() {
            return Err(ApiError(
                StatusCode::CONFLICT,
                "stop the server before deleting a mod".into(),
            ));
        }
    }

    let mods_dir = {
        let s = state.read().await;
        resolve_mods_dir_blocking(&s)?
    };

    let n = name.clone();
    let bytes = tokio::task::spawn_blocking(move || crate::mods::delete_mod(&mods_dir, &n))
        .await
        .map_err(|e| anyhow::anyhow!("delete task panicked: {e}"))?
        .map_err(anyhow::Error::from)?;

    state.write().await.log_manager(format!(
        "mod '{name}' deleted ({} bytes freed)", bytes,
    ));
    Ok(Json(DeleteResponse { name, bytes_freed: bytes }))
}

// ─── Firewall rules ──────────────────────────────────────────────────────

async fn get_firewall_status() -> Json<crate::firewall::RuleStatus> {
    // netsh `show rule name=all` is fast (~50ms) but blocks — keep it
    // off the runtime.
    let s = tokio::task::spawn_blocking(crate::firewall::status)
        .await
        .unwrap_or_else(|_| crate::firewall::RuleStatus {
            any_present: false,
            present: Vec::new(),
            unsupported: false,
        });
    Json(s)
}

async fn post_firewall_allow(
    State(state): State<SharedState>,
) -> ApiResult<Json<crate::firewall::RuleResult>> {
    let port = {
        let s = state.read().await;
        resolve_server_port(&s)?
    };
    state.write().await.log_manager(format!(
        "firewall: adding allow rules for {port}, {}, {}", port + 1, port + 2,
    ));
    let r = tokio::task::spawn_blocking(move || crate::firewall::add_rules(port))
        .await
        .map_err(|e| anyhow::anyhow!("firewall task panicked: {e}"))?;
    {
        let mut s = state.write().await;
        s.log_manager(format!(
            "firewall: added {} ports", r.added_ports.len(),
        ));
        for n in &r.notes {
            s.log_manager(format!("firewall: {n}"));
        }
    }
    // If we couldn't add a single rule and the failure mentions
    // elevation, surface that as a 403 so the UI can show a helpful
    // "run as admin" message instead of a generic error.
    if r.added_ports.is_empty() && r.notes.iter().any(|n| n.contains("administrator")) {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            r.notes.first().cloned().unwrap_or_else(||
                "administrator privileges required".into()),
        ));
    }
    Ok(Json(r))
}

async fn post_firewall_remove(
    State(state): State<SharedState>,
) -> ApiResult<Json<crate::firewall::RuleResult>> {
    state.write().await.log_manager("firewall: removing our allow rules");
    let r = tokio::task::spawn_blocking(crate::firewall::remove_rules)
        .await
        .map_err(|e| anyhow::anyhow!("firewall task panicked: {e}"))?;
    {
        let mut s = state.write().await;
        s.log_manager(format!(
            "firewall: removed {} ports", r.removed_ports.len(),
        ));
        for n in &r.notes {
            s.log_manager(format!("firewall: {n}"));
        }
    }
    if r.removed_ports.is_empty() && r.notes.iter().any(|n| n.contains("administrator")) {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            r.notes.first().cloned().unwrap_or_else(||
                "administrator privileges required".into()),
        ));
    }
    Ok(Json(r))
}
