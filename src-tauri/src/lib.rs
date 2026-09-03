mod analysis;
mod capture;
mod diagnostics;
mod game_state;
mod manual_controller;
mod scenario;

use analysis::AnalysisManager;
use capture::CaptureManager;
use game_state::default_games_root;
use manual_controller::ManualControllerManager;
use scenario::ScenarioManager;
use tauri::{path::BaseDirectory, Manager};
use tracing_subscriber::EnvFilter;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    init_tracing();

    tauri::Builder::default()
        .manage(CaptureManager::default())
        .manage(ManualControllerManager::default())
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
            let diagnostics_root = std::env::var_os("SHADOWCAST_DIAGNOSTICS_ROOT")
                .map(std::path::PathBuf::from)
                .unwrap_or(app.path().app_local_data_dir()?.join("automation-runs"));
            let analysis = AnalysisManager::from_games_root_with_diagnostics(
                &games_root,
                Some(diagnostics_root.clone()),
            )
            .map_err(std::io::Error::other)?;
            let scenario =
                ScenarioManager::from_games_root_with_diagnostics(&games_root, diagnostics_root);
            app.manage(analysis);
            app.manage(scenario);
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
            scenario::get_scenario_status,
            scenario::start_scenario,
            scenario::resume_scenario,
            scenario::stop_scenario,
            manual_controller::get_manual_controller_status,
            manual_controller::connect_manual_controller,
            manual_controller::disconnect_manual_controller,
            manual_controller::set_manual_controller_button,
            manual_controller::set_manual_controller_stick,
            manual_controller::neutralize_manual_controller,
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
