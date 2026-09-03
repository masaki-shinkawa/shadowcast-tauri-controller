use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::game_state::SceneDetection;

pub const DIAGNOSTICS_LIMIT_BYTES: u64 = 500 * 1024 * 1024;

#[derive(Clone, Debug)]
pub(crate) struct DiagnosticFrame {
    pub frame_number: u64,
    pub captured_at_ms: u64,
    pub jpeg: Vec<u8>,
}

#[derive(Clone, Debug)]
pub(crate) struct RunMetadata {
    pub game_id: String,
    pub scenario_id: String,
    pub scenario_name: String,
    pub controller_port: String,
    pub start_step_id: String,
    pub resumed_from_run_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct RunManifest {
    schema_version: u32,
    run_id: String,
    game_id: String,
    scenario_id: String,
    scenario_name: String,
    controller_port: String,
    state: String,
    started_at_ms: u64,
    ended_at_ms: Option<u64>,
    start_step_id: String,
    current_step_id: Option<String>,
    completed_steps: u64,
    resumed_from_run_id: Option<String>,
    error: Option<String>,
    log_directory: String,
}

pub(crate) struct AutomationRunLog {
    root: PathBuf,
    directory: PathBuf,
    events_path: PathBuf,
    manifest: RunManifest,
    sequence: u64,
    image_storage_exhausted: bool,
}

impl AutomationRunLog {
    pub fn start(root: &Path, games_root: &Path, metadata: RunMetadata) -> Result<Self, String> {
        fs::create_dir_all(root)
            .map_err(|error| format!("Failed to create {}: {error}", root.display()))?;
        rotate_logs(root, None, 0)?;

        let started_at_ms = unix_time_ms();
        let prefix = format!(
            "{}-{}-{}",
            started_at_ms,
            safe_part(&metadata.game_id),
            safe_part(&metadata.scenario_id)
        );
        let mut suffix = 0_u32;
        let directory = loop {
            let name = if suffix == 0 {
                prefix.clone()
            } else {
                format!("{prefix}-{suffix}")
            };
            let candidate = root.join(name);
            match fs::create_dir(&candidate) {
                Ok(()) => break candidate,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => suffix += 1,
                Err(error) => {
                    return Err(format!("Failed to create {}: {error}", candidate.display()))
                }
            }
        };
        fs::create_dir(directory.join("screenshots")).map_err(|error| {
            format!(
                "Failed to create screenshot directory in {}: {error}",
                directory.display()
            )
        })?;

        snapshot_configuration(
            games_root,
            &directory.join("configuration"),
            &metadata.game_id,
            &metadata.scenario_id,
        )?;

        let run_id = directory
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("automation-run")
            .to_owned();
        let manifest = RunManifest {
            schema_version: 1,
            run_id,
            game_id: metadata.game_id,
            scenario_id: metadata.scenario_id,
            scenario_name: metadata.scenario_name,
            controller_port: metadata.controller_port,
            state: "running".to_owned(),
            started_at_ms,
            ended_at_ms: None,
            start_step_id: metadata.start_step_id.clone(),
            current_step_id: Some(metadata.start_step_id),
            completed_steps: 0,
            resumed_from_run_id: metadata.resumed_from_run_id,
            error: None,
            log_directory: directory.to_string_lossy().into_owned(),
        };
        let log = Self {
            root: root.to_path_buf(),
            events_path: directory.join("events.jsonl"),
            directory,
            manifest,
            sequence: 0,
            image_storage_exhausted: false,
        };
        log.write_manifest()?;
        Ok(log)
    }

