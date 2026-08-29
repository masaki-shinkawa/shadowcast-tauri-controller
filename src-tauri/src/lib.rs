mod analysis;
mod capture;

use analysis::AnalysisManager;
use capture::CaptureManager;
use tracing_subscriber::EnvFilter;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    init_tracing();

    tauri::Builder::default()
        .manage(CaptureManager::default())
        .manage(AnalysisManager::default())
        .invoke_handler(tauri::generate_handler![
            capture::start_capture,
            capture::stop_capture,
            capture::get_capture_status,
            capture::set_telemetry_enabled,
            capture::report_preview_metrics,
            analysis::get_analysis_status,
            analysis::configure_analysis,
            analysis::set_analysis_template,
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
