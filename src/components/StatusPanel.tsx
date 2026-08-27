import type { CaptureStatus, PreviewMetrics } from "../lib/tauri";

interface StatusPanelProps {
  status: CaptureStatus;
  previewMetrics: PreviewMetrics;
  telemetryBusy: boolean;
  onTelemetryToggle: () => void;
}

function formatResolution(status: CaptureStatus) {
  return status.width && status.height ? `${status.width} × ${status.height}` : "—";
}

function formatKib(bytes: number) {
  return bytes > 0 ? `${(bytes / 1024).toFixed(1)} KiB` : "—";
}

export function StatusPanel({
  status,
  previewMetrics,
  telemetryBusy,
  onTelemetryToggle,
}: StatusPanelProps) {
  const isRunning = status.state === "running";

  return (
    <aside className="status-panel">
      <div className="status-heading">
        <div>
          <span className="eyebrow">CAPTURE STATUS</span>
          <h2>Signal telemetry</h2>
        </div>
        <div className="status-heading-actions">
          <button
            type="button"
            className={`telemetry-toggle ${status.telemetryEnabled ? "telemetry-toggle--on" : ""}`}
            aria-pressed={status.telemetryEnabled}
            disabled={telemetryBusy}
            onClick={onTelemetryToggle}
          >
            <span aria-hidden="true" />
            TELEMETRY {status.telemetryEnabled ? "ON" : "OFF"}
          </button>
          <div className={`status-pill status-pill--${status.state}`}>
            <span />
            {status.state}
          </div>
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
        {status.telemetryEnabled ? (
          <>
            <div className="telemetry-row telemetry-row--split">
              <div>
                <dt>Capture FPS</dt>
                <dd>{isRunning ? status.measuredFps.toFixed(1) : "—"}</dd>
              </div>
              <div>
                <dt>Rendered FPS</dt>
                <dd>{isRunning ? previewMetrics.renderedFps.toFixed(1) : "—"}</dd>
              </div>
            </div>
            <div className="telemetry-row telemetry-row--split">
              <div>
                <dt>JPEG average</dt>
                <dd>{formatKib(status.averageJpegBytes)}</dd>
              </div>
              <div>
                <dt>Channel</dt>
                <dd>{isRunning ? `${status.channelMbps.toFixed(1)} Mb/s` : "—"}</dd>
              </div>
            </div>
            <div className="telemetry-row telemetry-row--split">
              <div>
                <dt>Receive → draw</dt>
                <dd>{isRunning ? `${previewMetrics.receiveToDrawMs.toFixed(1)} ms` : "—"}</dd>
              </div>
              <div>
                <dt>Dropped / frames</dt>
                <dd>
                  {previewMetrics.droppedFrames.toLocaleString()} /{" "}
                  {status.frameCount.toLocaleString()}
                </dd>
              </div>
            </div>
            <div className="telemetry-row telemetry-row--split">
              <div>
                <dt>Target / format</dt>
                <dd className="accent-value">
                  {status.targetFps ?? "—"} / {status.frameFormat ?? "—"}
                </dd>
              </div>
              <div>
                <dt>Send call</dt>
                <dd>{isRunning ? `${status.averageChannelSendMs.toFixed(3)} ms` : "—"}</dd>
              </div>
            </div>
          </>
        ) : (
          <div className="telemetry-row">
            <dt>Target / format</dt>
            <dd className="accent-value">
              {status.targetFps ?? "—"} / {status.frameFormat ?? "—"}
            </dd>
          </div>
        )}
      </dl>

      {!status.telemetryEnabled && (
        <div className="telemetry-disabled" role="status">
          <strong>Detailed telemetry is off</strong>
          <p>Enable it when collecting FPS, throughput, and render timing.</p>
        </div>
      )}

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