    pub fn run_id(&self) -> &str {
        &self.manifest.run_id
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    pub fn update_progress(&mut self, step_id: Option<&str>, completed_steps: u64) {
        self.manifest.current_step_id = step_id.map(str::to_owned);
        self.manifest.completed_steps = completed_steps;
        let _ = self.write_manifest();
    }

    pub fn record(
        &mut self,
        event_type: &str,
        mut payload: Value,
        frame: Option<&DiagnosticFrame>,
    ) -> Result<(), String> {
        self.sequence += 1;
        let screenshot = frame
            .and_then(|frame| self.save_frame(event_type, frame).transpose())
            .transpose()?;
        let mut event = json!({
            "sequence": self.sequence,
            "atMs": unix_time_ms(),
            "type": event_type,
            "payload": payload,
        });
        if let Some(path) = screenshot {
            event["screenshot"] = Value::String(path);
        }
        if self.image_storage_exhausted {
            event["imageStorageExhausted"] = Value::Bool(true);
        }
        payload = event;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.events_path)
            .map_err(|error| format!("Failed to open {}: {error}", self.events_path.display()))?;
        serde_json::to_writer(&mut file, &payload)
            .map_err(|error| format!("Failed to serialize automation event: {error}"))?;
        file.write_all(b"\n")
            .map_err(|error| format!("Failed to append {}: {error}", self.events_path.display()))
    }

    pub fn finish(&mut self, state: &str, error: Option<String>) -> Result<(), String> {
        self.manifest.state = state.to_owned();
        self.manifest.ended_at_ms = Some(unix_time_ms());
        self.manifest.error = error;
        self.write_manifest()?;
        if state == "error" {
            self.write_latest_error()?;
        }
        rotate_logs(&self.root, Some(self.run_id()), 0).map(|_| ())
    }

    fn save_frame(
        &mut self,
        event_type: &str,
        frame: &DiagnosticFrame,
    ) -> Result<Option<String>, String> {
        if self.image_storage_exhausted {
            return Ok(None);
        }
        if !rotate_logs(&self.root, Some(self.run_id()), frame.jpeg.len() as u64)? {
            self.image_storage_exhausted = true;
            return Ok(None);
        }
        let filename = format!(
            "{:06}-{}-frame-{}.jpg",
            self.sequence,
            safe_part(event_type),
            frame.frame_number
        );
        let relative = Path::new("screenshots").join(filename);
        let path = self.directory.join(&relative);
        fs::write(&path, &frame.jpeg)
            .map_err(|error| format!("Failed to save {}: {error}", path.display()))?;
        Ok(Some(relative.to_string_lossy().replace('\\', "/")))
    }

    fn write_manifest(&self) -> Result<(), String> {
        write_json(&self.directory.join("manifest.json"), &self.manifest)
    }

    fn write_latest_error(&self) -> Result<(), String> {
        let evidence = json!({
            "schemaVersion": 1,
            "runId": self.run_id(),
            "gameId": self.manifest.game_id,
            "scenarioId": self.manifest.scenario_id,
            "error": self.manifest.error,
            "runDirectory": self.directory,
            "manifestPath": self.directory.join("manifest.json"),
            "eventsPath": self.events_path,
            "liveDirectory": self.root.join("live"),
            "createdAtMs": unix_time_ms(),
        });
        write_json(&self.directory.join("error-evidence.json"), &evidence)?;
        write_json(&self.root.join("latest-error.json"), &evidence)
    }
}

impl Drop for AutomationRunLog {
    fn drop(&mut self) {
        if self.manifest.state != "running" {
            return;
        }
        self.manifest.state = "error".to_owned();
        self.manifest.ended_at_ms = Some(unix_time_ms());
        self.manifest.error = Some("Automation run ended without a terminal event".to_owned());
        let _ = self.write_manifest();
        let _ = self.write_latest_error();
    }
}

pub(crate) fn write_live_snapshot(
    root: &Path,
    detection: &SceneDetection,
    frame: &DiagnosticFrame,
) -> Result<(), String> {
    let directory = root.join("live");
    fs::create_dir_all(&directory)
        .map_err(|error| format!("Failed to create {}: {error}", directory.display()))?;
    let image_file = format!("latest-{}.jpg", frame.frame_number % 2);
    fs::write(directory.join(&image_file), &frame.jpeg)
        .map_err(|error| format!("Failed to write versioned live screenshot: {error}"))?;
    fs::write(directory.join("latest.jpg"), &frame.jpeg)
        .map_err(|error| format!("Failed to write live screenshot: {error}"))?;
    write_json(
        &directory.join("state.json"),
        &json!({
            "schemaVersion": 1,
            "capturedAtMs": frame.captured_at_ms,
            "frameNumber": frame.frame_number,
            "imageFile": image_file,
            "sceneDetection": detection,
        }),
    )
}

fn snapshot_configuration(
    games_root: &Path,
    destination: &Path,
    game_id: &str,
    scenario_id: &str,
) -> Result<(), String> {
    let game_root = games_root.join(game_id);
    fs::create_dir_all(destination)
        .map_err(|error| format!("Failed to create {}: {error}", destination.display()))?;
    copy_file(&game_root.join("game.yaml"), &destination.join("game.yaml"))?;
    copy_file(
        &game_root
            .join("scenarios")
            .join(format!("{scenario_id}.yaml")),
        &destination.join("scenario.yaml"),
    )?;
    let scene_source = game_root.join("scenes");
    let scene_destination = destination.join("scenes");
    fs::create_dir_all(&scene_destination)
        .map_err(|error| format!("Failed to create {}: {error}", scene_destination.display()))?;
    for entry in fs::read_dir(&scene_source)
        .map_err(|error| format!("Failed to read {}: {error}", scene_source.display()))?
    {
        let entry = entry.map_err(|error| format!("Failed to read scene entry: {error}"))?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) == Some("yaml") {
            copy_file(&path, &scene_destination.join(entry.file_name()))?;
        }
    }
    Ok(())
}

