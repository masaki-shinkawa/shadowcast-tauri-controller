use std::{
    collections::{HashMap, HashSet},
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, MutexGuard,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::json;
use serialport::{ClearBuffer, SerialPort};
use tauri::State;
use tracing::{error, info, warn};

use crate::analysis::{AnalysisManager, SceneStatusReader};
use crate::diagnostics::{AutomationRunLog, RunMetadata};

const NEUTRAL_STATE: &str = "000000000880000880";
const SCENE_POLL_INTERVAL: Duration = Duration::from_millis(50);
const STOP_POLL_INTERVAL: Duration = Duration::from_millis(20);
const MAX_INPUT_LOGS: usize = 100;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ScenarioConfig {
    id: String,
    name: String,
    game_id: String,
    entry_step_id: String,
    max_runtime_ms: Option<u64>,
    controller: ControllerConfig,
    steps: Vec<ScenarioStep>,
    failure_action: FailureAction,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ControllerConfig {
    port: String,
    baud_rate: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ScenarioStep {
    id: String,
    scene_id: String,
    inputs: Vec<ScenarioInput>,
    wait_after_ms: u64,
    expected_scene_id: String,
    timeout_ms: u64,
    retries: u32,
    #[serde(default, deserialize_with = "deserialize_next_step")]
    next_step_id: NextStep,
    #[serde(rename = "notes")]
    _notes: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ScenarioInput {
    #[serde(rename = "type")]
    input_type: InputType,
    button: ControllerButton,
    hold_ms: u64,
    #[serde(default)]
    wait_after_ms: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum InputType {
    Tap,
    Hold,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum ControllerButton {
    A,
    B,
    X,
    Y,
    Up,
    Down,
    Left,
    Right,
    L,
    R,
    Zl,
    Zr,
    Plus,
    Minus,
    LStick,
    RStick,
}

impl ControllerButton {
    fn state(self) -> &'static str {
        match self {
            Self::A => "080000000880000880",
            Self::B => "040000000880000880",
            Self::X => "020000000880000880",
            Self::Y => "010000000880000880",
            Self::Up => "000002000880000880",
            Self::Down => "000001000880000880",
            Self::Left => "000008000880000880",
            Self::Right => "000004000880000880",
            Self::L => "000040000880000880",
            Self::R => "400000000880000880",
            Self::Zl => "000080000880000880",
            Self::Zr => "800000000880000880",
            Self::Minus => "000100000880000880",
            Self::Plus => "000200000880000880",
            Self::LStick => "000800000880000880",
            Self::RStick => "000400000880000880",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum FailureAction {
    StopAndNeutralize,
}

#[derive(Clone, Debug, Default, Serialize, PartialEq, Eq)]
enum NextStep {
    #[default]
    Default,
    Finish,
    Id(String),
}

fn deserialize_next_step<'de, D>(deserializer: D) -> Result<NextStep, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(match Option::<String>::deserialize(deserializer)? {
        Some(id) => NextStep::Id(id),
        None => NextStep::Finish,
    })
}

#[derive(Clone, Copy, Debug, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ScenarioState {
    #[default]
    Idle,
    Running,
    Stopping,
    Stopped,
    Completed,
    Error,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScenarioStatus {
    state: ScenarioState,
    game_id: Option<String>,
    scenario_id: Option<String>,
    scenario_name: Option<String>,
    current_step_id: Option<String>,
    current_attempt: Option<u32>,
    last_scene_id: Option<String>,
    controller_port: Option<String>,
    completed_steps: u64,
    started_at_ms: Option<u64>,
    input_logs: Vec<ScenarioInputLog>,
    run_id: Option<String>,
    resumed_from_run_id: Option<String>,
    log_directory: Option<String>,
    evidence_path: Option<String>,
    resume_candidates: Vec<ScenarioResumeCandidate>,
    error: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ScenarioInputLog {
    at_ms: u64,
    step_id: String,
    input_type: InputType,
    button: ControllerButton,
    hold_ms: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ScenarioResumeCandidate {
    step_id: String,
    scene_id: String,
}

struct ActiveScenario {
    stop: Arc<AtomicBool>,
    thread: JoinHandle<()>,
}

pub struct ScenarioManager {
    games_root: PathBuf,
    diagnostics_root: PathBuf,
    active: Mutex<Option<ActiveScenario>>,
    status: Arc<Mutex<ScenarioStatus>>,
}

impl ScenarioManager {
    pub fn from_games_root_with_diagnostics(
        games_root: impl AsRef<Path>,
        diagnostics_root: impl AsRef<Path>,
    ) -> Self {
        Self {
            games_root: games_root.as_ref().to_path_buf(),
            diagnostics_root: diagnostics_root.as_ref().to_path_buf(),
            active: Mutex::new(None),
            status: Arc::new(Mutex::new(ScenarioStatus::default())),
        }
    }

    fn status(&self) -> ScenarioStatus {
        lock(&self.status).clone()
    }

    fn status_with_resume_candidates(&self, analysis: &AnalysisManager) -> ScenarioStatus {
        let mut status = self.status();
        status.resume_candidates.clear();
        if status.state != ScenarioState::Error {
            return status;
        }
        let (Some(game_id), Some(scenario_id)) =
            (status.game_id.as_deref(), status.scenario_id.as_deref())
        else {
            return status;
        };
        let scene = analysis.scene_status_reader().snapshot();
        if !scene.running || scene.scene_id == "unknown" || scene.game_id != game_id {
            return status;
        }
        if let Ok(config) = load_scenario(&self.games_root, game_id, scenario_id) {
            status.resume_candidates = resume_candidates(&config, &scene.scene_id);
        }
        status
    }

    fn start(
        &self,
        analysis: &AnalysisManager,
        game_id: &str,
        scenario_id: &str,
    ) -> Result<ScenarioStatus, String> {
        self.start_from(analysis, game_id, scenario_id, None, None)
    }

    fn resume(
        &self,
        analysis: &AnalysisManager,
        game_id: &str,
        scenario_id: &str,
        step_id: &str,
    ) -> Result<ScenarioStatus, String> {
        let previous = self.status();
        if previous.state != ScenarioState::Error {
            return Err("Only a scenario in the error state can be resumed".to_owned());
        }
        if previous.game_id.as_deref() != Some(game_id)
            || previous.scenario_id.as_deref() != Some(scenario_id)
        {
            return Err("Resume target does not match the failed scenario".to_owned());
        }
        let resumed_from_run_id = previous
            .run_id
            .clone()
            .ok_or_else(|| "Failed scenario has no run id".to_owned())?;
        self.start_from(
            analysis,
            game_id,
            scenario_id,
            Some(step_id),
            Some(resumed_from_run_id),
        )
    }

    fn start_from(
        &self,
        analysis: &AnalysisManager,
        game_id: &str,
        scenario_id: &str,
        requested_step_id: Option<&str>,
        resumed_from_run_id: Option<String>,
    ) -> Result<ScenarioStatus, String> {
        let mut active = lock(&self.active);
        if active
            .as_ref()
            .is_some_and(|scenario| !scenario.thread.is_finished())
        {
            return Err("A scenario is already running".to_owned());
        }
        if let Some(finished) = active.take() {
            let _ = finished.thread.join();
        }

        let config = Arc::new(load_scenario(&self.games_root, game_id, scenario_id)?);
        let start_step_id = requested_step_id.unwrap_or(&config.entry_step_id);
        let first_step = config
            .steps
            .iter()
            .find(|step| step.id == start_step_id)
            .ok_or_else(|| format!("Resume step {start_step_id:?} does not exist"))?;
        let scene_reader = analysis.scene_status_reader();
        let scene = scene_reader.snapshot();
        if !scene.running {
            return Err("Start capture and wait for scene detection before automation".to_owned());
        }
        if scene.game_id != config.game_id {
            return Err(format!(
                "Loaded game is {:?}; scenario requires {:?}",
                scene.game_id, config.game_id
            ));
        }
        if scene.scene_id != first_step.scene_id {
            return Err(format!(
                "Start step {:?} requires scene {:?}; detected {:?}",
                first_step.id, first_step.scene_id, scene.scene_id
            ));
        }
        let worker_start_step_id = first_step.id.clone();

        let controller = Controller::connect(&config.controller)?;
        let mut run_log = AutomationRunLog::start(
            &self.diagnostics_root,
            &self.games_root,
            RunMetadata {
                game_id: config.game_id.clone(),
                scenario_id: config.id.clone(),
                scenario_name: config.name.clone(),
                controller_port: config.controller.port.clone(),
                start_step_id: first_step.id.clone(),
                resumed_from_run_id: resumed_from_run_id.clone(),
            },
        )?;
        let run_id = run_log.run_id().to_owned();
        let log_directory = run_log.directory().to_string_lossy().into_owned();
        let evidence_path = run_log
            .directory()
            .join("error-evidence.json")
            .to_string_lossy()
            .into_owned();
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let thread_status = Arc::clone(&self.status);
        let started_at_ms = unix_time_ms();
        let initial_status = ScenarioStatus {
            state: ScenarioState::Running,
            game_id: Some(config.game_id.clone()),
            scenario_id: Some(config.id.clone()),
            scenario_name: Some(config.name.clone()),
            current_step_id: Some(first_step.id.clone()),
            current_attempt: None,
            last_scene_id: Some(scene.scene_id),
            controller_port: Some(config.controller.port.clone()),
            completed_steps: 0,
            started_at_ms: Some(started_at_ms),
            input_logs: Vec::new(),
            run_id: Some(run_id),
            resumed_from_run_id,
            log_directory: Some(log_directory),
            evidence_path: Some(evidence_path),
            resume_candidates: Vec::new(),
            error: None,
        };
        run_log.record(
            "run_started",
            json!({
                "automationSnapshot": {
                    "sceneDetection": scene.detection,
                    "scenarioStatus": &initial_status,
                },
                "scenario": config.as_ref(),
            }),
            scene_reader.latest_frame().as_ref(),
        )?;
        update_status(&self.status, |status| *status = initial_status);

        let worker = thread::Builder::new()
            .name("shadowcast-scenario".to_owned())
            .spawn(move || {
                let panic_status = Arc::clone(&thread_status);
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    run_scenario(
                        config,
                        controller,
                        scene_reader,
                        thread_stop,
                        thread_status,
                        worker_start_step_id,
                        run_log,
                    )
                }));
                if result.is_err() {
                    update_status(&panic_status, |status| {
                        status.state = ScenarioState::Error;
                        status.current_attempt = None;
                        status.error = Some("Scenario worker panicked".to_owned());
                    });
                }
            })
            .map_err(|error| {
                let message = format!("Failed to spawn scenario worker: {error}");
                update_status(&self.status, |status| {
                    status.state = ScenarioState::Error;
                    status.error = Some(message.clone());
                });
                message
            })?;
        *active = Some(ActiveScenario {
            stop,
            thread: worker,
        });
        Ok(self.status())
    }

    pub(crate) fn stop(&self) -> ScenarioStatus {
        let mut active = lock(&self.active);
        if let Some(active_scenario) = active.take() {
            update_status(&self.status, |status| {
                if status.state == ScenarioState::Running {
                    status.state = ScenarioState::Stopping;
                }
            });
            active_scenario.stop.store(true, Ordering::Release);
            if active_scenario.thread.join().is_err() {
                update_status(&self.status, |status| {
                    status.state = ScenarioState::Error;
                    status.error = Some("Scenario worker panicked while stopping".to_owned());
                });
            }
        }
        self.status()
    }
}

fn resume_candidates(config: &ScenarioConfig, scene_id: &str) -> Vec<ScenarioResumeCandidate> {
    config
        .steps
        .iter()
        .filter(|step| step.scene_id == scene_id)
        .map(|step| ScenarioResumeCandidate {
            step_id: step.id.clone(),
            scene_id: step.scene_id.clone(),
        })
        .collect()
}

impl Drop for ScenarioManager {
    fn drop(&mut self) {
        if let Ok(active) = self.active.get_mut() {
            if let Some(active) = active.take() {
                active.stop.store(true, Ordering::Release);
                let _ = active.thread.join();
            }
        }
    }
}

struct Controller {
    port: Box<dyn SerialPort>,
    neutralized: bool,
}

impl Controller {
    fn connect(config: &ControllerConfig) -> Result<Self, String> {
        let mut port = serialport::new(&config.port, config.baud_rate)
            .timeout(Duration::from_millis(50))
            .open()
            .map_err(|error| format!("Failed to open {}: {error}", config.port))?;
        port.write_data_terminal_ready(true)
            .map_err(|error| format!("Failed to enable DTR on {}: {error}", config.port))?;
        thread::sleep(Duration::from_millis(200));
        port.clear(ClearBuffer::Input)
            .map_err(|error| format!("Failed to clear {} input: {error}", config.port))?;

        let mut controller = Self {
            port,
            neutralized: false,
        };
        let identity = controller.query("+ID ", Duration::from_millis(500))?;
        if !identity.lines().any(|line| line.trim() == "+2wiCC") {
            return Err(format!("Controller identity check failed: {identity:?}"));
        }
        let connection = controller.query("+GCS ", Duration::from_millis(500))?;
        if !connection.lines().any(|line| line.trim() == "+GCS 1") {
            return Err(format!("Controller USB is not connected: {connection:?}"));
        }
        controller.write_line("+SPM RT")?;
        controller.neutralize()?;
        info!(port = %config.port, "controller verified and neutralized");
        Ok(controller)
    }

    fn press(&mut self, button: ControllerButton) -> Result<(), String> {
        self.write_state(button.state())?;
        self.neutralized = false;
        Ok(())
    }

    fn neutralize(&mut self) -> Result<(), String> {
        self.write_state(NEUTRAL_STATE)?;
        self.neutralized = true;
        Ok(())
    }

    fn write_state(&mut self, state: &str) -> Result<(), String> {
        self.write_line(&format!("+QF {state}"))
    }

    fn write_line(&mut self, command: &str) -> Result<(), String> {
        self.port
            .write_all(format!("{command}\n").as_bytes())
            .and_then(|()| self.port.flush())
            .map_err(|error| format!("Controller write failed: {error}"))
    }

    fn query(&mut self, command: &str, duration: Duration) -> Result<String, String> {
        self.write_line(command)?;
        let deadline = Instant::now() + duration;
        let mut reply = Vec::new();
        let mut buffer = [0_u8; 256];
        while Instant::now() < deadline {
            match self.port.read(&mut buffer) {
                Ok(count) => reply.extend_from_slice(&buffer[..count]),
                Err(error) if error.kind() == std::io::ErrorKind::TimedOut => {}
                Err(error) => return Err(format!("Controller read failed: {error}")),
            }
        }
        Ok(String::from_utf8_lossy(&reply).into_owned())
    }
}

impl Drop for Controller {
    fn drop(&mut self) {
        if !self.neutralized {
            if let Err(error) = self.neutralize() {
                error!(%error, "failed to neutralize controller during drop");
            }
        }
    }
}

fn run_scenario(
    config: Arc<ScenarioConfig>,
    mut controller: Controller,
    scene_reader: SceneStatusReader,
    stop: Arc<AtomicBool>,
    status: Arc<Mutex<ScenarioStatus>>,
    start_step_id: String,
    mut run_log: AutomationRunLog,
) {
    let result = execute_scenario(
        &config,
        &mut controller,
        &scene_reader,
        &stop,
        &status,
        &start_step_id,
        &mut run_log,
    );
    if let Err(error) = controller.neutralize() {
        warn!(%error, "final controller neutralization failed");
    }
    match result {
        Ok(ExecutionEnd::Stopped) => {
            update_status(&status, |current| {
                current.state = ScenarioState::Stopped;
                current.current_attempt = None;
                current.error = None;
            });
            let _ = record_snapshot_event(
                &mut run_log,
                "run_stopped",
                &status,
                &scene_reader,
                json!({}),
                false,
            );
            let _ = run_log.finish("stopped", None);
        }
        Ok(ExecutionEnd::Completed) => {
            update_status(&status, |current| {
                current.state = ScenarioState::Completed;
                current.current_step_id = None;
                current.current_attempt = None;
                current.error = None;
            });
            let _ = record_snapshot_event(
                &mut run_log,
                "run_completed",
                &status,
                &scene_reader,
                json!({}),
                false,
            );
            let _ = run_log.finish("completed", None);
        }
        Err(message) => {
            error!(error = %message, "scenario failed");
            update_status(&status, |current| {
                current.state = ScenarioState::Error;
                current.current_attempt = None;
                current.error = Some(message);
            });
            let error = lock(&status).error.clone();
            let _ = record_snapshot_event(
                &mut run_log,
                "automation_error",
                &status,
                &scene_reader,
                json!({ "error": error }),
                true,
            );
            let _ = run_log.finish("error", error);
        }
    }
}

enum ExecutionEnd {
    Stopped,
    Completed,
}

fn execute_scenario(
    config: &ScenarioConfig,
    controller: &mut Controller,
    scene_reader: &SceneStatusReader,
    stop: &AtomicBool,
    status: &Arc<Mutex<ScenarioStatus>>,
    start_step_id: &str,
    run_log: &mut AutomationRunLog,
) -> Result<ExecutionEnd, String> {
    let step_indexes = config
        .steps
        .iter()
        .enumerate()
        .map(|(index, step)| (step.id.as_str(), index))
        .collect::<HashMap<_, _>>();
    let started = Instant::now();
    let mut step_index = *step_indexes
        .get(start_step_id)
        .expect("validated start step");

    loop {
        if stop.load(Ordering::Acquire) {
            return Ok(ExecutionEnd::Stopped);
        }
        if config
            .max_runtime_ms
            .is_some_and(|limit| started.elapsed() >= Duration::from_millis(limit))
        {
            return Err("Scenario exceeded max_runtime_ms".to_owned());
        }
        let step = &config.steps[step_index];
        let before = scene_reader.snapshot();
        update_status(status, |current| {
            current.current_step_id = Some(step.id.clone());
            current.current_attempt = None;
            current.last_scene_id = Some(before.scene_id.clone());
        });
        run_log.update_progress(Some(&step.id), lock(status).completed_steps);
        record_snapshot_event(
            run_log,
            "step_started",
            status,
            scene_reader,
            json!({
                "stepId": step.id,
                "requiredSceneId": step.scene_id,
                "expectedSceneId": step.expected_scene_id,
            }),
            true,
        )?;
        if !before.running {
            return Err("Capture or analysis stopped while scenario was running".to_owned());
        }
        if before.scene_id != step.scene_id {
            return Err(format!(
                "Step {:?} requires scene {:?}; detected {:?}",
                step.id, step.scene_id, before.scene_id
            ));
        }

        let mut transitioned = false;
        for attempt in 0..=step.retries {
            update_status(status, |current| current.current_attempt = Some(attempt));
            let current = scene_reader.snapshot();
            record_snapshot_event(
                run_log,
                "attempt_started",
                status,
                scene_reader,
                json!({ "stepId": step.id, "attempt": attempt }),
                false,
            )?;
            if attempt == 0 || current.scene_id == step.scene_id {
                info!(
                    step_id = %step.id,
                    scene_id = %step.scene_id,
                    attempt,
                    "executing scenario step inputs"
                );
                for input in &step.inputs {
                    record_snapshot_event(
                        run_log,
                        "before_input",
                        status,
                        scene_reader,
                        json!({
                            "stepId": step.id,
                            "attempt": attempt,
                            "input": input,
                        }),
                        true,
                    )?;
                    controller.press(input.button)?;
                    record_input(status, &step.id, input);
                    if interruptible_wait(Duration::from_millis(input.hold_ms), stop) {
                        return Ok(ExecutionEnd::Stopped);
                    }
                    controller.neutralize()?;
                    record_snapshot_event(
                        run_log,
                        "after_input",
                        status,
                        scene_reader,
                        json!({
                            "stepId": step.id,
                            "attempt": attempt,
                            "input": input,
                        }),
                        true,
                    )?;
                    if interruptible_wait(Duration::from_millis(input.wait_after_ms), stop) {
                        return Ok(ExecutionEnd::Stopped);
                    }
                }
                if interruptible_wait(Duration::from_millis(step.wait_after_ms), stop) {
                    return Ok(ExecutionEnd::Stopped);
                }
            }

            let mut wait_context = SceneWaitContext {
                status,
                step_id: &step.id,
                attempt,
                run_log,
            };
            match wait_for_scene(
                &step.expected_scene_id,
                Duration::from_millis(step.timeout_ms),
                scene_reader,
                stop,
                &mut wait_context,
            )? {
                SceneWait::Matched => {
                    transitioned = true;
                    break;
                }
                SceneWait::Stopped => return Ok(ExecutionEnd::Stopped),
                SceneWait::TimedOut => {
                    record_snapshot_event(
                        run_log,
                        "attempt_timed_out",
                        status,
                        scene_reader,
                        json!({
                            "stepId": step.id,
                            "attempt": attempt,
                            "expectedSceneId": step.expected_scene_id,
                        }),
                        false,
                    )?;
                }
            }
        }
        if !transitioned {
            let last_scene = scene_reader.snapshot().scene_id;
            return Err(format!(
                "Step {:?} exhausted {} retries waiting for scene {:?}; last detected {:?}",
                step.id, step.retries, step.expected_scene_id, last_scene
            ));
        }
        update_status(status, |current| {
            current.completed_steps += 1;
            current.current_attempt = None;
        });
        let completed_steps = lock(status).completed_steps;
        run_log.update_progress(Some(&step.id), completed_steps);
        record_snapshot_event(
            run_log,
            "step_completed",
            status,
            scene_reader,
            json!({ "stepId": step.id }),
            false,
        )?;

        step_index = match &step.next_step_id {
            NextStep::Default if step_index + 1 < config.steps.len() => step_index + 1,
            NextStep::Default | NextStep::Finish => return Ok(ExecutionEnd::Completed),
            NextStep::Id(id) => *step_indexes.get(id.as_str()).expect("validated next step"),
        };
    }
}

enum SceneWait {
    Matched,
    TimedOut,
    Stopped,
}

struct SceneWaitContext<'a> {
    status: &'a Arc<Mutex<ScenarioStatus>>,
    step_id: &'a str,
    attempt: u32,
    run_log: &'a mut AutomationRunLog,
}

fn wait_for_scene(
    expected_scene_id: &str,
    timeout: Duration,
    scene_reader: &SceneStatusReader,
    stop: &AtomicBool,
    context: &mut SceneWaitContext<'_>,
) -> Result<SceneWait, String> {
    let deadline = Instant::now() + timeout;
    let mut observed_scene_id = scene_reader.snapshot().scene_id;
    loop {
        if stop.load(Ordering::Acquire) {
            return Ok(SceneWait::Stopped);
        }
        let scene = scene_reader.snapshot();
        update_status(context.status, |current| {
            current.last_scene_id = Some(scene.scene_id.clone())
        });
        if scene.scene_id != observed_scene_id {
            record_snapshot_event(
                context.run_log,
                "scene_changed",
                context.status,
                scene_reader,
                json!({
                    "stepId": context.step_id,
                    "attempt": context.attempt,
                    "fromSceneId": observed_scene_id,
                    "toSceneId": scene.scene_id,
                }),
                true,
            )?;
            observed_scene_id = scene.scene_id.clone();
        }
        if !scene.running {
            return Err("Capture or analysis stopped while waiting for a scene".to_owned());
        }
        if scene.scene_id == expected_scene_id {
            return Ok(SceneWait::Matched);
        }
        if Instant::now() >= deadline {
            return Ok(SceneWait::TimedOut);
        }
        thread::sleep(SCENE_POLL_INTERVAL);
    }
}

fn record_snapshot_event(
    run_log: &mut AutomationRunLog,
    event_type: &str,
    status: &Arc<Mutex<ScenarioStatus>>,
    scene_reader: &SceneStatusReader,
    details: serde_json::Value,
    save_frame: bool,
) -> Result<(), String> {
    let scene = scene_reader.snapshot();
    let scenario_status = lock(status).clone();
    let frame = save_frame.then(|| scene_reader.latest_frame()).flatten();
    run_log.record(
        event_type,
        json!({
            "automationSnapshot": {
                "sceneDetection": scene.detection,
                "scenarioStatus": scenario_status,
            },
            "details": details,
        }),
        frame.as_ref(),
    )
}

fn interruptible_wait(duration: Duration, stop: &AtomicBool) -> bool {
    let deadline = Instant::now() + duration;
    while Instant::now() < deadline {
        if stop.load(Ordering::Acquire) {
            return true;
        }
        thread::sleep(STOP_POLL_INTERVAL.min(deadline.saturating_duration_since(Instant::now())));
    }
    stop.load(Ordering::Acquire)
}

fn load_scenario(
    games_root: &Path,
    game_id: &str,
    scenario_id: &str,
) -> Result<ScenarioConfig, String> {
    validate_safe_id(game_id, "game")?;
    validate_safe_id(scenario_id, "scenario")?;
    let game_dir = games_root.join(game_id);
    let path = game_dir
        .join("scenarios")
        .join(format!("{scenario_id}.yaml"));
    let canonical_game_dir = game_dir
        .canonicalize()
        .map_err(|error| format!("Failed to resolve {}: {error}", game_dir.display()))?;
    let canonical_path = path
        .canonicalize()
        .map_err(|error| format!("Failed to resolve {}: {error}", path.display()))?;
    if !canonical_path.starts_with(&canonical_game_dir) {
        return Err(format!(
            "Scenario {} is outside the game directory",
            path.display()
        ));
    }
    let contents = fs::read_to_string(&canonical_path)
        .map_err(|error| format!("Failed to read {}: {error}", path.display()))?;
    let config: ScenarioConfig = serde_yaml::from_str(&contents)
        .map_err(|error| format!("Failed to parse {}: {error}", path.display()))?;
    validate_scenario(&config, game_id, scenario_id)?;
    Ok(config)
}

fn validate_scenario(
    config: &ScenarioConfig,
    game_id: &str,
    scenario_id: &str,
) -> Result<(), String> {
    if config.id != scenario_id || config.game_id != game_id {
        return Err("Scenario id or game_id does not match its path".to_owned());
    }
    if config.name.trim().is_empty() || config.controller.port.trim().is_empty() {
        return Err("Scenario name and controller port must not be empty".to_owned());
    }
    if config.controller.baud_rate == 0 || config.steps.is_empty() {
        return Err("Controller baud rate and scenario steps must be non-zero".to_owned());
    }
    if config.failure_action != FailureAction::StopAndNeutralize {
        return Err("Only stop_and_neutralize is supported".to_owned());
    }
    let mut ids = HashSet::new();
    for step in &config.steps {
        validate_safe_id(&step.id, "step")?;
        if !ids.insert(step.id.as_str()) {
            return Err(format!("Duplicate step id {:?}", step.id));
        }
        if step.scene_id.trim().is_empty()
            || step.expected_scene_id.trim().is_empty()
            || step.inputs.is_empty()
            || step.timeout_ms == 0
        {
            return Err(format!(
                "Step {:?} has an empty or zero required field",
                step.id
            ));
        }
        for input in &step.inputs {
            if input.hold_ms == 0 {
                return Err(format!("Step {:?} has a zero-duration input", step.id));
            }
        }
    }
    if !ids.contains(config.entry_step_id.as_str()) {
        return Err(format!(
            "Entry step {:?} does not exist",
            config.entry_step_id
        ));
    }
    for step in &config.steps {
        if let NextStep::Id(id) = &step.next_step_id {
            if !ids.contains(id.as_str()) {
                return Err(format!("Step {:?} points to missing step {id:?}", step.id));
            }
        }
    }
    Ok(())
}

fn validate_safe_id(id: &str, kind: &str) -> Result<(), String> {
    if id.is_empty()
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        return Err(format!("Unsafe {kind} id {id:?}"));
    }
    Ok(())
}

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn update_status(status: &Arc<Mutex<ScenarioStatus>>, update: impl FnOnce(&mut ScenarioStatus)) {
    update(&mut lock(status));
}

fn record_input(status: &Arc<Mutex<ScenarioStatus>>, step_id: &str, input: &ScenarioInput) {
    update_status(status, |current| {
        current.input_logs.push(ScenarioInputLog {
            at_ms: unix_time_ms(),
            step_id: step_id.to_owned(),
            input_type: input.input_type,
            button: input.button,
            hold_ms: input.hold_ms,
        });
        if current.input_logs.len() > MAX_INPUT_LOGS {
            current.input_logs.remove(0);
        }
    });
}

#[tauri::command]
pub fn get_scenario_status(
    manager: State<'_, ScenarioManager>,
    analysis: State<'_, AnalysisManager>,
) -> ScenarioStatus {
    manager.status_with_resume_candidates(&analysis)
}

#[tauri::command(async)]
pub fn start_scenario(
    manager: State<'_, ScenarioManager>,
    analysis: State<'_, AnalysisManager>,
    game_id: String,
    scenario_id: String,
) -> Result<ScenarioStatus, String> {
    manager.start(&analysis, &game_id, &scenario_id)
}

#[tauri::command(async)]
pub fn resume_scenario(
    manager: State<'_, ScenarioManager>,
    analysis: State<'_, AnalysisManager>,
    game_id: String,
    scenario_id: String,
    step_id: String,
) -> Result<ScenarioStatus, String> {
    manager.resume(&analysis, &game_id, &scenario_id, &step_id)
}

#[tauri::command(async)]
pub fn stop_scenario(manager: State<'_, ScenarioManager>) -> ScenarioStatus {
    manager.stop()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn games_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../config/games")
    }

    #[test]
    fn loads_looping_culdcept_scenario() {
        let config = load_scenario(&games_root(), "culdcept-begins", "money-collect-automation")
            .expect("scenario should load");
        assert_eq!(config.steps.len(), 9);
        assert_eq!(config.entry_step_id, "step-01");
        assert_eq!(config.steps[4].scene_id, "result-battle");
        assert_eq!(config.steps[4].inputs[0].button, ControllerButton::A);
        assert_eq!(config.steps[5].scene_id, "mvp");
        assert_eq!(config.steps[6].scene_id, "reward-next");
        assert_eq!(config.steps[6].inputs[0].button, ControllerButton::A);
        assert_eq!(config.steps[7].scene_id, "get-cards");
        assert_eq!(config.steps[7].inputs[0].button, ControllerButton::Down);
        assert_eq!(config.steps[8].scene_id, "reward-next");
        assert_eq!(config.steps[8].inputs[0].button, ControllerButton::A);
        assert_eq!(
            config.steps[8].next_step_id,
            NextStep::Id("step-01".to_owned())
        );
    }

    #[test]
    fn resume_candidates_only_include_steps_for_the_current_scene() {
        let config = load_scenario(&games_root(), "culdcept-begins", "money-collect-automation")
            .expect("scenario should load");
        let candidates = resume_candidates(&config, "get-cards");
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].step_id, "step-08");
        assert!(resume_candidates(&config, "unknown").is_empty());
    }

    #[test]
    fn button_states_match_verified_firmware_recording() {
        assert_eq!(ControllerButton::A.state(), "080000000880000880");
        assert_eq!(ControllerButton::Y.state(), "010000000880000880");
        assert_eq!(ControllerButton::Down.state(), "000001000880000880");
        assert_eq!(ControllerButton::Minus.state(), "000100000880000880");
    }
}
