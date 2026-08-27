use std::{
    cmp::Ordering,
    io::Cursor,
    sync::{
        atomic::{AtomicBool, Ordering as AtomicOrdering},
        mpsc, Arc, Mutex, MutexGuard,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use image::codecs::jpeg::JpegEncoder;
use nokhwa::{
    pixel_format::RgbFormat,
    query,
    utils::{
        ApiBackend, CameraFormat, CameraInfo, FrameFormat, RequestedFormat, RequestedFormatType,
    },
    Camera,
};
use serde::Serialize;
use tauri::{
    ipc::{Channel, Response},
    State,
};
use tracing::{error, info, warn};

const TARGET_WIDTH: u32 = 1280;
const TARGET_HEIGHT: u32 = 720;
const TARGET_FPS: u32 = 60;
const ACCEPTED_FORMATS: &[FrameFormat] = &[
    FrameFormat::MJPEG,
    FrameFormat::YUYV,
    FrameFormat::NV12,
    FrameFormat::RAWRGB,
    FrameFormat::RAWBGR,
    FrameFormat::GRAY,
];

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
}

impl Default for CaptureManager {
    fn default() -> Self {
        Self {
            active: Mutex::new(None),
            status: Arc::new(Mutex::new(CaptureStatus::default())),
        }
    }
}

impl CaptureManager {
    fn start(&self, on_frame: Channel<Response>) -> Result<CaptureStatus, String> {
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

        update_status(&self.status, |status| {
            *status = CaptureStatus {
                state: CaptureState::Starting,
                ..CaptureStatus::default()
            };
        });

        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let thread_status = Arc::clone(&self.status);
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);

        let capture_thread = thread::Builder::new()
            .name("shadowcast-capture".to_owned())
            .spawn(move || run_capture(on_frame, thread_stop, thread_status, ready_tx))
            .map_err(|error| format!("Failed to spawn capture thread: {error}"))?;

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
                Err(message)
            }
            Err(error) => {
                let _ = capture_thread.join();
                Err(format!("Capture thread stopped during startup: {error}"))
            }
        }
    }

    fn stop(&self) -> CaptureStatus {
        let active_capture = lock(&self.active).take();
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
    on_frame: Channel<Response>,
) -> Result<CaptureStatus, String> {
    manager.start(on_frame)
}

#[tauri::command(async)]
pub fn stop_capture(manager: State<'_, CaptureManager>) -> Result<CaptureStatus, String> {
    Ok(manager.stop())
}

#[tauri::command]
pub fn get_capture_status(manager: State<'_, CaptureManager>) -> CaptureStatus {
    manager.status()
}

fn run_capture(
    on_frame: Channel<Response>,
    stop: Arc<AtomicBool>,
    status: Arc<Mutex<CaptureStatus>>,
    ready: mpsc::SyncSender<Result<CaptureStatus, String>>,
) {
    let mut ready = Some(ready);
    let result = capture_loop(&on_frame, &stop, &status, &mut ready);

    if let Err(message) = result {
        error!(error = %message, "ShadowCast capture failed");
        update_status(&status, |capture_status| {
            capture_status.state = CaptureState::Error;
            capture_status.error = Some(message.clone());
            capture_status.measured_fps = 0.0;
        });
        if let Some(ready) = ready.take() {
            let _ = ready.send(Err(message));
        }
    } else {
        info!("ShadowCast capture stopped");
        update_status(&status, |capture_status| {
            capture_status.state = CaptureState::Stopped;
            capture_status.measured_fps = 0.0;
        });
    }
}

