use std::{
    io::Cursor,
    sync::{
        atomic::{AtomicBool, Ordering as AtomicOrdering},
        mpsc, Arc, Mutex,
    },
    time::{Duration, Instant},
};

use image::codecs::jpeg::JpegEncoder;
use nokhwa::{
    pixel_format::RgbFormat,
    utils::{CameraFormat, FrameFormat},
};
use tauri::ipc::{Channel, Response};
use tracing::{error, info, warn};

use super::{
    device::{format_name, open_shadowcast, OpenedCamera},
    lock, update_status, CaptureState, CaptureStatus,
};

pub(super) fn run_capture(
    on_frame: Channel<Response>,
    stop: Arc<AtomicBool>,
    status: Arc<Mutex<CaptureStatus>>,
    telemetry_enabled: Arc<AtomicBool>,
    ready: mpsc::SyncSender<Result<CaptureStatus, String>>,
) {
    let mut ready = Some(ready);
    let result = capture_loop(&on_frame, &stop, &status, &telemetry_enabled, &mut ready);

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
    telemetry_enabled: &AtomicBool,
    ready: &mut Option<mpsc::SyncSender<Result<CaptureStatus, String>>>,
) -> Result<(), String> {
    let OpenedCamera {
        mut camera,
        device_name,
        requested_format,
        stream_format,
    } = open_shadowcast()?;

    update_status(status, |capture_status| {
        *capture_status = running_status(
            device_name,
            requested_format,
            stream_format,
            telemetry_enabled.load(AtomicOrdering::Acquire),
        );
    });

    if let Some(ready) = ready.take() {
        let _ = ready.send(Ok(lock(status).clone()));
    }

    let mut total_frames = 0_u64;
    let mut total_telemetry_frames = 0_u64;
    let mut total_jpeg_bytes = 0_u64;
    let mut interval_frames = 0_u64;
    let mut interval_jpeg_bytes = 0_u64;
    let mut interval_channel_send = Duration::ZERO;
    let mut interval_started = Instant::now();
    let mut telemetry_was_enabled = false;

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

        let telemetry_active = telemetry_enabled.load(AtomicOrdering::Acquire);
        if telemetry_active && !telemetry_was_enabled {
            total_telemetry_frames = 0;
            total_jpeg_bytes = 0;
            interval_frames = 0;
            interval_jpeg_bytes = 0;
            interval_channel_send = Duration::ZERO;
            interval_started = Instant::now();
        }

        let jpeg_bytes = jpeg.len() as u64;
        let channel_send_started = telemetry_active.then(Instant::now);
        if on_frame.send(Response::new(jpeg)).is_err() {
            info!("frontend frame channel closed");
            break;
        }
        let channel_send_elapsed = channel_send_started.map(|started| started.elapsed());

        total_frames += 1;
        if telemetry_active {
            total_telemetry_frames += 1;
            total_jpeg_bytes += jpeg_bytes;
            interval_frames += 1;
            interval_jpeg_bytes += jpeg_bytes;
            interval_channel_send += channel_send_elapsed.unwrap_or_default();
            let elapsed = interval_started.elapsed();
            if elapsed >= Duration::from_secs(1) {
                let measured_fps = interval_frames as f64 / elapsed.as_secs_f64();
                let channel_mbps =
                    interval_jpeg_bytes as f64 * 8.0 / elapsed.as_secs_f64() / 1_000_000.0;
                let average_jpeg_bytes = total_jpeg_bytes as f64 / total_telemetry_frames as f64;
                let average_channel_send_ms =
                    interval_channel_send.as_secs_f64() * 1_000.0 / interval_frames as f64;
                update_status(status, |capture_status| {
                    if capture_status.telemetry_enabled {
                        capture_status.frame_count = total_frames;
                        capture_status.measured_fps = measured_fps;
                        capture_status.jpeg_bytes = total_jpeg_bytes;
                        capture_status.average_jpeg_bytes = average_jpeg_bytes;
                        capture_status.channel_mbps = channel_mbps;
                        capture_status.average_channel_send_ms = average_channel_send_ms;
                    }
                });
                info!(
                    capture_fps = measured_fps,
                    average_jpeg_kib = average_jpeg_bytes / 1024.0,
                    channel_mbps,
                    average_channel_send_ms,
                    total_frames,
                    "capture performance"
                );
                interval_started = Instant::now();
                interval_frames = 0;
                interval_jpeg_bytes = 0;
                interval_channel_send = Duration::ZERO;
            }
        }
        telemetry_was_enabled = telemetry_active;
    }

    if let Err(error) = camera.stop_stream() {
        warn!(%error, "failed to stop ShadowCast stream cleanly");
    }
    update_status(status, |capture_status| {
        capture_status.frame_count = total_frames;
        if telemetry_enabled.load(AtomicOrdering::Acquire) {
            capture_status.jpeg_bytes = total_jpeg_bytes;
            capture_status.average_jpeg_bytes = if total_telemetry_frames == 0 {
                0.0
            } else {
                total_jpeg_bytes as f64 / total_telemetry_frames as f64
            };
        }
    });
    Ok(())
}

fn running_status(
    device_name: String,
    requested_format: CameraFormat,
    stream_format: CameraFormat,
    telemetry_enabled: bool,
) -> CaptureStatus {
    CaptureStatus {
        state: CaptureState::Running,
        device_name: Some(device_name),
        width: Some(stream_format.width()),
        height: Some(stream_format.height()),
        // nokhwa-bindings-windows 0.4.6 reads the denominator of the packed
        // MF_MT_FRAME_RATE ratio when refreshing the active stream format. For
        // a 60/1 stream that makes stream_format.frame_rate() return 1. The
        // target is the format selected from the enumerated native formats;
        // actual throughput is tracked independently in measured_fps.
        target_fps: Some(requested_format.frame_rate()),
        measured_fps: 0.0,
        frame_format: Some(format_name(stream_format.format()).to_owned()),
        frame_count: 0,
        jpeg_bytes: 0,
        average_jpeg_bytes: 0.0,
        channel_mbps: 0.0,
        average_channel_send_ms: 0.0,
        telemetry_enabled,
        error: None,
    }
}

#[cfg(test)]
mod tests {
    use nokhwa::utils::{CameraFormat, FrameFormat};

    use super::*;
    use crate::capture::device::TARGET_FPS;

    #[test]
    fn target_fps_uses_requested_format_when_stream_reports_ratio_denominator() {
        let requested_format = CameraFormat::new_from(1280, 720, FrameFormat::MJPEG, TARGET_FPS);
        // Media Foundation stores 60 FPS as 60/1. nokhwa-bindings-windows
        // currently reports the denominator after refreshing the stream.
        let stream_format = CameraFormat::new_from(1280, 720, FrameFormat::MJPEG, 1);

        let status = running_status(
            "ShadowCast".to_owned(),
            requested_format,
            stream_format,
            false,
        );

        assert_eq!(status.target_fps, Some(60));
        assert_eq!(status.measured_fps, 0.0);
        assert_eq!(status.width, Some(1280));
        assert_eq!(status.height, Some(720));
        assert_eq!(status.frame_format.as_deref(), Some("MJPEG"));
        assert!(!status.telemetry_enabled);
    }
}