fn copy_file(source: &Path, destination: &Path) -> Result<(), String> {
    fs::copy(source, destination).map(|_| ()).map_err(|error| {
        format!(
            "Failed to copy {} to {}: {error}",
            source.display(),
            destination.display()
        )
    })
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let file = File::create(path)
        .map_err(|error| format!("Failed to create {}: {error}", path.display()))?;
    serde_json::to_writer_pretty(file, value)
        .map_err(|error| format!("Failed to write {}: {error}", path.display()))
}

fn rotate_logs(
    root: &Path,
    protected_run_id: Option<&str>,
    incoming_bytes: u64,
) -> Result<bool, String> {
    rotate_logs_with_limit(
        root,
        protected_run_id,
        incoming_bytes,
        DIAGNOSTICS_LIMIT_BYTES,
    )
}

fn rotate_logs_with_limit(
    root: &Path,
    protected_run_id: Option<&str>,
    incoming_bytes: u64,
    limit_bytes: u64,
) -> Result<bool, String> {
    let mut total = directory_size(root)?;
    if total.saturating_add(incoming_bytes) <= limit_bytes {
        return Ok(true);
    }

    let mut runs = Vec::new();
    let mut newest_unresolved: Option<(u64, String)> = None;
    for entry in
        fs::read_dir(root).map_err(|error| format!("Failed to read {}: {error}", root.display()))?
    {
        let entry = entry.map_err(|error| format!("Failed to read diagnostics entry: {error}"))?;
        if !entry
            .file_type()
            .map_err(|error| format!("Failed to inspect {}: {error}", entry.path().display()))?
            .is_dir()
        {
            continue;
        }
        let path = entry.path();
        let manifest_path = path.join("manifest.json");
        if !manifest_path.exists() {
            continue;
        }
        let manifest: RunManifest = match fs::read(&manifest_path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        {
            Some(manifest) => manifest,
            None => continue,
        };
        let resolved = path.join("resolution.json").exists();
        if manifest.state == "error" && !resolved {
            let replace = newest_unresolved
                .as_ref()
                .is_none_or(|(started, _)| manifest.started_at_ms > *started);
            if replace {
                newest_unresolved = Some((manifest.started_at_ms, manifest.run_id.clone()));
            }
        }
        let rank = match manifest.state.as_str() {
            "completed" | "stopped" => 0,
            "error" if resolved => 1,
            "error" => 2,
            _ => 3,
        };
        runs.push((rank, manifest.started_at_ms, manifest.run_id, path));
    }
    runs.sort_by_key(|(rank, started, _, _)| (*rank, *started));
    let newest_unresolved = newest_unresolved.map(|(_, run_id)| run_id);

    for (_, _, run_id, path) in runs {
        if protected_run_id == Some(run_id.as_str())
            || newest_unresolved.as_deref() == Some(run_id.as_str())
        {
            continue;
        }
        let size = directory_size(&path)?;
        fs::remove_dir_all(&path)
            .map_err(|error| format!("Failed to rotate {}: {error}", path.display()))?;
        total = total.saturating_sub(size);
        if total.saturating_add(incoming_bytes) <= limit_bytes {
            return Ok(true);
        }
    }
    Ok(total.saturating_add(incoming_bytes) <= limit_bytes)
}

fn directory_size(path: &Path) -> Result<u64, String> {
    if !path.exists() {
        return Ok(0);
    }
    let mut total = 0_u64;
    for entry in
        fs::read_dir(path).map_err(|error| format!("Failed to read {}: {error}", path.display()))?
    {
        let entry = entry.map_err(|error| format!("Failed to read directory entry: {error}"))?;
        let metadata = entry
            .metadata()
            .map_err(|error| format!("Failed to inspect {}: {error}", entry.path().display()))?;
        total = total.saturating_add(if metadata.is_dir() {
            directory_size(&entry.path())?
        } else {
            metadata.len()
        });
    }
    Ok(total)
}

fn safe_part(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '-' || character == '_' {
                character
            } else {
                '_'
            }
        })
        .collect()
}

