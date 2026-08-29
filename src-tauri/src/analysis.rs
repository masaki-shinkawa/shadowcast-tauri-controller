use std::{
    sync::{Arc, Condvar, Mutex, MutexGuard},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use image::RgbImage;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

const DEFAULT_MAX_FPS: u32 = 15;
const MAX_ANALYSIS_FPS: u32 = 60;
const MAX_TEMPLATE_DIMENSION: u32 = 64;
const TEMPLATE_COMPARISON_BUDGET: u64 = 4_000_000;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Roi {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RgbColor {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisConfig {
    pub enabled: bool,
    pub roi: Roi,
    pub target_color: RgbColor,
    pub color_tolerance: u8,
    pub max_fps: u32,
}

impl Default for AnalysisConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            roi: Roi {
                x: 480,
                y: 270,
                width: 320,
                height: 180,
            },
            target_color: RgbColor {
                red: 0,
                green: 255,
                blue: 0,
            },
            color_tolerance: 48,
            max_fps: DEFAULT_MAX_FPS,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisTemplateInput {
    width: u32,
    height: u32,
    grayscale: Vec<u8>,
}

#[derive(Clone, Debug)]
struct AnalysisTemplate {
    width: u32,
    height: u32,
    grayscale: Vec<u8>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ColorAnalysis {
    target: RgbColor,
    tolerance: u8,
    average: RgbColor,
    matching_pixels: u64,
    total_pixels: u64,
    match_ratio: f64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateMatch {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    score: f64,
    search_step: u32,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisResult {
    frame_number: u64,
    source_width: u32,
    source_height: u32,
    roi: Roi,
    color: ColorAnalysis,
    template_match: Option<TemplateMatch>,
    queue_delay_ms: f64,
    analysis_ms: f64,
    jpeg_decode_ms: f64,
    color_analysis_ms: f64,
    template_match_ms: f64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "lowercase")]
enum AnalysisState {
    Running,
    Stopped,
    Error,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisStatus {
    state: AnalysisState,
    config: AnalysisConfig,
    submitted_frames: u64,
    analyzed_frames: u64,
    dropped_frames: u64,
    failed_frames: u64,
    measured_fps: f64,
    average_analysis_ms: f64,
    last_result: Option<AnalysisResult>,
    error: Option<String>,
}

impl Default for AnalysisStatus {
    fn default() -> Self {
        Self {
            state: AnalysisState::Stopped,
            config: AnalysisConfig::default(),
            submitted_frames: 0,
            analyzed_frames: 0,
            dropped_frames: 0,
            failed_frames: 0,
            measured_fps: 0.0,
            average_analysis_ms: 0.0,
            last_result: None,
            error: None,
        }
    }
}

struct AnalysisFrame {
    number: u64,
    jpeg: Vec<u8>,
    submitted_at: Instant,
}

#[derive(Default)]
struct QueueState {
    latest: Option<AnalysisFrame>,
    closed: bool,
}

#[derive(Default)]
struct LatestFrameQueue {
    state: Mutex<QueueState>,
    ready: Condvar,
}

impl LatestFrameQueue {
    fn submit(&self, frame: AnalysisFrame) -> Option<bool> {
        let mut state = lock(&self.state);
        if state.closed {
            return None;
        }
        let replaced = state.latest.replace(frame).is_some();
        self.ready.notify_one();
        Some(replaced)
    }

    fn take_after(&self, deadline: Instant) -> Option<AnalysisFrame> {
        let mut state = lock(&self.state);
        loop {
            if state.closed {
                return None;
            }

            if state.latest.is_none() {
                state = self
                    .ready
                    .wait(state)
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                continue;
            }

            let now = Instant::now();
            if now >= deadline {
                return state.latest.take();
            }

            let wait = deadline.saturating_duration_since(now);
            let (next_state, _) = self
                .ready
                .wait_timeout(state, wait)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state = next_state;
        }
    }

    fn close(&self) {
        let mut state = lock(&self.state);
        state.closed = true;
        state.latest = None;
        self.ready.notify_all();
    }
}

struct ActiveAnalysis {
    queue: Arc<LatestFrameQueue>,
    thread: JoinHandle<()>,
}

pub struct AnalysisManager {
    active: Mutex<Option<ActiveAnalysis>>,
    status: Arc<Mutex<AnalysisStatus>>,
    config: Arc<Mutex<AnalysisConfig>>,
    template: Arc<Mutex<Option<AnalysisTemplate>>>,
}

impl Default for AnalysisManager {
    fn default() -> Self {
        Self {
            active: Mutex::new(None),
            status: Arc::new(Mutex::new(AnalysisStatus::default())),
            config: Arc::new(Mutex::new(AnalysisConfig::default())),
            template: Arc::new(Mutex::new(None)),
        }
    }
}

#[derive(Clone)]
pub struct AnalysisInput {
    queue: Arc<LatestFrameQueue>,
    status: Arc<Mutex<AnalysisStatus>>,
    config: Arc<Mutex<AnalysisConfig>>,
}

impl AnalysisInput {
    pub fn submit(&self, frame_number: u64, jpeg: &[u8]) {
        if !lock(&self.config).enabled {
            return;
        }
        let replaced = self.queue.submit(AnalysisFrame {
            number: frame_number,
            jpeg: jpeg.to_vec(),
            submitted_at: Instant::now(),
        });
        if let Some(replaced) = replaced {
            update_status(&self.status, |status| {
                status.submitted_frames += 1;
                if replaced {
                    status.dropped_frames += 1;
                }
            });
        }
    }

    pub fn close(&self) {
        self.queue.close();
    }
}

impl AnalysisManager {
    pub fn start(&self) -> Result<AnalysisInput, String> {
        self.stop();

        let queue = Arc::new(LatestFrameQueue::default());
        let thread_queue = Arc::clone(&queue);
        let thread_status = Arc::clone(&self.status);
        let thread_config = Arc::clone(&self.config);
        let thread_template = Arc::clone(&self.template);
        let initial_config = lock(&self.config).clone();
        update_status(&self.status, |status| {
            *status = AnalysisStatus {
                state: AnalysisState::Running,
                config: initial_config,
                ..AnalysisStatus::default()
            };
        });

        let worker = thread::Builder::new()
            .name("shadowcast-analysis".to_owned())
            .spawn(move || {
                run_analysis_worker(thread_queue, thread_status, thread_config, thread_template);
            })
            .map_err(|error| {
                let message = format!("Failed to spawn analysis worker: {error}");
                update_status(&self.status, |status| {
                    status.state = AnalysisState::Error;
                    status.error = Some(message.clone());
                });
                message
            })?;

        *lock(&self.active) = Some(ActiveAnalysis {
            queue: Arc::clone(&queue),
            thread: worker,
        });
        Ok(AnalysisInput {
            queue,
            status: Arc::clone(&self.status),
            config: Arc::clone(&self.config),
        })
    }

    pub fn stop(&self) {
        if let Some(active) = lock(&self.active).take() {
            active.queue.close();
            if active.thread.join().is_err() {
                update_status(&self.status, |status| {
                    status.state = AnalysisState::Error;
                    status.error = Some("Analysis worker panicked while stopping".to_owned());
                });
                return;
            }
        }
        update_status(&self.status, |status| {
            if !matches!(status.state, AnalysisState::Error) {
                status.state = AnalysisState::Stopped;
                status.measured_fps = 0.0;
            }
        });
    }

    fn status(&self) -> AnalysisStatus {
        lock(&self.status).clone()
    }

    fn configure(&self, config: AnalysisConfig) -> Result<AnalysisStatus, String> {
        validate_config(&config)?;
        {
            let mut current_config = lock(&self.config);
            let current_template = lock(&self.template);
            validate_template_fits_roi(current_template.as_ref(), config.roi)?;
            *current_config = config.clone();
        }
        update_status(&self.status, |status| {
            if !config.enabled {
                status.measured_fps = 0.0;
            }
            status.config = config;
        });
        Ok(self.status())
    }

    fn set_template(&self, input: Option<AnalysisTemplateInput>) -> Result<(), String> {
        let template = input.map(AnalysisTemplate::try_from).transpose()?;
        let current_config = lock(&self.config);
        let mut current_template = lock(&self.template);
        validate_template_fits_roi(template.as_ref(), current_config.roi)?;
        *current_template = template;
        Ok(())
    }
}

impl Drop for AnalysisManager {
    fn drop(&mut self) {
        if let Ok(active) = self.active.get_mut() {
            if let Some(active) = active.take() {
                active.queue.close();
                let _ = active.thread.join();
            }
        }
    }
}

impl TryFrom<AnalysisTemplateInput> for AnalysisTemplate {
    type Error = String;

    fn try_from(input: AnalysisTemplateInput) -> Result<Self, Self::Error> {
        if input.width == 0 || input.height == 0 {
            return Err("Template dimensions must be greater than zero".to_owned());
        }
        if input.width > MAX_TEMPLATE_DIMENSION || input.height > MAX_TEMPLATE_DIMENSION {
            return Err(format!(
                "Template dimensions must not exceed {MAX_TEMPLATE_DIMENSION} x {MAX_TEMPLATE_DIMENSION}"
            ));
        }
        let expected = u64::from(input.width) * u64::from(input.height);
        if input.grayscale.len() as u64 != expected {
            return Err(format!(
                "Template contains {} pixels; expected {expected}",
                input.grayscale.len()
            ));
        }
        Ok(Self {
            width: input.width,
            height: input.height,
            grayscale: input.grayscale,
        })
    }
}

#[tauri::command]
pub fn get_analysis_status(manager: tauri::State<'_, AnalysisManager>) -> AnalysisStatus {
    manager.status()
}

#[tauri::command]
pub fn configure_analysis(
    manager: tauri::State<'_, AnalysisManager>,
    config: AnalysisConfig,
) -> Result<AnalysisStatus, String> {
    manager.configure(config)
}

#[tauri::command]
pub fn set_analysis_template(
    manager: tauri::State<'_, AnalysisManager>,
    template: Option<AnalysisTemplateInput>,
) -> Result<(), String> {
    manager.set_template(template)
}

fn run_analysis_worker(
    queue: Arc<LatestFrameQueue>,
    status: Arc<Mutex<AnalysisStatus>>,
    config: Arc<Mutex<AnalysisConfig>>,
    template: Arc<Mutex<Option<AnalysisTemplate>>>,
) {
    info!("analysis worker started");
    let mut next_allowed = Instant::now();
    let mut interval_started = Instant::now();
    let mut interval_frames = 0_u64;
    let mut total_analysis_time = Duration::ZERO;

    while let Some(frame) = queue.take_after(next_allowed) {
        let (current_config, current_template) = {
            let current_config = lock(&config);
            let current_template = lock(&template);
            (current_config.clone(), current_template.clone())
        };
        if !current_config.enabled {
            continue;
        }
        let started = Instant::now();
        let queue_delay = started.saturating_duration_since(frame.submitted_at);
        match analyze_jpeg(
            &frame.jpeg,
            frame.number,
            &current_config,
            current_template.as_ref(),
        ) {
            Ok(mut result) => {
                let elapsed = started.elapsed();
                result.queue_delay_ms = queue_delay.as_secs_f64() * 1_000.0;
                result.analysis_ms = elapsed.as_secs_f64() * 1_000.0;
                total_analysis_time += elapsed;
                interval_frames += 1;

                let interval_elapsed = interval_started.elapsed();
                let measured_fps = if interval_elapsed >= Duration::from_secs(1) {
                    let fps = interval_frames as f64 / interval_elapsed.as_secs_f64();
                    interval_started = Instant::now();
                    interval_frames = 0;
                    Some(fps)
                } else {
                    None
                };
                record_analysis_success(&status, total_analysis_time, measured_fps, result);
            }
            Err(message) => {
                warn!(error = %message, frame_number = frame.number, "frame analysis failed");
                update_status(&status, |analysis_status| {
                    analysis_status.failed_frames += 1;
                    analysis_status.error = Some(message);
                });
            }
        }

        let interval = Duration::from_secs_f64(1.0 / f64::from(current_config.max_fps));
        next_allowed = started + interval;
    }

    update_status(&status, |analysis_status| {
        if !matches!(analysis_status.state, AnalysisState::Error) {
            analysis_status.state = AnalysisState::Stopped;
            analysis_status.measured_fps = 0.0;
        }
    });
    info!("analysis worker stopped");
}

fn record_analysis_success(
    status: &Arc<Mutex<AnalysisStatus>>,
    total_analysis_time: Duration,
    measured_fps: Option<f64>,
    result: AnalysisResult,
) {
    update_status(status, |analysis_status| {
        analysis_status.analyzed_frames += 1;
        analysis_status.average_analysis_ms =
            total_analysis_time.as_secs_f64() * 1_000.0 / analysis_status.analyzed_frames as f64;
        if analysis_status.config.enabled {
            if let Some(fps) = measured_fps {
                analysis_status.measured_fps = fps;
            }
        } else {
            analysis_status.measured_fps = 0.0;
        }
        analysis_status.last_result = Some(result);
        analysis_status.error = None;
    });
}

fn analyze_jpeg(
    jpeg: &[u8],
    frame_number: u64,
    config: &AnalysisConfig,
    template: Option<&AnalysisTemplate>,
) -> Result<AnalysisResult, String> {
    let decode_started = Instant::now();
    let image = image::load_from_memory(jpeg)
        .map_err(|error| format!("Failed to decode analysis frame: {error}"))?
        .to_rgb8();
    let jpeg_decode_ms = decode_started.elapsed().as_secs_f64() * 1_000.0;
    let mut result = analyze_rgb(&image, frame_number, config, template)?;
    result.jpeg_decode_ms = jpeg_decode_ms;
    Ok(result)
}

fn analyze_rgb(
    image: &RgbImage,
    frame_number: u64,
    config: &AnalysisConfig,
    template: Option<&AnalysisTemplate>,
) -> Result<AnalysisResult, String> {
    let color_started = Instant::now();
    let roi = clamp_roi(config.roi, image.width(), image.height())?;
    let mut sums = [0_u64; 3];
    let mut matching_pixels = 0_u64;

    for y in roi.y..roi.y + roi.height {
        for x in roi.x..roi.x + roi.width {
            let pixel = image.get_pixel(x, y).0;
            sums[0] += u64::from(pixel[0]);
            sums[1] += u64::from(pixel[1]);
            sums[2] += u64::from(pixel[2]);
            if channel_matches(pixel[0], config.target_color.red, config.color_tolerance)
                && channel_matches(pixel[1], config.target_color.green, config.color_tolerance)
                && channel_matches(pixel[2], config.target_color.blue, config.color_tolerance)
            {
                matching_pixels += 1;
            }
        }
    }

    let total_pixels = u64::from(roi.width) * u64::from(roi.height);
    let average = RgbColor {
        red: (sums[0] / total_pixels) as u8,
        green: (sums[1] / total_pixels) as u8,
        blue: (sums[2] / total_pixels) as u8,
    };
    let color_analysis_ms = color_started.elapsed().as_secs_f64() * 1_000.0;
    let (template_match, template_match_ms) = if let Some(template) = template {
        let template_started = Instant::now();
        let template_match = match_template(image, roi, template)?;
        (
            Some(template_match),
            template_started.elapsed().as_secs_f64() * 1_000.0,
        )
    } else {
        (None, 0.0)
    };

    Ok(AnalysisResult {
        frame_number,
        source_width: image.width(),
        source_height: image.height(),
        roi,
        color: ColorAnalysis {
            target: config.target_color,
            tolerance: config.color_tolerance,
            average,
            matching_pixels,
            total_pixels,
            match_ratio: matching_pixels as f64 / total_pixels as f64,
        },
        template_match,
        queue_delay_ms: 0.0,
        analysis_ms: 0.0,
        jpeg_decode_ms: 0.0,
        color_analysis_ms,
        template_match_ms,
    })
}

fn clamp_roi(roi: Roi, image_width: u32, image_height: u32) -> Result<Roi, String> {
    if roi.width == 0 || roi.height == 0 {
        return Err("ROI dimensions must be greater than zero".to_owned());
    }
    if roi.x >= image_width || roi.y >= image_height {
        return Err(format!(
            "ROI origin ({}, {}) is outside the {image_width} x {image_height} frame",
            roi.x, roi.y
        ));
    }
    Ok(Roi {
        x: roi.x,
        y: roi.y,
        width: roi.width.min(image_width - roi.x),
        height: roi.height.min(image_height - roi.y),
    })
}

fn match_template(
    image: &RgbImage,
    roi: Roi,
    template: &AnalysisTemplate,
) -> Result<TemplateMatch, String> {
    if template.width > roi.width || template.height > roi.height {
        return Err(format!(
            "Template {} x {} does not fit ROI {} x {}",
            template.width, template.height, roi.width, roi.height
        ));
    }

    let positions_x = roi.width - template.width + 1;
    let positions_y = roi.height - template.height + 1;
    let comparisons = u64::from(positions_x)
        * u64::from(positions_y)
        * u64::from(template.width)
        * u64::from(template.height);
    let search_step = comparison_step(comparisons);
    let mut best = (u64::MAX, roi.x, roi.y);

    for offset_y in stepped_offsets(positions_y, search_step) {
        for offset_x in stepped_offsets(positions_x, search_step) {
            let mut difference = 0_u64;
            for template_y in 0..template.height {
                for template_x in 0..template.width {
                    let pixel = image
                        .get_pixel(roi.x + offset_x + template_x, roi.y + offset_y + template_y);
                    let gray = rgb_to_gray(pixel.0);
                    let template_index = (template_y * template.width + template_x) as usize;
                    difference += u64::from(gray.abs_diff(template.grayscale[template_index]));
                }
            }
            if difference < best.0 {
                best = (difference, roi.x + offset_x, roi.y + offset_y);
            }
        }
    }

    let max_difference = 255.0 * f64::from(template.width) * f64::from(template.height);
    Ok(TemplateMatch {
        x: best.1,
        y: best.2,
        width: template.width,
        height: template.height,
        score: (1.0 - best.0 as f64 / max_difference).clamp(0.0, 1.0),
        search_step,
    })
}

fn stepped_offsets(position_count: u32, step: u32) -> impl Iterator<Item = u32> {
    let last = position_count - 1;
    (0..position_count)
        .step_by(step as usize)
        .chain((!last.is_multiple_of(step)).then_some(last))
}

fn comparison_step(comparisons: u64) -> u32 {
    if comparisons <= TEMPLATE_COMPARISON_BUDGET {
        return 1;
    }
    ((comparisons as f64 / TEMPLATE_COMPARISON_BUDGET as f64)
        .sqrt()
        .ceil() as u32)
        .max(1)
}

fn rgb_to_gray([red, green, blue]: [u8; 3]) -> u8 {
    ((u32::from(red) * 77 + u32::from(green) * 150 + u32::from(blue) * 29) >> 8) as u8
}

fn channel_matches(actual: u8, target: u8, tolerance: u8) -> bool {
    actual.abs_diff(target) <= tolerance
}

fn validate_config(config: &AnalysisConfig) -> Result<(), String> {
    if config.roi.width == 0 || config.roi.height == 0 {
        return Err("ROI dimensions must be greater than zero".to_owned());
    }
    if config.max_fps == 0 || config.max_fps > MAX_ANALYSIS_FPS {
        return Err(format!(
            "Analysis max FPS must be between 1 and {MAX_ANALYSIS_FPS}"
        ));
    }
    Ok(())
}

fn validate_template_fits_roi(template: Option<&AnalysisTemplate>, roi: Roi) -> Result<(), String> {
    if let Some(template) = template {
        if template.width > roi.width || template.height > roi.height {
            return Err(format!(
                "Template {} x {} does not fit configured ROI {} x {}",
                template.width, template.height, roi.width, roi.height
            ));
        }
    }
    Ok(())
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn update_status(status: &Arc<Mutex<AnalysisStatus>>, update: impl FnOnce(&mut AnalysisStatus)) {
    update(&mut lock(status));
}

#[cfg(test)]
mod tests {
    use image::{Rgb, RgbImage};

    use super::*;

    fn config(roi: Roi) -> AnalysisConfig {
        AnalysisConfig {
            enabled: true,
            roi,
            target_color: RgbColor {
                red: 0,
                green: 255,
                blue: 0,
            },
            color_tolerance: 0,
            max_fps: 15,
        }
    }

    #[test]
    fn roi_and_color_analysis_return_structured_counts() {
        let mut image = RgbImage::from_pixel(4, 3, Rgb([10, 20, 30]));
        image.put_pixel(1, 1, Rgb([0, 255, 0]));
        image.put_pixel(2, 1, Rgb([0, 255, 0]));

        let result = analyze_rgb(
            &image,
            7,
            &config(Roi {
                x: 1,
                y: 1,
                width: 2,
                height: 1,
            }),
            None,
        )
        .expect("ROI should be analyzed");

        assert_eq!(result.frame_number, 7);
        assert_eq!(result.roi.width, 2);
        assert_eq!(result.color.matching_pixels, 2);
        assert_eq!(result.color.match_ratio, 1.0);
        assert_eq!(result.color.average.green, 255);
    }

    #[test]
    fn template_matching_finds_the_best_location_inside_roi() {
        let mut image = RgbImage::from_pixel(5, 4, Rgb([0, 0, 0]));
        image.put_pixel(3, 1, Rgb([255, 255, 255]));
        image.put_pixel(3, 2, Rgb([128, 128, 128]));
        let template = AnalysisTemplate {
            width: 1,
            height: 2,
            grayscale: vec![255, 128],
        };

        let result = match_template(
            &image,
            Roi {
                x: 1,
                y: 1,
                width: 4,
                height: 3,
            },
            &template,
        )
        .expect("template should fit");

        assert_eq!((result.x, result.y), (3, 1));
        assert_eq!(result.score, 1.0);
        assert_eq!(result.search_step, 1);
    }

    #[test]
    fn stepped_template_search_includes_right_and_bottom_edge() {
        let mut image = RgbImage::from_pixel(2_002, 2_002, Rgb([0, 0, 0]));
        image.put_pixel(2_001, 2_001, Rgb([255, 255, 255]));
        let template = AnalysisTemplate {
            width: 1,
            height: 1,
            grayscale: vec![255],
        };

        let result = match_template(
            &image,
            Roi {
                x: 0,
                y: 0,
                width: 2_002,
                height: 2_002,
            },
            &template,
        )
        .expect("template should fit");

        assert_eq!(result.search_step, 2);
        assert_eq!((result.x, result.y), (2_001, 2_001));
        assert_eq!(result.score, 1.0);
    }

    #[test]
    fn latest_frame_queue_replaces_pending_work() {
        let queue = LatestFrameQueue::default();
        let frame = |number| AnalysisFrame {
            number,
            jpeg: vec![],
            submitted_at: Instant::now(),
        };

        assert_eq!(queue.submit(frame(1)), Some(false));
        assert_eq!(queue.submit(frame(2)), Some(true));
        assert_eq!(
            queue.take_after(Instant::now()).map(|item| item.number),
            Some(2)
        );
    }

    #[test]
    fn invalid_configuration_and_template_are_rejected() {
        let invalid_config = AnalysisConfig {
            max_fps: 0,
            ..AnalysisConfig::default()
        };
        assert!(validate_config(&invalid_config).is_err());

        let invalid_template = AnalysisTemplateInput {
            width: 2,
            height: 2,
            grayscale: vec![0; 3],
        };
        assert!(AnalysisTemplate::try_from(invalid_template).is_err());
    }

    #[test]
    fn template_and_configured_roi_must_remain_compatible() {
        let manager = AnalysisManager::default();
        manager
            .configure(config(Roi {
                x: 0,
                y: 0,
                width: 2,
                height: 2,
            }))
            .expect("configuration should be valid");

        let oversized = AnalysisTemplateInput {
            width: 3,
            height: 2,
            grayscale: vec![0; 6],
        };
        assert!(manager.set_template(Some(oversized)).is_err());

        manager
            .set_template(Some(AnalysisTemplateInput {
                width: 2,
                height: 2,
                grayscale: vec![0; 4],
            }))
            .expect("template should fit current ROI");

        let too_small = config(Roi {
            x: 0,
            y: 0,
            width: 1,
            height: 2,
        });
        assert!(manager.configure(too_small).is_err());
        assert_eq!(manager.status().config.roi.width, 2);
    }

    #[test]
    fn completed_frame_does_not_restore_stale_configuration_after_disable() {
        let image = RgbImage::from_pixel(1, 1, Rgb([0, 255, 0]));
        let result = analyze_rgb(
            &image,
            1,
            &config(Roi {
                x: 0,
                y: 0,
                width: 1,
                height: 1,
            }),
            None,
        )
        .expect("frame should be analyzed");
        let status = Arc::new(Mutex::new(AnalysisStatus::default()));
        update_status(&status, |current| {
            current.config.enabled = false;
            current.measured_fps = 12.0;
        });

        record_analysis_success(&status, Duration::from_millis(2), Some(15.0), result);

        let current = lock(&status);
        assert!(!current.config.enabled);
        assert_eq!(current.measured_fps, 0.0);
        assert_eq!(current.analyzed_frames, 1);
    }
}
