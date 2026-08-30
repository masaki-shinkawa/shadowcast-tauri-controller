mod analysis;
mod capture;
mod game_state;

use analysis::AnalysisManager;
use capture::CaptureManager;
use game_state::default_games_root;
use tauri::{path::BaseDirectory, Manager};
use tracing_subscriber::EnvFilter;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    init_tracing();

    tauri::Builder::default()
        .manage(CaptureManager::default())
        .setup(|app| {
            let bundled_root = app
                .path()
                .resolve("config/games", BaseDirectory::Resource)?;
            let games_root = std::env::var_os("SHADOWCAST_GAME_CONFIG_ROOT")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| {
                    if bundled_root.exists() {
                        bundled_root
                    } else {
                        default_games_root()
                    }
                });
            let analysis =
                AnalysisManager::from_games_root(games_root).map_err(std::io::Error::other)?;
            app.manage(analysis);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            capture::start_capture,
            capture::stop_capture,
            capture::get_capture_status,
            capture::set_telemetry_enabled,
            capture::report_preview_metrics,
            analysis::get_analysis_status,
            analysis::configure_analysis,
            analysis::set_analysis_template,
            analysis::load_game_config,
            analysis::save_game_screenshot,
        ])
        .run(tauri::generate_context!())
        .expect("error while running ShadowCast Controller");
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("shadowcast_tauri_controller=info"));

    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .try_init();
}
