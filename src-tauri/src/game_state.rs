use serde::Serialize;

const DEFAULT_CONFIRMATION_FRAMES: u32 = 3;
const DEFAULT_TIMEOUT_MS: u64 = 2_000;
const DEFAULT_LOADING_LUMA_MAX: f64 = 24.0;
const DEFAULT_GAMEPLAY_COLOR_RATIO_MIN: f64 = 0.20;
const DEFAULT_RESULT_TEMPLATE_SCORE_MIN: f64 = 0.90;

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GameStateProfile {
    pub name: &'static str,
    pub confirmation_frames: u32,
    pub timeout_ms: u64,
    pub loading_luma_max: u8,
    pub gameplay_color_ratio_min_percent: u8,
    pub result_template_score_min_percent: u8,
}

impl Default for GameStateProfile {
    fn default() -> Self {
        Self {
            name: "generic-switch-game-v1",
            confirmation_frames: DEFAULT_CONFIRMATION_FRAMES,
            timeout_ms: DEFAULT_TIMEOUT_MS,
            loading_luma_max: DEFAULT_LOADING_LUMA_MAX as u8,
            gameplay_color_ratio_min_percent: (DEFAULT_GAMEPLAY_COLOR_RATIO_MIN * 100.0) as u8,
            result_template_score_min_percent: (DEFAULT_RESULT_TEMPLATE_SCORE_MIN * 100.0) as u8,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum GameState {
    #[default]
    Unknown,
    Loading,
    Gameplay,
    Result,
}

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GameStateSnapshot {
    pub state: GameState,
    pub confidence: f64,
    pub detected_at_ms: u64,
    pub frame_number: u64,
    pub reason: String,
    pub consecutive_frames: u32,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GameStateTransition {
    pub from: GameState,
    pub to: GameState,
    pub confidence: f64,
    pub detected_at_ms: u64,
    pub frame_number: u64,
    pub reason: String,
}

#[derive(Clone, Copy, Debug)]
pub struct GameSignals {
    pub average_rgb: [u8; 3],
    pub target_color_ratio: f64,
    pub template_score: Option<f64>,
}

#[derive(Clone, Debug)]
struct Candidate {
    state: GameState,
    confidence: f64,
    reason: String,
}

#[derive(Default)]
pub struct GameStateDetector {
    snapshot: GameStateSnapshot,
    pending: Option<Candidate>,
    pending_frames: u32,
    unstable_since_ms: Option<u64>,
}

impl GameStateDetector {
    pub fn snapshot(&self) -> GameStateSnapshot {
        self.snapshot.clone()
    }

    pub fn observe(
        &mut self,
        signals: GameSignals,
        frame_number: u64,
        detected_at_ms: u64,
        elapsed_ms: u64,
    ) -> Option<GameStateTransition> {
        self.observe_candidate(classify(signals), frame_number, detected_at_ms, elapsed_ms)
    }

    pub fn observe_unavailable(
        &mut self,
        frame_number: u64,
        detected_at_ms: u64,
        elapsed_ms: u64,
    ) -> Option<GameStateTransition> {
        self.observe_candidate(None, frame_number, detected_at_ms, elapsed_ms)
    }

    pub fn advance_time(
        &mut self,
        frame_number: u64,
        detected_at_ms: u64,
        elapsed_ms: u64,
    ) -> Option<GameStateTransition> {
        self.timeout_if_unstable(frame_number, detected_at_ms, elapsed_ms)
    }

    fn observe_candidate(
        &mut self,
        candidate: Option<Candidate>,
        frame_number: u64,
        detected_at_ms: u64,
        elapsed_ms: u64,
    ) -> Option<GameStateTransition> {
        match candidate {
            Some(candidate) => {
                if self.pending.as_ref().map(|item| item.state) == Some(candidate.state) {
                    self.pending_frames = self.pending_frames.saturating_add(1);
                } else {
                    self.pending_frames = 1;
                }
                self.pending = Some(candidate.clone());

                if candidate.state == self.snapshot.state {
                    self.unstable_since_ms = None;
                    self.snapshot =
                        snapshot_from(candidate, frame_number, detected_at_ms, self.pending_frames);
                    return None;
                }

                if self.pending_frames >= DEFAULT_CONFIRMATION_FRAMES {
                    self.unstable_since_ms = None;
                    return self.transition(
                        candidate,
                        frame_number,
                        detected_at_ms,
                        self.pending_frames,
                    );
                }

                self.timeout_if_unstable(frame_number, detected_at_ms, elapsed_ms)
            }
            None => {
                self.pending = None;
                self.pending_frames = 0;
                self.timeout_if_unstable(frame_number, detected_at_ms, elapsed_ms)
            }
        }
    }

    fn timeout_if_unstable(
        &mut self,
        frame_number: u64,
        detected_at_ms: u64,
        elapsed_ms: u64,
    ) -> Option<GameStateTransition> {
        if self.snapshot.state == GameState::Unknown {
            self.unstable_since_ms = None;
            return None;
        }

        let unstable_since = *self.unstable_since_ms.get_or_insert(elapsed_ms);
        let unstable_for_ms = elapsed_ms.saturating_sub(unstable_since);
        if unstable_for_ms < DEFAULT_TIMEOUT_MS {
            return None;
        }

        self.unstable_since_ms = None;
        self.transition(
            Candidate {
                state: GameState::Unknown,
                confidence: 0.0,
                reason: format!(
                    "stable state not observed for {unstable_for_ms} ms (timeout {DEFAULT_TIMEOUT_MS} ms)"
                ),
            },
            frame_number,
            detected_at_ms,
            0,
        )
    }

    fn transition(
        &mut self,
        candidate: Candidate,
        frame_number: u64,
        detected_at_ms: u64,
        consecutive_frames: u32,
    ) -> Option<GameStateTransition> {
        let from = self.snapshot.state;
        self.snapshot = snapshot_from(candidate, frame_number, detected_at_ms, consecutive_frames);
        Some(GameStateTransition {
            from,
            to: self.snapshot.state,
            confidence: self.snapshot.confidence,
            detected_at_ms,
            frame_number,
            reason: self.snapshot.reason.clone(),
        })
    }
}

fn classify(signals: GameSignals) -> Option<Candidate> {
    if let Some(score) = signals
        .template_score
        .filter(|score| *score >= DEFAULT_RESULT_TEMPLATE_SCORE_MIN)
    {
        return Some(Candidate {
            state: GameState::Result,
            confidence: score.clamp(0.0, 1.0),
            reason: format!(
                "result template score {score:.3} >= {DEFAULT_RESULT_TEMPLATE_SCORE_MIN:.2}"
            ),
        });
    }

    let luma = rgb_luma(signals.average_rgb);
    if luma <= DEFAULT_LOADING_LUMA_MAX {
        return Some(Candidate {
            state: GameState::Loading,
            confidence: (1.0 - luma / (DEFAULT_LOADING_LUMA_MAX + 1.0)).clamp(0.0, 1.0),
            reason: format!("ROI luma {luma:.1} <= {DEFAULT_LOADING_LUMA_MAX:.1}"),
        });
    }

    if signals.target_color_ratio >= DEFAULT_GAMEPLAY_COLOR_RATIO_MIN {
        return Some(Candidate {
            state: GameState::Gameplay,
            confidence: signals.target_color_ratio.clamp(0.0, 1.0),
            reason: format!(
                "target color ratio {:.3} >= {DEFAULT_GAMEPLAY_COLOR_RATIO_MIN:.2}",
                signals.target_color_ratio
            ),
        });
    }

    None
}

fn snapshot_from(
    candidate: Candidate,
    frame_number: u64,
    detected_at_ms: u64,
    consecutive_frames: u32,
) -> GameStateSnapshot {
    GameStateSnapshot {
        state: candidate.state,
        confidence: candidate.confidence,
        detected_at_ms,
        frame_number,
        reason: candidate.reason,
        consecutive_frames,
    }
}

fn rgb_luma([red, green, blue]: [u8; 3]) -> f64 {
    (f64::from(red) * 0.299) + (f64::from(green) * 0.587) + (f64::from(blue) * 0.114)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signals(
        average_rgb: [u8; 3],
        target_color_ratio: f64,
        template_score: Option<f64>,
    ) -> GameSignals {
        GameSignals {
            average_rgb,
            target_color_ratio,
            template_score,
        }
    }

    fn observe_three(detector: &mut GameStateDetector, signals: GameSignals) {
        for frame in 1..=3 {
            detector.observe(signals, frame, 1_000 + frame, frame * 100);
        }
    }

    #[test]
    fn requires_three_consecutive_frames_before_transition() {
        let mut detector = GameStateDetector::default();
        let gameplay = signals([80, 120, 80], 0.7, None);

        assert!(detector.observe(gameplay, 1, 1_001, 100).is_none());
        assert!(detector.observe(gameplay, 2, 1_002, 200).is_none());
        assert_eq!(detector.snapshot().state, GameState::Unknown);

        let transition = detector
            .observe(gameplay, 3, 1_003, 300)
            .expect("third matching frame should confirm gameplay");
        assert_eq!(transition.from, GameState::Unknown);
        assert_eq!(transition.to, GameState::Gameplay);
        assert_eq!(detector.snapshot().consecutive_frames, 3);
    }

    #[test]
    fn a_single_noise_frame_resets_confirmation() {
        let mut detector = GameStateDetector::default();
        let gameplay = signals([80, 120, 80], 0.7, None);
        let unmatched = signals([90, 90, 90], 0.0, None);

        detector.observe(gameplay, 1, 1_001, 100);
        detector.observe(unmatched, 2, 1_002, 200);
        detector.observe(gameplay, 3, 1_003, 300);
        detector.observe(gameplay, 4, 1_004, 400);

        assert_eq!(detector.snapshot().state, GameState::Unknown);
        assert!(detector.observe(gameplay, 5, 1_005, 500).is_some());
    }

    #[test]
    fn template_has_priority_over_color_and_darkness() {
        let mut detector = GameStateDetector::default();
        observe_three(&mut detector, signals([0, 0, 0], 1.0, Some(0.95)));

        assert_eq!(detector.snapshot().state, GameState::Result);
        assert_eq!(detector.snapshot().confidence, 0.95);
    }

    #[test]
    fn unmatched_frames_timeout_the_last_stable_state() {
        let mut detector = GameStateDetector::default();
        observe_three(&mut detector, signals([100, 120, 100], 0.7, None));
        let unmatched = signals([90, 90, 90], 0.0, None);

        detector.observe(unmatched, 4, 1_004, 400);
        assert!(detector.observe(unmatched, 5, 2_903, 2_300).is_none());
        let transition = detector
            .observe(unmatched, 6, 3_004, 2_400)
            .expect("two seconds without a match should time out");

        assert_eq!(transition.from, GameState::Gameplay);
        assert_eq!(transition.to, GameState::Unknown);
        assert!(transition.reason.contains("timeout"));
    }

    #[test]
    fn alternating_unconfirmed_states_timeout_the_last_stable_state() {
        let mut detector = GameStateDetector::default();
        observe_three(&mut detector, signals([100, 120, 100], 0.7, None));

        assert!(detector
            .observe(signals([0, 0, 0], 0.0, None), 4, 1_400, 400)
            .is_none());
        assert!(detector
            .observe(signals([120, 120, 120], 0.0, Some(0.96)), 5, 2_300, 1_300)
            .is_none());
        let transition = detector
            .observe(signals([0, 0, 0], 0.0, None), 6, 3_400, 2_400)
            .expect("unstable candidates must not preserve actionable gameplay forever");

        assert_eq!(transition.from, GameState::Gameplay);
        assert_eq!(transition.to, GameState::Unknown);
    }

    #[test]
    fn unavailable_analysis_counts_toward_the_timeout() {
        let mut detector = GameStateDetector::default();
        observe_three(&mut detector, signals([100, 120, 100], 0.7, None));

        assert!(detector.observe_unavailable(4, 1_400, 400).is_none());
        let transition = detector
            .observe_unavailable(5, 3_400, 2_400)
            .expect("unavailable analysis must invalidate the last stable state");

        assert_eq!(transition.from, GameState::Gameplay);
        assert_eq!(transition.to, GameState::Unknown);
    }

    #[test]
    fn waiting_for_a_frame_does_not_break_frame_confirmation() {
        let mut detector = GameStateDetector::default();
        let gameplay = signals([100, 120, 100], 0.7, None);

        detector.observe(gameplay, 1, 1_100, 100);
        detector.advance_time(1, 1_350, 350);
        detector.observe(gameplay, 2, 1_600, 600);
        detector.advance_time(2, 1_850, 850);
        let transition = detector
            .observe(gameplay, 3, 2_100, 1_100)
            .expect("only analyzed frames should determine the confirmation streak");

        assert_eq!(transition.to, GameState::Gameplay);
        assert_eq!(detector.snapshot().consecutive_frames, 3);
    }

    #[test]
    fn fixture_samples_are_classified_without_false_positives() {
        let cases = [
            (signals([0, 0, 0], 0.0, None), GameState::Loading),
            (signals([80, 130, 75], 0.65, None), GameState::Gameplay),
            (signals([120, 120, 120], 0.0, Some(0.96)), GameState::Result),
        ];

        for (case, expected) in cases {
            let mut detector = GameStateDetector::default();
            observe_three(&mut detector, case);
            assert_eq!(detector.snapshot().state, expected);
        }

        for negative in [
            signals([35, 35, 35], 0.19, Some(0.89)),
            signals([100, 100, 100], 0.0, None),
            signals([255, 255, 255], 0.0, Some(0.50)),
        ] {
            let mut detector = GameStateDetector::default();
            observe_three(&mut detector, negative);
            assert_eq!(detector.snapshot().state, GameState::Unknown);
        }
    }
}
