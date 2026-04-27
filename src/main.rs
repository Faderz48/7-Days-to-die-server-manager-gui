//! Entry point. Boots an Axum HTTP server that serves the embedded web UI
//! and exposes a small JSON API for the front-end to drive the dedicated
//! server.

mod admin;
mod api;
mod backup;
mod config;
mod dialog_focus;
mod firewall;
mod mods;
mod netinfo;
mod paths;
mod scheduler;
mod seed;
mod server;
mod settings;
mod state;
mod static_files;
mod telnet;
mod upnp;
mod worlds;

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::{Context, Result};
use axum::Router;
use tokio::sync::RwLock;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

use crate::settings::AppSettings;
use crate::state::AppState;

#[tokio::main]
async fn main() -> Result<()> {
    // ── Logging ──────────────────────────────────────────────────────────
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info,sdtd_server_manager=debug")),
        )
        .compact()
        .init();

    // ── Load persisted app settings (paths to game files etc.) ───────────
    let settings = AppSettings::load_or_default()?;
    let state = Arc::new(RwLock::new(AppState::new(settings)));

    // ── Background scheduler (daily restart / backup tasks) ──────────────
    scheduler::spawn(state.clone());

    // ── Build router ─────────────────────────────────────────────────────
    let app = Router::new()
        .merge(api::routes())
        .merge(static_files::routes())
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    // ── Bind ─────────────────────────────────────────────────────────────
    // Address comes from $BIND (preferred — full "ip:port" form) or just
    // $PORT (keeps the listener on localhost). Default: 127.0.0.1:8421.
    //
    // Setting BIND=0.0.0.0:8421 exposes the manager to your LAN. We do
    // NOT default to that because there's no auth on these endpoints.
    let addr: SocketAddr = match std::env::var("BIND").ok() {
        Some(s) => s.parse()
            .with_context(|| format!("BIND='{s}' is not a valid 'ip:port' address"))?,
        None => {
            let port = std::env::var("PORT")
                .ok()
                .and_then(|s| s.parse::<u16>().ok())
                .unwrap_or(8421u16);
            SocketAddr::from(([127, 0, 0, 1], port))
        }
    };
    let port = addr.port();

    tracing::info!("listening on http://{}", addr);
    print_banner(port);

    let listener = tokio::net::TcpListener::bind(&addr).await?;

    // ── Auto-open browser ────────────────────────────────────────────────
    // Skip when NO_BROWSER is set (handy for headless / CI / Linux-server use).
    if std::env::var("NO_BROWSER").is_err() {
        let url = format!("http://127.0.0.1:{}", port);
        tokio::spawn(async move {
            // small delay so the listener is fully accepting before the
            // browser tab races to load it.
            tokio::time::sleep(std::time::Duration::from_millis(400)).await;
            match webbrowser::open(&url) {
                Ok(_) => tracing::debug!("opened {} in default browser", url),
                Err(e) => tracing::warn!("could not auto-open browser ({e}); open {} manually", url),
            }
        });
    }

    axum::serve(listener, app.into_make_service()).await?;

    Ok(())
}

/// Print a startup banner: chunky ASCII skull + the connection info box.
/// Uses ANSI amber (33) for the skull and ANSI dim (2) for the framing.
/// On terminals that don't speak ANSI the codes show as plain text — the
/// art still reads fine, just monochrome.
fn print_banner(port: u16) {
    // Try to enable ANSI escape processing on Windows. Harmless on other
    // platforms (the function is a no-op there).
    enable_vt_on_windows();

    let amber = "\x1b[38;5;208m"; // 256-color amber
    let dim   = "\x1b[2m";
    let bold  = "\x1b[1m";
    let reset = "\x1b[0m";
    let green = "\x1b[32m";

    println!();
    println!("{amber}              ▄▄▄▄▄▄▄▄▄▄▄▄▄▄{reset}");
    println!("{amber}           ▄██▀░░░░░░░░░░░░▀██▄{reset}");
    println!("{amber}         ▄█▀░░░░██░░░░░░██░░░░▀█▄{reset}");
    println!("{amber}        █▀░░░░░░██░░░░░░██░░░░░░▀█{reset}");
    println!("{amber}        █░░░░░░░░░░░░░░░░░░░░░░░░█{reset}");
    println!("{amber}        █░░░░░░░▀▀▄▄▄▄▄▄▄▀▀░░░░░░█{reset}");
    println!("{amber}        █▄░░░░░░██░██░██░██░░░░░▄█{reset}");
    println!("{amber}         ▀█▄░░░░▀▀░▀▀░▀▀░▀▀░░░▄█▀{reset}");
    println!("{amber}           ▀██▄▄░░░░░░░░░░░▄▄██▀{reset}");
    println!("{amber}              ▀▀████████████▀▀{reset}      {green}● online{reset}");
    println!();
    println!("  {dim}╔══════════════════════════════════════════════════╗{reset}");
    println!("  {dim}║{reset}   {bold}7 DAYS TO DIE  ::  SERVER MANAGER{reset}              {dim}║{reset}");
    println!("  {dim}║{reset}                                                  {dim}║{reset}");
    println!("  {dim}║{reset}   Open: {amber}http://localhost:{:<5}{reset}                   {dim}║{reset}", port);
    println!("  {dim}║{reset}   Stop: Ctrl+C                                   {dim}║{reset}");
    println!("  {dim}╚══════════════════════════════════════════════════╝{reset}");
    println!();
}

/// Turn on Windows console virtual-terminal processing so ANSI escape
/// sequences (colors, etc.) render as colors instead of literal bytes.
/// No-op on Windows 10 < 1607 — those will just see the escape codes.
#[cfg(windows)]
fn enable_vt_on_windows() {
    use std::ffi::c_void;
    type HANDLE = *mut c_void;
    const STD_OUTPUT_HANDLE: u32 = 0xFFFF_FFF5; // -11 as u32
    const ENABLE_VIRTUAL_TERMINAL_PROCESSING: u32 = 0x0004;
    const INVALID_HANDLE_VALUE: HANDLE = -1isize as HANDLE;

    #[link(name = "kernel32")]
    extern "system" {
        fn GetStdHandle(handle: u32) -> HANDLE;
        fn GetConsoleMode(handle: HANDLE, mode: *mut u32) -> i32;
        fn SetConsoleMode(handle: HANDLE, mode: u32) -> i32;
    }
    unsafe {
        let h = GetStdHandle(STD_OUTPUT_HANDLE);
        if h.is_null() || h == INVALID_HANDLE_VALUE { return; }
        let mut mode: u32 = 0;
        if GetConsoleMode(h, &mut mode) == 0 { return; }
        let _ = SetConsoleMode(h, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING);
    }
}

#[cfg(not(windows))]
fn enable_vt_on_windows() {}
