use std::cmp::Ordering;

use nokhwa::{
    query,
    utils::{
        ApiBackend, CameraFormat, CameraInfo, FrameFormat, RequestedFormat, RequestedFormatType,
    },
    Camera,
};
use tracing::info;

pub(super) const TARGET_FPS: u32 = 60;
const TARGET_WIDTH: u32 = 1280;
const TARGET_HEIGHT: u32 = 720;
const ACCEPTED_FORMATS: &[FrameFormat] = &[
    FrameFormat::MJPEG,
    FrameFormat::YUYV,
    FrameFormat::NV12,
    FrameFormat::RAWRGB,
    FrameFormat::RAWBGR,
    FrameFormat::GRAY,
];

pub(super) struct OpenedCamera {
    pub camera: Camera,
    pub device_name: String,
    pub requested_format: CameraFormat,
    pub stream_format: CameraFormat,
}

pub(super) fn open_shadowcast() -> Result<OpenedCamera, String> {
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

    let requested_format = select_preferred_format(&formats)
        .ok_or_else(|| "No usable ShadowCast capture format was found".to_owned())?;
    let exact = RequestedFormat::with_formats(
        RequestedFormatType::Exact(requested_format),
        ACCEPTED_FORMATS,
    );
    camera
        .set_camera_requset(exact)
        .map_err(|error| format!("Failed to select {requested_format}: {error}"))?;
    camera
        .open_stream()
        .map_err(|error| format!("Failed to start the ShadowCast stream: {error}"))?;

    let stream_format = camera.camera_format();
    let device_name = shadowcast.human_name();
    info!(
        device = %device_name,
        width = stream_format.width(),
        height = stream_format.height(),
        requested_fps = requested_format.frame_rate(),
        reported_stream_fps = stream_format.frame_rate(),
        frame_format = ?stream_format.format(),
        passthrough = stream_format.format() == FrameFormat::MJPEG,
        "ShadowCast capture started"
    );

    Ok(OpenedCamera {
        camera,
        device_name,
        requested_format,
        stream_format,
    })
}

pub(super) fn format_name(format: FrameFormat) -> &'static str {
    match format {
        FrameFormat::MJPEG => "MJPEG",
        FrameFormat::YUYV => "YUYV",
        FrameFormat::NV12 => "NV12",
        FrameFormat::GRAY => "GRAY",
        FrameFormat::RAWRGB => "RGB",
        FrameFormat::RAWBGR => "BGR",
    }
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
