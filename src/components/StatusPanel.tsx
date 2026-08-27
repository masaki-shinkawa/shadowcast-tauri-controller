import type { CaptureStatus } from "../lib/tauri";

interface StatusPanelProps {
  status: CaptureStatus;
}

function formatResolution(status: CaptureStatus) {
  return status.width && status.height ? `${status.width} × ${status.height}` : "—";
}

export function StatusPanel({ status }: StatusPanelProps) {
  const isRunning = status.state === "running";

  return (
    <aside className="status-panel">
      <div className="status-heading">
        <div>
          <span className="eyebrow">CAPTURE STATUS</span>
          <h2>Signal telemetry</h2>
        </div>
        <div className={`status-pill status-pill--${status.state}`}>
          <span />
          {status.state}
        </div>
      </div>

      <dl className="telemetry-list">
        <div className="telemetry-row">
          <dt>Device</dt>
          <dd title={status.deviceName ?? undefined}>{status.deviceName ?? "Not connected"}</dd>
        </div>
        <div className="telemetry-row">
          <dt>Resolution</dt>
          <dd>{formatResolution(status)}</dd>
        </div>
        <div className="telemetry-row telemetry-row--split">
          <div>
            <dt>Target FPS</dt>
            <dd>{status.targetFps ?? "—"}</dd>
          </div>
          <div>
            <dt>Measured</dt>
            <dd>{isRunning ? status.measuredFps.toFixed(1) : "—"}</dd>
          </div>
        </div>
        <div className="telemetry-row telemetry-row--split">
          <div>
            <dt>Frame format</dt>
            <dd className="accent-value">{status.frameFormat ?? "—"}</dd>
          </div>
          <div>
            <dt>Frames</dt>
            <dd>{status.frameCount.toLocaleString()}</dd>
          </div>
        </div>
      </dl>

      <div className="pipeline-note">
        <span className="pipeline-icon" aria-hidden="true">
          ↯
        </span>
        <div>
          <strong>Direct MSMF pipeline</strong>
          <p>No FFmpeg · No getUserMedia · JPEG frames over Tauri Channel</p>
        </div>
      </div>

      {status.error && <div className="error-message">{status.error}</div>}
    </aside>
  );
}
