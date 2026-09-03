import type { ScenarioResumeCandidate } from "../lib/tauri";

interface CaptureControlsProps {
  running: boolean;
  busy: boolean;
  onStart: () => void;
  onStop: () => void;
  screenshotBusy: boolean;
  screenshotAvailable: boolean;
  screenshotMessage: string | null;
  onScreenshot: () => void;
  scenarioRunning: boolean;
  scenarioReady: boolean;
  scenarioBlockedReason: string | null;
  scenarioBusy: boolean;
  onScenarioStart: () => void;
  onScenarioStop: () => void;
  scenarioError: boolean;
  resumeCandidates: ScenarioResumeCandidate[];
  selectedResumeStep: string;
  onResumeStepChange: (stepId: string) => void;
  resumeReady: boolean;
  resumeBlockedReason: string | null;
  onScenarioResume: () => void;
}

export function CaptureControls({
  running,
  busy,
  onStart,
  onStop,
  screenshotBusy,
  screenshotAvailable,
  screenshotMessage,
  onScreenshot,
  scenarioRunning,
  scenarioReady,
  scenarioBlockedReason,
  scenarioBusy,
  onScenarioStart,
  onScenarioStop,
  scenarioError,
  resumeCandidates,
  selectedResumeStep,
  onResumeStepChange,
  resumeReady,
  resumeBlockedReason,
  onScenarioResume,
}: CaptureControlsProps) {
  return (
    <fieldset className="controls">
      <legend className="sr-only">Capture controls</legend>
      <button
        type="button"
        className="control-button control-button--start"
        onClick={onStart}
        disabled={busy || running}
      >
        <span className="play-icon" aria-hidden="true" />
        Start capture
      </button>
      <button
        type="button"
        className="control-button control-button--stop"
        onClick={onStop}
        disabled={busy || !running}
      >
        <span className="stop-icon" aria-hidden="true" />
        Stop
      </button>
      <button
        type="button"
        className="control-button control-button--automation"
        onClick={onScenarioStart}
        disabled={!scenarioReady || scenarioRunning || scenarioBusy}
        title={scenarioBlockedReason ?? undefined}
      >
        <span className="automation-icon" aria-hidden="true">
          ↻
        </span>
        Start automation
      </button>
      <button
        type="button"
        className="control-button control-button--stop"
        onClick={onScenarioStop}
        disabled={!scenarioRunning || scenarioBusy}
      >
        <span className="stop-icon" aria-hidden="true" />
        Stop automation
      </button>
      {scenarioError && (
        <div className="resume-controls">
          {resumeCandidates.length > 1 && (
            <select
              aria-label="Resume automation step"
              value={selectedResumeStep}
              disabled={scenarioBusy}
              onChange={(event) => onResumeStepChange(event.target.value)}
            >
              {resumeCandidates.map((candidate) => (
                <option key={candidate.stepId} value={candidate.stepId}>
                  {candidate.stepId} · {candidate.sceneId}
                </option>
              ))}
            </select>
          )}
          <button
            type="button"
            className="control-button control-button--automation"
            onClick={onScenarioResume}
            disabled={!resumeReady || scenarioBusy || !selectedResumeStep}
            title={resumeBlockedReason ?? undefined}
          >
            <span className="automation-icon" aria-hidden="true">
              ↪
            </span>
            {selectedResumeStep ? `Resume from ${selectedResumeStep}` : "Resume unavailable"}
          </button>
        </div>
      )}
      <button
        type="button"
        className="control-button control-button--screenshot"
        onClick={onScreenshot}
        disabled={!running || !screenshotAvailable || screenshotBusy}
      >
        <span className="camera-icon" aria-hidden="true" />
        Save frame
      </button>
      {(busy || scenarioBusy) && <span className="working-indicator">Working…</span>}
      {screenshotMessage && (
        <span className="screenshot-message" title={screenshotMessage}>
          {screenshotMessage}
        </span>
      )}
    </fieldset>
  );
}
