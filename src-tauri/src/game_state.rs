use std::{
    cmp::Ordering,
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use image::{GrayImage, RgbImage};
use serde::{Deserialize, Serialize};

const DEFAULT_GAME_ID: &str = "sample-switch-game";
const TEMPLATE_COMPARISON_BUDGET: u64 = 4_000_000;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct StabilityConfig {
    #[serde(default = "default_confirmation_frames")]
    pub consecutive_frames: u32,
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,
}

impl Default for StabilityConfig {
    fn default() -> Self {
        Self {
            consecutive_frames: default_confirmation_frames(),
            timeout_ms: default_timeout_ms(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GameConfig {
    pub id: String,
    pub name: String,
    pub resolution: [u32; 2],
    #[serde(default)]
    pub defaults: StabilityConfig,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Combination {
    #[default]
    All,
    Any,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SceneFile {
    id: String,
    #[serde(default)]
    priority: i32,
    #[serde(default)]
    combination: Combination,
    detectors: Vec<DetectorConfig>,
    stability: Option<StabilityConfig>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum DetectorConfig {
    Luma {
        region: [u32; 4],
        min: Option<f64>,
        max: Option<f64>,
    },
    ColorRatio {
        region: [u32; 4],
        target: [u8; 3],
        tolerance: u8,
        min_ratio: f64,
    },
    Template {
        region: [u32; 4],
        image: String,
        threshold: f64,
    },
    EdgeDensity {
        region: [u32; 4],
        difference_threshold: u8,
        min_ratio: f64,
    },
}

#[derive(Clone, Debug)]
enum Detector {
    Luma {
        region: [u32; 4],
        min: Option<f64>,
        max: Option<f64>,
    },
    ColorRatio {
        region: [u32; 4],
        target: [u8; 3],
        tolerance: u8,
        min_ratio: f64,
    },
    Template {
        region: [u32; 4],
        image_path: String,
        image: GrayImage,
        threshold: f64,
    },
    EdgeDensity {
        region: [u32; 4],
        difference_threshold: u8,
        min_ratio: f64,
    },
}

#[derive(Clone, Debug)]
struct SceneDefinition {
    id: String,
    priority: i32,
    combination: Combination,
    detectors: Vec<Detector>,
    stability: StabilityConfig,
}

#[derive(Clone, Debug)]
pub struct GameProfile {
    config: GameConfig,
    scenes: Vec<SceneDefinition>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SceneSummary {
    pub id: String,
    pub detector_count: usize,
    pub combination: Combination,
    pub stability: StabilityConfig,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GameProfileSummary {
    pub game_id: String,
    pub game_name: String,
    pub resolution: [u32; 2],
    pub scenes: Vec<SceneSummary>,
}

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DetectorEvidence {
    pub detector_type: String,
    pub matched: bool,
    pub confidence: f64,
    pub observed: f64,
    pub expected: String,
    pub region: [u32; 4],
    pub detail: String,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SceneDetection {
    pub game_id: String,
    pub scene_id: String,
    pub confidence: f64,
    pub detected_at_ms: u64,
    pub frame_number: u64,
    pub evidence: Vec<DetectorEvidence>,
    pub consecutive_frames: u32,
    pub candidate_scene_id: Option<String>,
    pub candidate_consecutive_frames: u32,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SceneTransition {
    pub from_scene_id: String,
    pub detection: SceneDetection,
    pub reason: String,
}

#[derive(Clone, Debug)]
struct Candidate {
    scene_id: String,
    confidence: f64,
    evidence: Vec<DetectorEvidence>,
}

pub struct SceneDetector {
    profile: Arc<GameProfile>,
    snapshot: SceneDetection,
    pending: Option<Candidate>,
    pending_frames: u32,
    unstable_since_ms: Option<u64>,
}

impl GameProfile {
    pub fn load(games_root: &Path, game_id: &str) -> Result<Self, String> {
        validate_game_id(game_id)?;
        let game_dir = games_root.join(game_id);
        let game_path = game_dir.join("game.yaml");
        let config: GameConfig = read_yaml(&game_path)?;
        if config.id != game_id {
            return Err(format!(
                "{} declares game id {:?}; expected {:?}",
                game_path.display(),
                config.id,
                game_id
            ));
        }
        validate_game_config(&config)?;

        let scene_dir = game_dir.join("scenes");
        let mut paths = fs::read_dir(&scene_dir)
            .map_err(|error| format!("Failed to read {}: {error}", scene_dir.display()))?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                matches!(
                    path.extension().and_then(|extension| extension.to_str()),
                    Some("yaml" | "yml")
                )
            })
            .collect::<Vec<_>>();
        paths.sort();
        if paths.is_empty() {
            return Err(format!(
                "No scene YAML files found in {}",
                scene_dir.display()
            ));
        }

        let canonical_game_dir = game_dir.canonicalize().map_err(|error| {
            format!(
                "Failed to resolve game directory {}: {error}",
                game_dir.display()
            )
        })?;
        let mut ids = HashSet::new();
        let mut scenes = Vec::with_capacity(paths.len());
        for path in paths {
            let scene: SceneFile = read_yaml(&path)?;
            if scene.id.trim().is_empty() || scene.id == "unknown" {
                return Err(format!(
                    "{} contains an empty or reserved scene id",
                    path.display()
                ));
            }
            if !ids.insert(scene.id.clone()) {
                return Err(format!("Duplicate scene id {:?}", scene.id));
            }
            if scene.detectors.is_empty() {
                return Err(format!(
                    "Scene {:?} must define at least one detector",
                    scene.id
                ));
            }
            let stability = scene.stability.unwrap_or_else(|| config.defaults.clone());
            validate_stability(&stability, &format!("scene {:?}", scene.id))?;
            let detectors = scene
                .detectors
                .into_iter()
                .map(|detector| {
                    load_detector(detector, &game_dir, &canonical_game_dir, config.resolution)
                })
                .collect::<Result<Vec<_>, _>>()?;
            scenes.push(SceneDefinition {
                id: scene.id,
                priority: scene.priority,
                combination: scene.combination,
                detectors,
                stability,
            });
        }
        Ok(Self { config, scenes })
    }

    pub fn summary(&self) -> GameProfileSummary {
        GameProfileSummary {
            game_id: self.config.id.clone(),
            game_name: self.config.name.clone(),
            resolution: self.config.resolution,
            scenes: self
                .scenes
                .iter()
                .map(|scene| SceneSummary {
                    id: scene.id.clone(),
                    detector_count: scene.detectors.len(),
                    combination: scene.combination,
                    stability: scene.stability.clone(),
                })
                .collect(),
        }
    }

    fn scene(&self, id: &str) -> Option<&SceneDefinition> {
        self.scenes.iter().find(|scene| scene.id == id)
    }

    fn classify(&self, image: &RgbImage) -> Result<Option<Candidate>, String> {
        if [image.width(), image.height()] != self.config.resolution {
            return Err(format!(
                "Game {:?} expects {} x {} frames, received {} x {}",
                self.config.id,
                self.config.resolution[0],
                self.config.resolution[1],
                image.width(),
                image.height()
            ));
        }
        let mut matches = Vec::new();
        for scene in &self.scenes {
            let evidence = scene
                .detectors
                .iter()
                .map(|detector| evaluate_detector(image, detector))
                .collect::<Result<Vec<_>, _>>()?;
            let matched = match scene.combination {
                Combination::All => evidence.iter().all(|item| item.matched),
                Combination::Any => evidence.iter().any(|item| item.matched),
            };
            if matched {
                let matched_evidence = evidence.iter().filter(|item| item.matched);
                let count = matched_evidence.clone().count();
                let confidence =
                    matched_evidence.map(|item| item.confidence).sum::<f64>() / count as f64;
                matches.push((
                    scene.priority,
                    Candidate {
                        scene_id: scene.id.clone(),
                        confidence,
                        evidence,
                    },
                ));
            }
        }
        matches.sort_by(|left, right| {
            right
                .0
                .cmp(&left.0)
                .then_with(|| {
                    right
                        .1
                        .confidence
                        .partial_cmp(&left.1.confidence)
                        .unwrap_or(Ordering::Equal)
                })
                .then_with(|| left.1.scene_id.cmp(&right.1.scene_id))
        });
        Ok(matches.into_iter().next().map(|(_, candidate)| candidate))
    }
}

impl SceneDetector {
    pub fn new(profile: Arc<GameProfile>) -> Self {
        let snapshot = unknown_detection(&profile.config.id, 0, 0, "analysis has not started");
        Self {
            profile,
            snapshot,
            pending: None,
            pending_frames: 0,
            unstable_since_ms: None,
        }
    }

    pub fn snapshot(&self) -> SceneDetection {
        self.snapshot.clone()
    }

    pub fn observe(
        &mut self,
        image: &RgbImage,
        frame_number: u64,
        detected_at_ms: u64,
        elapsed_ms: u64,
    ) -> Result<Option<SceneTransition>, String> {
        let candidate = self.profile.classify(image)?;
        Ok(self.observe_candidate(candidate, frame_number, detected_at_ms, elapsed_ms))
    }

    pub fn observe_unavailable(
        &mut self,
        frame_number: u64,
        detected_at_ms: u64,
        elapsed_ms: u64,
    ) -> Option<SceneTransition> {
        self.observe_candidate(None, frame_number, detected_at_ms, elapsed_ms)
    }

    pub fn advance_time(
        &mut self,
        frame_number: u64,
        detected_at_ms: u64,
        elapsed_ms: u64,
    ) -> Option<SceneTransition> {
        self.timeout_if_unstable(frame_number, detected_at_ms, elapsed_ms)
    }

    fn observe_candidate(
        &mut self,
        candidate: Option<Candidate>,
        frame_number: u64,
        detected_at_ms: u64,
        elapsed_ms: u64,
    ) -> Option<SceneTransition> {
        let Some(candidate) = candidate else {
            self.pending = None;
            self.pending_frames = 0;
            self.snapshot.candidate_scene_id = None;
            self.snapshot.candidate_consecutive_frames = 0;
            if self.snapshot.scene_id == "unknown" {
                self.snapshot = unknown_detection(
                    &self.profile.config.id,
                    frame_number,
                    detected_at_ms,
                    "no configured scene matched",
                );
            }
            return self.timeout_if_unstable(frame_number, detected_at_ms, elapsed_ms);
        };

        if self.pending.as_ref().map(|item| item.scene_id.as_str())
            == Some(candidate.scene_id.as_str())
        {
            self.pending_frames = self.pending_frames.saturating_add(1);
        } else {
            self.pending_frames = 1;
        }
        self.pending = Some(candidate.clone());
        self.snapshot.candidate_scene_id = Some(candidate.scene_id.clone());
        self.snapshot.candidate_consecutive_frames = self.pending_frames;

        if candidate.scene_id == self.snapshot.scene_id {
            self.unstable_since_ms = None;
            self.snapshot = detection_from(
                &self.profile.config.id,
                candidate,
                frame_number,
                detected_at_ms,
                self.pending_frames,
            );
            return None;
        }

        let confirmation_frames = self
            .profile
            .scene(&candidate.scene_id)
            .expect("classified scene must exist")
            .stability
            .consecutive_frames;
        if self.pending_frames >= confirmation_frames {
            self.unstable_since_ms = None;
            return Some(self.transition(
                candidate,
                frame_number,
                detected_at_ms,
                self.pending_frames,
                format!("scene matched for {confirmation_frames} consecutive frames"),
            ));
        }
        self.timeout_if_unstable(frame_number, detected_at_ms, elapsed_ms)
    }

    fn timeout_if_unstable(
        &mut self,
        frame_number: u64,
        detected_at_ms: u64,
        elapsed_ms: u64,
    ) -> Option<SceneTransition> {
        if self.snapshot.scene_id == "unknown" {
            self.unstable_since_ms = None;
            return None;
        }
        let timeout_ms = self
            .profile
            .scene(&self.snapshot.scene_id)
            .map_or(self.profile.config.defaults.timeout_ms, |scene| {
                scene.stability.timeout_ms
            });
        let unstable_since = *self.unstable_since_ms.get_or_insert(elapsed_ms);
        let unstable_for_ms = elapsed_ms.saturating_sub(unstable_since);
        if unstable_for_ms < timeout_ms {
            return None;
        }

        self.unstable_since_ms = None;
        let from_scene_id = self.snapshot.scene_id.clone();
        let reason =
            format!("no stable scene observed for {unstable_for_ms} ms (timeout {timeout_ms} ms)");
        self.snapshot = unknown_detection(
            &self.profile.config.id,
            frame_number,
            detected_at_ms,
            &reason,
        );
        Some(SceneTransition {
            from_scene_id,
            detection: self.snapshot.clone(),
            reason,
        })
    }

    fn transition(
        &mut self,
        candidate: Candidate,
        frame_number: u64,
        detected_at_ms: u64,
        consecutive_frames: u32,
        reason: String,
    ) -> SceneTransition {
        let from_scene_id = self.snapshot.scene_id.clone();
        self.snapshot = detection_from(
            &self.profile.config.id,
            candidate,
            frame_number,
            detected_at_ms,
            consecutive_frames,
        );
        SceneTransition {
            from_scene_id,
            detection: self.snapshot.clone(),
            reason,
        }
    }
}

pub fn default_games_root() -> PathBuf {
    std::env::var_os("SHADOWCAST_GAME_CONFIG_ROOT").map_or_else(
        || {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .expect("src-tauri must have a repository parent")
                .join("config/games")
        },
        PathBuf::from,
    )
}

pub fn load_default_profile(games_root: &Path) -> Result<GameProfile, String> {
    GameProfile::load(games_root, DEFAULT_GAME_ID)
}

fn read_yaml<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, String> {
    let contents = fs::read_to_string(path)
        .map_err(|error| format!("Failed to read {}: {error}", path.display()))?;
    serde_yaml::from_str(&contents)
        .map_err(|error| format!("Failed to parse {}: {error}", path.display()))
}

fn load_detector(
    config: DetectorConfig,
    game_dir: &Path,
    canonical_game_dir: &Path,
    resolution: [u32; 2],
) -> Result<Detector, String> {
    let region = match &config {
        DetectorConfig::Luma { region, .. }
        | DetectorConfig::ColorRatio { region, .. }
        | DetectorConfig::Template { region, .. }
        | DetectorConfig::EdgeDensity { region, .. } => *region,
    };
    validate_region(region, resolution)?;
    match config {
        DetectorConfig::Luma { region, min, max } => {
            if min.is_none() && max.is_none() {
                return Err("A luma detector requires min and/or max".to_owned());
            }
            for value in min.into_iter().chain(max) {
                validate_range(value, 0.0, 255.0, "luma threshold")?;
            }
            if min.zip(max).is_some_and(|(min, max)| min > max) {
                return Err("Luma min must not exceed max".to_owned());
            }
            Ok(Detector::Luma { region, min, max })
        }
        DetectorConfig::ColorRatio {
            region,
            target,
            tolerance,
            min_ratio,
        } => {
            validate_range(min_ratio, 0.0, 1.0, "color min_ratio")?;
            Ok(Detector::ColorRatio {
                region,
                target,
                tolerance,
                min_ratio,
            })
        }
        DetectorConfig::Template {
            region,
            image,
            threshold,
        } => {
            validate_range(threshold, 0.0, 1.0, "template threshold")?;
            let path = game_dir.join(&image);
            let canonical_path = path.canonicalize().map_err(|error| {
                format!("Failed to resolve template {}: {error}", path.display())
            })?;
            if !canonical_path.starts_with(canonical_game_dir) {
                return Err(format!(
                    "Template {} is outside the game directory",
                    path.display()
                ));
            }
            let template = image::open(&canonical_path)
                .map_err(|error| format!("Failed to decode template {}: {error}", path.display()))?
                .to_luma8();
            if template.width() == 0
                || template.height() == 0
                || template.width() > region[2]
                || template.height() > region[3]
            {
                return Err(format!(
                    "Template {} does not fit configured region",
                    path.display()
                ));
            }
            Ok(Detector::Template {
                region,
                image_path: image,
                image: template,
                threshold,
            })
        }
        DetectorConfig::EdgeDensity {
            region,
            difference_threshold,
            min_ratio,
        } => {
            validate_range(min_ratio, 0.0, 1.0, "edge min_ratio")?;
            Ok(Detector::EdgeDensity {
                region,
                difference_threshold,
                min_ratio,
            })
        }
    }
}

fn evaluate_detector(image: &RgbImage, detector: &Detector) -> Result<DetectorEvidence, String> {
    match detector {
        Detector::Luma { region, min, max } => {
            let mut total = 0_u64;
            for_each_pixel(image, *region, |pixel| {
                total += u64::from(rgb_to_gray(pixel))
            });
            let count = u64::from(region[2]) * u64::from(region[3]);
            let average = total as f64 / count as f64;
            let matched = min.is_none_or(|value| average >= value)
                && max.is_none_or(|value| average <= value);
            let expected = match (min, max) {
                (Some(min), Some(max)) => format!("{min:.1}..={max:.1}"),
                (Some(min), None) => format!(">= {min:.1}"),
                (None, Some(max)) => format!("<= {max:.1}"),
                _ => unreachable!(),
            };
            let confidence = if matched {
                match (min, max) {
                    (None, Some(max)) => (1.0 - average / (max + 1.0)).clamp(0.0, 1.0),
                    (Some(_), None) => (average / 255.0).clamp(0.0, 1.0),
                    _ => 1.0,
                }
            } else {
                0.0
            };
            Ok(evidence(
                "luma",
                matched,
                confidence,
                average,
                expected,
                *region,
                format!("average luma {average:.2}"),
            ))
        }
        Detector::ColorRatio {
            region,
            target,
            tolerance,
            min_ratio,
        } => {
            let mut matching = 0_u64;
            for_each_pixel(image, *region, |pixel| {
                if pixel[0].abs_diff(target[0]) <= *tolerance
                    && pixel[1].abs_diff(target[1]) <= *tolerance
                    && pixel[2].abs_diff(target[2]) <= *tolerance
                {
                    matching += 1;
                }
            });
            let count = u64::from(region[2]) * u64::from(region[3]);
            let ratio = matching as f64 / count as f64;
            let matched = ratio >= *min_ratio;
            Ok(evidence(
                "color_ratio",
                matched,
                if matched { ratio } else { 0.0 },
                ratio,
                format!(">= {min_ratio:.3}"),
                *region,
                format!(
                    "{matching}/{count} pixels match rgb({}, {}, {}) ±{}",
                    target[0], target[1], target[2], tolerance
                ),
            ))
        }
        Detector::Template {
            region,
            image_path,
            image: template,
            threshold,
        } => {
            let (score, x, y, step) = match_template(image, *region, template)?;
            let matched = score >= *threshold;
            Ok(evidence(
                "template",
                matched,
                if matched { score } else { 0.0 },
                score,
                format!(">= {threshold:.3}"),
                *region,
                format!("{image_path} best match at ({x}, {y}), search step {step}"),
            ))
        }
        Detector::EdgeDensity {
            region,
            difference_threshold,
            min_ratio,
        } => {
            let [x, y, width, height] = *region;
            let mut edges = 0_u64;
            let mut comparisons = 0_u64;
            for py in y..y + height {
                for px in x..x + width {
                    let current = rgb_to_gray(image.get_pixel(px, py).0);
                    for neighbor in [
                        (px + 1 < x + width).then_some((px + 1, py)),
                        (py + 1 < y + height).then_some((px, py + 1)),
                    ]
                    .into_iter()
                    .flatten()
                    {
                        comparisons += 1;
                        edges += u64::from(
                            current
                                .abs_diff(rgb_to_gray(image.get_pixel(neighbor.0, neighbor.1).0))
                                >= *difference_threshold,
                        );
                    }
                }
            }
            let ratio = if comparisons == 0 {
                0.0
            } else {
                edges as f64 / comparisons as f64
            };
            let matched = ratio >= *min_ratio;
            Ok(evidence(
                "edge_density",
                matched,
                if matched { ratio } else { 0.0 },
                ratio,
                format!(">= {min_ratio:.3}"),
                *region,
                format!("{edges}/{comparisons} adjacent pairs differ by {difference_threshold}+"),
            ))
        }
    }
}

fn match_template(
    image: &RgbImage,
    [x, y, width, height]: [u32; 4],
    template: &GrayImage,
) -> Result<(f64, u32, u32, u32), String> {
    if template.width() > width || template.height() > height {
        return Err("Template does not fit configured region".to_owned());
    }
    let positions_x = width - template.width() + 1;
    let positions_y = height - template.height() + 1;
    let comparisons = u64::from(positions_x)
        * u64::from(positions_y)
        * u64::from(template.width())
        * u64::from(template.height());
    let step = comparison_step(comparisons);
    let mut best = (u64::MAX, x, y);
    for offset_y in stepped_offsets(positions_y, step) {
        for offset_x in stepped_offsets(positions_x, step) {
            let mut difference = 0_u64;
            for template_y in 0..template.height() {
                for template_x in 0..template.width() {
                    let actual = rgb_to_gray(
                        image
                            .get_pixel(x + offset_x + template_x, y + offset_y + template_y)
                            .0,
                    );
                    difference +=
                        u64::from(actual.abs_diff(template.get_pixel(template_x, template_y).0[0]));
                }
            }
            if difference < best.0 {
                best = (difference, x + offset_x, y + offset_y);
            }
        }
    }
    let max_difference = 255.0 * f64::from(template.width()) * f64::from(template.height());
    Ok((
        (1.0 - best.0 as f64 / max_difference).clamp(0.0, 1.0),
        best.1,
        best.2,
        step,
    ))
}

fn stepped_offsets(count: u32, step: u32) -> impl Iterator<Item = u32> {
    let last = count - 1;
    (0..count)
        .step_by(step as usize)
        .chain((!last.is_multiple_of(step)).then_some(last))
}

fn comparison_step(comparisons: u64) -> u32 {
    if comparisons <= TEMPLATE_COMPARISON_BUDGET {
        1
    } else {
        ((comparisons as f64 / TEMPLATE_COMPARISON_BUDGET as f64)
            .sqrt()
            .ceil() as u32)
            .max(1)
    }
}

fn for_each_pixel(
    image: &RgbImage,
    [x, y, width, height]: [u32; 4],
    mut visit: impl FnMut([u8; 3]),
) {
    for py in y..y + height {
        for px in x..x + width {
            visit(image.get_pixel(px, py).0);
        }
    }
}

fn evidence(
    detector_type: &str,
    matched: bool,
    confidence: f64,
    observed: f64,
    expected: String,
    region: [u32; 4],
    detail: String,
) -> DetectorEvidence {
    DetectorEvidence {
        detector_type: detector_type.to_owned(),
        matched,
        confidence,
        observed,
        expected,
        region,
        detail,
    }
}

fn detection_from(
    game_id: &str,
    candidate: Candidate,
    frame_number: u64,
    detected_at_ms: u64,
    consecutive_frames: u32,
) -> SceneDetection {
    SceneDetection {
        game_id: game_id.to_owned(),
        scene_id: candidate.scene_id.clone(),
        confidence: candidate.confidence,
        detected_at_ms,
        frame_number,
        evidence: candidate.evidence,
        consecutive_frames,
        candidate_scene_id: Some(candidate.scene_id),
        candidate_consecutive_frames: consecutive_frames,
    }
}

fn unknown_detection(
    game_id: &str,
    frame_number: u64,
    detected_at_ms: u64,
    reason: &str,
) -> SceneDetection {
    SceneDetection {
        game_id: game_id.to_owned(),
        scene_id: "unknown".to_owned(),
        confidence: 0.0,
        detected_at_ms,
        frame_number,
        evidence: vec![DetectorEvidence {
            detector_type: "stability".to_owned(),
            matched: false,
            confidence: 0.0,
            observed: 0.0,
            expected: "configured scene".to_owned(),
            region: [0, 0, 0, 0],
            detail: reason.to_owned(),
        }],
        consecutive_frames: 0,
        candidate_scene_id: None,
        candidate_consecutive_frames: 0,
    }
}

fn validate_game_id(game_id: &str) -> Result<(), String> {
    if game_id.is_empty()
        || !game_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(format!("Invalid game id {game_id:?}"));
    }
    Ok(())
}

fn validate_game_config(config: &GameConfig) -> Result<(), String> {
    validate_game_id(&config.id)?;
    if config.name.trim().is_empty() {
        return Err("Game name must not be empty".to_owned());
    }
    if config.resolution[0] == 0 || config.resolution[1] == 0 {
        return Err("Game resolution must be greater than zero".to_owned());
    }
    validate_stability(&config.defaults, "game defaults")
}

fn validate_stability(stability: &StabilityConfig, context: &str) -> Result<(), String> {
    if stability.consecutive_frames == 0 || stability.timeout_ms == 0 {
        return Err(format!(
            "{context} stability values must be greater than zero"
        ));
    }
    Ok(())
}

fn validate_region([x, y, width, height]: [u32; 4], resolution: [u32; 2]) -> Result<(), String> {
    if width == 0
        || height == 0
        || x.checked_add(width)
            .is_none_or(|right| right > resolution[0])
        || y.checked_add(height)
            .is_none_or(|bottom| bottom > resolution[1])
    {
        return Err(format!(
            "Region [{x}, {y}, {width}, {height}] is outside {} x {}",
            resolution[0], resolution[1]
        ));
    }
    Ok(())
}

fn validate_range(value: f64, min: f64, max: f64, name: &str) -> Result<(), String> {
    if !value.is_finite() || value < min || value > max {
        return Err(format!("{name} must be between {min} and {max}"));
    }
    Ok(())
}

fn rgb_to_gray([red, green, blue]: [u8; 3]) -> u8 {
    ((u32::from(red) * 77 + u32::from(green) * 150 + u32::from(blue) * 29) >> 8) as u8
}

const fn default_confirmation_frames() -> u32 {
    3
}

const fn default_timeout_ms() -> u64 {
    2_000
}

#[cfg(test)]
mod tests {
    use image::Rgb;

    use super::*;

    fn profile() -> Arc<GameProfile> {
        Arc::new(load_default_profile(&default_games_root()).expect("sample profile should load"))
    }

    fn candidate(profile: &GameProfile, scene_id: &str) -> Candidate {
        assert!(profile.scene(scene_id).is_some());
        Candidate {
            scene_id: scene_id.to_owned(),
            confidence: 0.9,
            evidence: vec![evidence(
                "fixture",
                true,
                0.9,
                0.9,
                ">= 0.8".to_owned(),
                [0, 0, 1, 1],
                "fixture matched".to_owned(),
            )],
        }
    }

    #[test]
    fn loads_three_scenes_from_the_sample_game_directory() {
        let summary = profile().summary();
        assert_eq!(summary.game_id, "sample-switch-game");
        assert_eq!(summary.resolution, [1280, 720]);
        assert_eq!(
            summary
                .scenes
                .iter()
                .map(|scene| scene.id.as_str())
                .collect::<Vec<_>>(),
            ["gameplay", "loading", "result"]
        );
        assert_eq!(summary.scenes[2].detector_count, 2);
    }

    #[test]
    fn configured_detector_returns_reproducible_evidence() {
        let profile = profile();
        let mut image = RgbImage::from_pixel(1280, 720, Rgb([96, 96, 96]));
        for y in 300..420 {
            for x in 480..800 {
                image.put_pixel(x, y, Rgb([0, 255, 0]));
            }
        }
        let detected = profile.classify(&image).unwrap().unwrap();
        assert_eq!(detected.scene_id, "gameplay");
        assert_eq!(detected.evidence[0].detector_type, "color_ratio");
        assert!(detected.evidence[0].matched);

        let loading = RgbImage::from_pixel(1280, 720, Rgb([0, 0, 0]));
        assert_eq!(
            profile.classify(&loading).unwrap().unwrap().scene_id,
            "loading"
        );

        let mut result = RgbImage::from_pixel(1280, 720, Rgb([96, 96, 96]));
        for (offset_y, row) in [[0_u8, 255, 0, 255], [255, 0, 255, 0]]
            .into_iter()
            .cycle()
            .take(4)
            .enumerate()
        {
            for (offset_x, gray) in row.into_iter().enumerate() {
                result.put_pixel(
                    600 + offset_x as u32,
                    60 + offset_y as u32,
                    Rgb([gray, gray, gray]),
                );
            }
        }
        let detected = profile.classify(&result).unwrap().unwrap();
        assert_eq!(detected.scene_id, "result");
        assert_eq!(detected.evidence.len(), 2);
        assert!(detected.evidence.iter().all(|item| item.matched));
    }

    #[test]
    fn no_matching_scene_remains_unknown() {
        let image = RgbImage::from_pixel(1280, 720, Rgb([96, 96, 96]));
        assert!(profile().classify(&image).unwrap().is_none());
    }

    #[test]
    fn consecutive_frames_noise_and_timeout_are_stable() {
        let profile = profile();
        let gameplay = candidate(&profile, "gameplay");
        let mut detector = SceneDetector::new(profile);
        detector.observe_candidate(Some(gameplay.clone()), 1, 1_001, 100);
        detector.observe_candidate(None, 2, 1_002, 200);
        detector.observe_candidate(Some(gameplay.clone()), 3, 1_003, 300);
        detector.observe_candidate(Some(gameplay.clone()), 4, 1_004, 400);
        assert_eq!(detector.snapshot().scene_id, "unknown");
        let confirmed = detector
            .observe_candidate(Some(gameplay), 5, 1_005, 500)
            .expect("three consecutive frames should confirm");
        assert_eq!(confirmed.detection.scene_id, "gameplay");
        assert_eq!(confirmed.detection.evidence[0].detector_type, "fixture");

        assert!(detector.observe_unavailable(6, 1_400, 600).is_none());
        let expired = detector
            .advance_time(6, 3_400, 2_600)
            .expect("configured timeout should expire the scene");
        assert_eq!(expired.from_scene_id, "gameplay");
        assert_eq!(expired.detection.scene_id, "unknown");
        assert!(expired.reason.contains("timeout"));
    }

    #[test]
    fn unsafe_game_ids_are_rejected() {
        assert!(GameProfile::load(&default_games_root(), "../sample-switch-game").is_err());
    }
}