pub(crate) fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_parts_cannot_escape_the_diagnostics_directory() {
        assert_eq!(safe_part("../game:name"), "___game_name");
    }

    #[test]
    fn rotation_keeps_the_newest_unresolved_error() {
        let root = test_directory("rotation");
        fs::create_dir_all(&root).expect("create test root");
        let completed = write_test_run(&root, "completed-run", "completed", 1, 1024);
        let error = write_test_run(&root, "error-run", "error", 2, 1024);
        let limit = directory_size(&error).expect("measure protected run") + 16;

        assert!(rotate_logs_with_limit(&root, None, 0, limit).expect("rotate logs"));
        assert!(!completed.exists());
        assert!(error.exists());

        fs::remove_dir_all(&root).expect("remove test root");
    }

    #[test]
    fn failed_run_writes_an_immutable_evidence_pointer() {
        let root = test_directory("evidence");
        let games_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../config/games");
        let mut log = AutomationRunLog::start(
            &root,
            &games_root,
            RunMetadata {
                game_id: "culdcept-begins".to_owned(),
                scenario_id: "money-collect-automation".to_owned(),
                scenario_name: "Money collection".to_owned(),
                controller_port: "COM3".to_owned(),
                start_step_id: "step-01".to_owned(),
                resumed_from_run_id: None,
            },
        )
        .expect("start run log");
        log.record("attempt_started", json!({ "attempt": 0 }), None)
            .expect("record event");
        log.finish("error", Some("test failure".to_owned()))
            .expect("finish run log");

        assert!(root.join("latest-error.json").exists());
        assert!(log.directory().join("error-evidence.json").exists());
        assert!(log.directory().join("configuration/scenario.yaml").exists());

        drop(log);
        fs::remove_dir_all(&root).expect("remove test root");
    }

    fn write_test_run(
        root: &Path,
        run_id: &str,
        state: &str,
        started_at_ms: u64,
        payload_bytes: usize,
    ) -> PathBuf {
        let directory = root.join(run_id);
        fs::create_dir(&directory).expect("create run directory");
        let manifest = RunManifest {
            schema_version: 1,
            run_id: run_id.to_owned(),
            game_id: "game".to_owned(),
            scenario_id: "scenario".to_owned(),
            scenario_name: "Scenario".to_owned(),
            controller_port: "COM3".to_owned(),
            state: state.to_owned(),
            started_at_ms,
            ended_at_ms: Some(started_at_ms),
            start_step_id: "step-1".to_owned(),
            current_step_id: Some("step-1".to_owned()),
            completed_steps: 0,
            resumed_from_run_id: None,
            error: (state == "error").then(|| "failed".to_owned()),
            log_directory: directory.to_string_lossy().into_owned(),
        };
        write_json(&directory.join("manifest.json"), &manifest).expect("write manifest");
        fs::write(directory.join("payload.bin"), vec![0_u8; payload_bytes]).expect("write payload");
        directory
    }

    fn test_directory(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "shadowcast-diagnostics-{label}-{}-{}",
            std::process::id(),
            unix_time_ms()
        ))
    }
}
