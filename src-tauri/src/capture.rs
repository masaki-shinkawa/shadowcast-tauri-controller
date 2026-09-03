mod device;
mod worker;

use std::{
    sync::{
        atomic::{AtomicBool, Ordering as AtomicOrdering},
        mpsc, Arc, Mutex, MutexGuard,
    },
    thread::{self, JoinHandle},
};

use serde::{Deserialize, Serialize};
use tauri::{
    ipc::{Channel, Response},
    State,
};
use tracing::{error, info};

use self::worker::run_capture;
use crate::analysis::AnalysisManager;
use crate::scenario::ScenarioManager;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureStatus {
    state: CaptureState,
    device_name: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    target_fps: Option<u32>,
    measured_fps: f64,
    frame_format: Option<String>,
    frame_count: u64,
    jpeg_bytes: u64,
    average_jpeg_bytes: f64,
    channel_mbps: f64,
    average_channel_send_ms: f64,
    telemetry_enabled: bool,
    average_analysis_submit_ms: f64,
    error: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "lowercase")]
enum CaptureState {
    Starting,
    Running,
    Stopped,
    Error,
}

impl Default for CaptureStatus {
    fn default() -> Self {
        Self {
            state: CaptureState::Stopped,
            device_name: None,
            width: None,
            height: None,
            target_fps: None,
            measured_fps: 0.0,
            frame_format: None,
            frame_count: 0,
            jpeg_bytes: 0,
            average_jpeg_bytes: 0.0,
            channel_mbps: 0.0,
            average_channel_send_ms: 0.0,
            telemetry_enabled: false,
            average_analysis_submit_ms: 0.0,
            error: None,
        }
    }
}

struct ActiveCapture {
    stop: Arc<AtomicBool>,
    thread: JoinHandle<()>,
}

