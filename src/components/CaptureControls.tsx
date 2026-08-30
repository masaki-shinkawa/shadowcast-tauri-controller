interface CaptureControlsProps {
  running: boolean;
  busy: boolean;
  onStart: () => void;
  onStop: () => void;
  screenshotBusy: boolean;
  screenshotAvailable: boolean;
  screenshotMessage: string | null;
  onScreenshot: () => void;
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
        className="control-button control-button--screenshot"
        onClick={onScreenshot}
        disabled={!running || !screenshotAvailable || screenshotBusy}
      >
        <span className="camera-icon" aria-hidden="true" />
        Save frame
      </button>
      {busy && <span className="working-indicator">Working…</span>}
      {screenshotMessage && (
        <span className="screenshot-message" title={screenshotMessage}>
          {screenshotMessage}
        </span>
      )}
    </fieldset>
  );
}
