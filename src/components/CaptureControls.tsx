interface CaptureControlsProps {
  running: boolean;
  busy: boolean;
  onStart: () => void;
  onStop: () => void;
}

export function CaptureControls({ running, busy, onStart, onStop }: CaptureControlsProps) {
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
      {busy && <span className="working-indicator">Working…</span>}
    </fieldset>
  );
}