fn capture_loop(
    on_frame: &Channel<Response>,
    stop: &AtomicBool,
    status: &Arc<Mutex<CaptureStatus>>,
    ready: &mut Option<mpsc::SyncSender<Result<CaptureStatus, String>>>,
) -> Result<(), String> {
    let devices = query(ApiBackend::MediaFoundation)
        .map_err(|error| format!("Failed to enumerate Windows camera devices: {error}"))?;

    for device in &devices {
        info!(
            name = %device.human_name(),
            description = %device.description(),
            index = %device.index().as_string(),
            "camera device found"
        );
    }

    let shadowcast = devices
        .iter()
        .find(|device| is_shadowcast(device))
        .cloned()
        .ok_or_else(|| {
            let available = devices
                .iter()
                .map(CameraInfo::human_name)
                .collect::<Vec<_>>()
                .join(", ");
            if available.is_empty() {
                "ShadowCast was not found. No Windows camera devices are available.".to_owned()
            } else {
                format!("ShadowCast was not found. Available cameras: {available}")
            }
        })?;

    info!(device = %shadowcast.human_name(), "ShadowCast detected");

    let target =
        CameraFormat::new_from(TARGET_WIDTH, TARGET_HEIGHT, FrameFormat::MJPEG, TARGET_FPS);
    let requested =
        RequestedFormat::with_formats(RequestedFormatType::Closest(target), ACCEPTED_FORMATS);
    let mut camera = Camera::with_backend(
        shadowcast.index().clone(),
        requested,
        ApiBackend::MediaFoundation,
    )
    .map_err(|error| format!("Failed to open ShadowCast through Media Foundation: {error}"))?;

    let formats = camera
        .compatible_camera_formats()
        .map_err(|error| format!("Failed to enumerate ShadowCast formats: {error}"))?;
    if formats.is_empty() {
        return Err("ShadowCast reported no compatible capture formats".to_owned());
    }

    for format in &formats {
        info!(
            width = format.width(),
            height = format.height(),
            fps = format.frame_rate(),
            frame_format = ?format.format(),
            "ShadowCast format supported"
        );
    }

    let selected = select_preferred_format(&formats)
        .ok_or_else(|| "No usable ShadowCast capture format was found".to_owned())?;
    let exact =
        RequestedFormat::with_formats(RequestedFormatType::Exact(selected), ACCEPTED_FORMATS);
    camera
        .set_camera_requset(exact)
        .map_err(|error| format!("Failed to select {selected}: {error}"))?;
    camera
        .open_stream()
        .map_err(|error| format!("Failed to start the ShadowCast stream: {error}"))?;

    let selected = camera.camera_format();
    let device_name = shadowcast.human_name();
    info!(
        device = %device_name,
        width = selected.width(),
        height = selected.height(),
        fps = selected.frame_rate(),
        frame_format = ?selected.format(),
        passthrough = selected.format() == FrameFormat::MJPEG,
        "ShadowCast capture started"
    );

    update_status(status, |capture_status| {
        *capture_status = CaptureStatus {
            state: CaptureState::Running,
            device_name: Some(device_name),
            width: Some(selected.width()),
            height: Some(selected.height()),
            target_fps: Some(selected.frame_rate()),
            measured_fps: 0.0,
            frame_format: Some(format_name(selected.format()).to_owned()),
            frame_count: 0,
            error: None,
        };
    });

    if let Some(ready) = ready.take() {
        let _ = ready.send(Ok(lock(status).clone()));
    }

    let mut total_frames = 0_u64;
    let mut interval_frames = 0_u64;
    let mut interval_started = Instant::now();

    while !stop.load(AtomicOrdering::Acquire) {
        let frame = camera
            .frame()
            .map_err(|error| format!("Failed to read a ShadowCast frame: {error}"))?;
        let jpeg = if frame.source_frame_format() == FrameFormat::MJPEG {
            frame.buffer().to_vec()
        } else {
            warn!(
                frame_format = ?frame.source_frame_format(),
                "non-MJPEG frame requires RGB decode and JPEG encode"
            );
            let rgb = frame.decode_image::<RgbFormat>().map_err(|error| {
                format!(
                    "Failed to decode {:?} frame: {error}",
                    frame.source_frame_format()
                )
            })?;
            let mut jpeg = Cursor::new(Vec::new());
            JpegEncoder::new_with_quality(&mut jpeg, 88)
                .encode_image(&rgb)
                .map_err(|error| format!("Failed to encode fallback JPEG frame: {error}"))?;
            jpeg.into_inner()
        };

        if on_frame.send(Response::new(jpeg)).is_err() {
            info!("frontend frame channel closed");
            break;
        }

        total_frames += 1;
        interval_frames += 1;
        let elapsed = interval_started.elapsed();
        if elapsed >= Duration::from_secs(1) {
            let measured_fps = interval_frames as f64 / elapsed.as_secs_f64();
            update_status(status, |capture_status| {
                capture_status.frame_count = total_frames;
                capture_status.measured_fps = measured_fps;
            });
            interval_started = Instant::now();
            interval_frames = 0;
        }
    }

    if let Err(error) = camera.stop_stream() {
        warn!(%error, "failed to stop ShadowCast stream cleanly");
    }
    update_status(status, |capture_status| {
        capture_status.frame_count = total_frames;
    });
    Ok(())
}

fn is_shadowcast(device: &CameraInfo) -> bool {
    let identity = format!(
        "{} {} {}",
        device.human_name(),
        device.description(),
        device.misc()
    )
    .to_lowercase();
    identity.contains("shadowcast") || identity.contains("genki")
}

fn select_preferred_format(formats: &[CameraFormat]) -> Option<CameraFormat> {
    formats.iter().copied().min_by(compare_formats)
}

fn compare_formats(left: &CameraFormat, right: &CameraFormat) -> Ordering {
    format_score(left).cmp(&format_score(right))
}

fn format_score(format: &CameraFormat) -> (u8, u8, u32, u64, std::cmp::Reverse<u32>) {
    let is_mjpeg = format.format() == FrameFormat::MJPEG;
    let is_target_resolution = format.width() == TARGET_WIDTH && format.height() == TARGET_HEIGHT;
    let format_penalty = if is_mjpeg { 0 } else { 1 };
    let resolution_penalty = if is_target_resolution { 0 } else { 1 };
    let fps_distance = format.frame_rate().abs_diff(TARGET_FPS);
    let pixel_distance = (u64::from(format.width()) * u64::from(format.height()))
        .abs_diff(u64::from(TARGET_WIDTH) * u64::from(TARGET_HEIGHT));

    (
        format_penalty,
        resolution_penalty,
        fps_distance,
        pixel_distance,
        std::cmp::Reverse(format.frame_rate()),
    )
}

fn format_name(format: FrameFormat) -> &'static str {
    match format {
        FrameFormat::MJPEG => "MJPEG",
        FrameFormat::YUYV => "YUYV",
        FrameFormat::NV12 => "NV12",
        FrameFormat::GRAY => "GRAY",
        FrameFormat::RAWRGB => "RGB",
        FrameFormat::RAWBGR => "BGR",
    }
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
    fn exact_target_mjpeg_wins() {
        let formats = [
            CameraFormat::new_from(1920, 1080, FrameFormat::MJPEG, 60),
            CameraFormat::new_from(1280, 720, FrameFormat::YUYV, 60),
            CameraFormat::new_from(1280, 720, FrameFormat::MJPEG, 60),
        ];

        assert_eq!(select_preferred_format(&formats), Some(formats[2]));
    }

    #[test]
    fn target_resolution_mjpeg_beats_other_mjpeg_resolutions() {
        let formats = [
            CameraFormat::new_from(1920, 1080, FrameFormat::MJPEG, 60),
            CameraFormat::new_from(1280, 720, FrameFormat::MJPEG, 30),
        ];

        assert_eq!(select_preferred_format(&formats), Some(formats[1]));
    }
}