pub struct CaptureManager {
    active: Mutex<Option<ActiveCapture>>,
    status: Arc<Mutex<CaptureStatus>>,
    telemetry_enabled: Arc<AtomicBool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewMetrics {
    received_fps: f64,
    rendered_fps: f64,
    receive_mbps: f64,
    receive_to_draw_ms: f64,
    dropped_frames: u64,
}

impl Default for CaptureManager {
    fn default() -> Self {
        Self {
            active: Mutex::new(None),
            status: Arc::new(Mutex::new(CaptureStatus::default())),
            telemetry_enabled: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl CaptureManager {
    fn start(
        &self,
        on_frame: Channel<Response>,
        analysis: &AnalysisManager,
    ) -> Result<CaptureStatus, String> {
        let mut active = lock(&self.active);

        if active
            .as_ref()
            .is_some_and(|capture| !capture.thread.is_finished())
        {
            return Err("Capture is already running".to_owned());
        }
        if let Some(finished) = active.take() {
            let _ = finished.thread.join();
        }

        let telemetry_enabled = self.telemetry_enabled.load(AtomicOrdering::Acquire);
        update_status(&self.status, |status| {
            *status = CaptureStatus {
                state: CaptureState::Starting,
                telemetry_enabled,
                ..CaptureStatus::default()
            };
        });

        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let thread_status = Arc::clone(&self.status);
        let thread_telemetry_enabled = Arc::clone(&self.telemetry_enabled);
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let analysis_input = analysis.start()?;

        let capture_thread = thread::Builder::new()
            .name("shadowcast-capture".to_owned())
            .spawn(move || {
                run_capture(
                    on_frame,
                    thread_stop,
                    thread_status,
                    thread_telemetry_enabled,
                    ready_tx,
                    analysis_input,
                );
            })
            .map_err(|error| {
                analysis.stop();
                format!("Failed to spawn capture thread: {error}")
            })?;

        match ready_rx.recv() {
            Ok(Ok(status)) => {
                *active = Some(ActiveCapture {
                    stop,
                    thread: capture_thread,
                });
                Ok(status)
            }
            Ok(Err(message)) => {
                let _ = capture_thread.join();
                analysis.stop();
                Err(message)
            }
            Err(error) => {
                let _ = capture_thread.join();
                analysis.stop();
                Err(format!("Capture thread stopped during startup: {error}"))
            }
        }
    }

    fn stop(&self, analysis: &AnalysisManager) -> CaptureStatus {
        // Keep start and stop linearized so an older stop cannot tear down the
        // analysis worker belonging to a concurrently started capture.
        let mut active = lock(&self.active);
        let active_capture = active.take();
        if let Some(active_capture) = active_capture {
            info!("stopping ShadowCast capture");
            active_capture.stop.store(true, AtomicOrdering::Release);
            if active_capture.thread.join().is_err() {
                error!("capture thread panicked while stopping");
                update_status(&self.status, |status| {
                    status.state = CaptureState::Error;
                    status.error = Some("Capture thread panicked while stopping".to_owned());
                });
            }
        }
        analysis.stop();

        update_status(&self.status, |status| {
            if !matches!(status.state, CaptureState::Error) {
                status.state = CaptureState::Stopped;
                status.measured_fps = 0.0;
            }
        });
        lock(&self.status).clone()
    }

    fn status(&self) -> CaptureStatus {
        lock(&self.status).clone()
    }

    fn set_telemetry_enabled(&self, enabled: bool) -> CaptureStatus {
        self.telemetry_enabled
            .store(enabled, AtomicOrdering::Release);
        update_status(&self.status, |status| {
            status.telemetry_enabled = enabled;
            reset_telemetry_metrics(status);
        });
        lock(&self.status).clone()
    }

    fn telemetry_enabled(&self) -> bool {
        self.telemetry_enabled.load(AtomicOrdering::Acquire)
    }
}

impl Drop for CaptureManager {
    fn drop(&mut self) {
        if let Ok(active) = self.active.get_mut() {
            if let Some(active) = active.take() {
                active.stop.store(true, AtomicOrdering::Release);
                let _ = active.thread.join();
            }
        }
    }
}

#[tauri::command(async)]
pub fn start_capture(
    manager: State<'_, CaptureManager>,
    analysis: State<'_, AnalysisManager>,
    on_frame: Channel<Response>,
) -> Result<CaptureStatus, String> {
    manager.start(on_frame, &analysis)
}

#[tauri::command(async)]
pub fn stop_capture(
    manager: State<'_, CaptureManager>,
    analysis: State<'_, AnalysisManager>,
    scenario: State<'_, ScenarioManager>,
) -> Result<CaptureStatus, String> {
    scenario.stop();
    Ok(manager.stop(&analysis))
}

#[tauri::command]
pub fn get_capture_status(manager: State<'_, CaptureManager>) -> CaptureStatus {
    manager.status()
}

#[tauri::command]
pub fn set_telemetry_enabled(manager: State<'_, CaptureManager>, enabled: bool) -> CaptureStatus {
    manager.set_telemetry_enabled(enabled)
}

#[tauri::command]
pub fn report_preview_metrics(manager: State<'_, CaptureManager>, metrics: PreviewMetrics) {
    if !manager.telemetry_enabled() {
        return;
    }
    info!(
        received_fps = metrics.received_fps,
        rendered_fps = metrics.rendered_fps,
        receive_mbps = metrics.receive_mbps,
        receive_to_draw_ms = metrics.receive_to_draw_ms,
        dropped_frames = metrics.dropped_frames,
        "preview performance"
    );
}

fn reset_telemetry_metrics(status: &mut CaptureStatus) {
    status.measured_fps = 0.0;
    status.jpeg_bytes = 0;
    status.average_jpeg_bytes = 0.0;
    status.channel_mbps = 0.0;
    status.average_channel_send_ms = 0.0;
    status.average_analysis_submit_ms = 0.0;
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn update_status(status: &Arc<Mutex<CaptureStatus>>, update: impl FnOnce(&mut CaptureStatus)) {
    update(&mut lock(status));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn telemetry_is_disabled_by_default_and_resets_detailed_metrics() {
        let manager = CaptureManager::default();
        assert!(!manager.status().telemetry_enabled);

        update_status(&manager.status, |status| {
            status.measured_fps = 60.0;
            status.jpeg_bytes = 1024;
            status.average_jpeg_bytes = 512.0;
            status.channel_mbps = 8.0;
            status.average_channel_send_ms = 0.1;
            status.average_analysis_submit_ms = 0.2;
        });

        let status = manager.set_telemetry_enabled(true);
        assert!(status.telemetry_enabled);
        assert_eq!(status.measured_fps, 0.0);
        assert_eq!(status.jpeg_bytes, 0);
        assert_eq!(status.average_jpeg_bytes, 0.0);
        assert_eq!(status.channel_mbps, 0.0);
        assert_eq!(status.average_channel_send_ms, 0.0);
        assert_eq!(status.average_analysis_submit_ms, 0.0);
    }
}
