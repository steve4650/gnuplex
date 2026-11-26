mod consts;
mod db;
mod error;
mod gnuplex;
mod models;
mod mpv;
mod screenshots;
mod subtitles;
mod sync;
mod timestamps;
mod upgrade;
mod url;
mod web;

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use chrono::{Local, Timelike};
use clap::{Parser, ValueHint};
use tokio::sync::watch;
use tracing::{Level, error, info, warn};

use crate::db::Db;
use crate::error::{Error, Result};
use crate::gnuplex::Gnuplex;
use crate::mpv::Mpv;
use crate::web::{AppState, router};

#[derive(Debug, Parser)]
#[command(name = "gnuplex", disable_version_flag = true)]
struct Args {
    /// Run in prod mode.
    #[arg(long)]
    prod: bool,
    /// Verbose logging.
    #[arg(long)]
    verbose: bool,
    /// Print version info.
    #[arg(long)]
    version: bool,
    /// Path to sqlite DB.
    #[arg(long, default_value = "gnuplex.sqlite3", value_hint = ValueHint::FilePath)]
    db_path: PathBuf,
    /// HTTP server's port.
    #[arg(long, default_value_t = 40000)]
    port: u16,
    /// Upgrade GNUPlex.
    #[arg(long)]
    upgrade: bool,
    /// Directory containing config files and Lua scripts for mpv.
    #[arg(long, default_value = "./mpv_config", value_hint = ValueHint::DirPath)]
    config_dir: PathBuf,
    /// Path to static web files.
    #[arg(long, default_value = "static", value_hint = ValueHint::DirPath)]
    static_files: PathBuf,
}

#[tokio::main]
async fn main() -> ExitCode {
    let args = Args::parse();
    init_logging(args.verbose);
    let exe = std::env::current_exe().unwrap_or_default();

    if args.upgrade {
        return match upgrade::upgrade_gnuplex(&exe, true) {
            Ok(_) => ExitCode::SUCCESS,
            Err(err) => {
                error!("7a7233a9-262a-4bf6-8229-43855d3852d2: could not upgrade GNUPlex: {err}");
                ExitCode::FAILURE
            }
        };
    }
    if args.version {
        return match print_version() {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                error!("678f5d62-8c22-42bc-b25a-c5903b533312: failed to print version info: {err}");
                ExitCode::FAILURE
            }
        };
    }
    if let Err(err) = std::fs::read_dir(&args.config_dir) {
        error!(
            "a895ec8b-8a76-48e9-a20e-3a7d562a10ca: could not read the --config-dir directory {}: {err}",
            args.config_dir.display()
        );
        return ExitCode::FAILURE;
    }
    info!("GNUPlex Version {}", consts::VERSION);

    match run(args, exe).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            error!("{err}");
            ExitCode::FAILURE
        }
    }
}

/// INFO logging by default; `--verbose` (passed by the dev scripts) enables
/// DEBUG logging.
fn init_logging(verbose: bool) {
    let level = if verbose { Level::DEBUG } else { Level::INFO };
    tracing_subscriber::fmt()
        .with_max_level(level)
        .with_target(false)
        .init();
}

async fn run(args: Args, exe: PathBuf) -> Result<()> {
    let verbose = !args.prod || args.verbose;
    let screenshots_dir = screenshots::screenshots_dir(&args.static_files);
    std::fs::create_dir_all(&screenshots_dir).map_err(|err| {
        Error::Startup(format!(
            "failed to create screenshots directory {}: {err}",
            screenshots_dir.display()
        ))
    })?;
    let mpv = Mpv::init(verbose, &args.config_dir)?;
    let db = Db::init(&args.db_path)?;
    let gnuplex = Arc::new(Gnuplex::new(
        db,
        Arc::clone(&mpv),
        screenshots_dir,
        args.static_files.clone(),
        exe,
    ));
    let (shutdown_tx, mut shutdown_rx) = watch::channel(false);
    mpv.spawn_supervisor(shutdown_tx.clone());

    let state = Arc::new(AppState {
        gnuplex: Arc::clone(&gnuplex),
        shutdown: shutdown_tx.clone(),
    });
    let app = router(state);
    let listener = tokio::net::TcpListener::bind(("0.0.0.0", args.port))
        .await
        .map_err(|err| {
            Error::Startup(format!(
                "e6c1f9ee-681d-4eb4-b487-d86622e07aac: could not bind listener: {err}"
            ))
        })?;

    tokio::spawn(scheduler(Arc::clone(&gnuplex), shutdown_tx.clone()));

    let shutdown = async move {
        while !*shutdown_rx.borrow() {
            if shutdown_rx.changed().await.is_err() {
                break;
            }
        }
    };
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown)
        .await
        .map_err(|err| {
            Error::Startup(format!(
                "38263abe-be5f-4d13-bddf-f4e16ba06d87: server error: {err}"
            ))
        })
}

async fn scheduler(gnuplex: Arc<Gnuplex>, shutdown: watch::Sender<bool>) {
    let mut last_scan = None;
    let mut last_update = None;
    loop {
        let now = Local::now();
        let date = now.date_naive();
        if now.hour() == 1 && now.minute() == 15 && last_scan != Some(date) {
            last_scan = Some(date);
            info!("running ScanLib job");
            if let Err(err) = gnuplex.scan_lib() {
                error!("0700be03-adde-4f6a-8b8b-40361d1c5655: scanlib job failed: {err}");
            }
        }
        if now.hour() == 2 && now.minute() == 15 && last_update != Some(date) {
            last_update = Some(date);
            info!("Running update job");
            match gnuplex.get_now_playing() {
                Ok(Some(paths)) => {
                    if !paths.is_empty() {
                        warn!(
                            "a3b99261-331c-4484-a9fb-358b66f364f2: not updating; something is playing"
                        );
                        continue;
                    }
                }
                Ok(None) => {}
                Err(err) => {
                    error!(
                        "857034c0-a7db-4aa8-8cdf-00d0b6d811c2: failed to retrieve NowPlaying: {err}"
                    );
                    continue;
                }
            }
            match upgrade::upgrade_gnuplex(gnuplex.exe(), false) {
                Ok(true) => {
                    info!("Updated GNUPlex. Quitting");
                    let _ = shutdown.send(true);
                }
                Ok(false) => {}
                Err(err) => error!("c4cde0b8-1fd5-4ab0-a081-c59b4944e6af: upgrade job failed: {err}"),
            }
        }
        tokio::time::sleep(std::time::Duration::from_secs(20)).await;
    }
}

fn print_version() -> Result<()> {
    let info = models::VersionInfo {
        version: consts::VERSION.to_string(),
        source_hash: consts::SOURCE_HASH.to_string(),
        platform: consts::PLATFORM.to_string(),
        compiler_version: consts::COMPILER_VERSION.to_string(),
    };
    println!("{}", serde_json::to_string(&info)?);
    Ok(())
}
