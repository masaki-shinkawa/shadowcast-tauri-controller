import { useCallback, useEffect, useRef, useState } from "react";
import { CaptureControls } from "./components/CaptureControls";
import { StatusPanel } from "./components/StatusPanel";
import { VideoPreview } from "./components/VideoPreview";
import {
  type AnalysisStatus,
  type CaptureStatus,
  EMPTY_PREVIEW_METRICS,
  type FrameBytes,
  type FrameListener,
  getAnalysisStatus,
  getCaptureStatus,
  type PreviewMetrics,
  reportPreviewMetrics,
  setTelemetryEnabled,
  startCapture,
  stopCapture,
} from "./lib/tauri";

const INITIAL_STATUS: CaptureStatus = {
  state: "stopped",
  deviceName: null,
  width: null,
  height: null,
  targetFps: null,
  measuredFps: 0,
  frameFormat: null,
  frameCount: 0,
  jpegBytes: 0,
  averageJpegBytes: 0,
  channelMbps: 0,
  averageChannelSendMs: 0,
  telemetryEnabled: false,
  averageAnalysisSubmitMs: 0,
  error: null,
};

const INITIAL_ANALYSIS_STATUS: AnalysisStatus = {
  state: "stopped",
  config: {
    enabled: true,
    roi: { x: 480, y: 270, width: 320, height: 180 },
    targetColor: { red: 0, green: 255, blue: 0 },
    colorTolerance: 48,
    maxFps: 15,
  },
  submittedFrames: 0,
  analyzedFrames: 0,
  droppedFrames: 0,
  failedFrames: 0,
  measuredFps: 0,
  averageAnalysisMs: 0,
  lastResult: null,
  gameStateProfile: {
    name: "generic-switch-game-v1",
    confirmationFrames: 3,
    timeoutMs: 2000,
    loadingLumaMax: 24,
    gameplayColorRatioMinPercent: 20,
    resultTemplateScoreMinPercent: 90,
  },
  gameState: {
    state: "unknown",
    confidence: 0,
    detectedAtMs: 0,
    frameNumber: 0,
    reason: "",
    consecutiveFrames: 0,
  },
  stateTransitions: [],
  error: null,
};

export default function App() {
  const [status, setStatus] = useState<CaptureStatus>(INITIAL_STATUS);
  const [previewMetrics, setPreviewMetrics] = useState<PreviewMetrics>(EMPTY_PREVIEW_METRICS);
  const [analysisStatus, setAnalysisStatus] = useState<AnalysisStatus>(INITIAL_ANALYSIS_STATUS);
  const [busy, setBusy] = useState(false);
  const [telemetryBusy, setTelemetryBusy] = useState(false);
  const frameListeners = useRef(new Set<FrameListener>());

  const subscribe = useCallback((listener: FrameListener) => {
    frameListeners.current.add(listener);
    return () => frameListeners.current.delete(listener);
  }, []);

  const broadcastFrame = useCallback((frame: FrameBytes) => {
    for (const listener of frameListeners.current) listener(frame);
  }, []);

  const handlePreviewMetrics = useCallback((metrics: PreviewMetrics) => {
    setPreviewMetrics(metrics);
    if (metrics.receivedFps > 0) {
      void reportPreviewMetrics(metrics).catch(() => undefined);
    }
  }, []);

  const refreshStatus = useCallback(async () => {
    const [capture, analysis] = await Promise.allSettled([getCaptureStatus(), getAnalysisStatus()]);
    if (capture.status === "fulfilled") setStatus(capture.value);
    else setStatus((current) => ({ ...current, state: "error", error: String(capture.reason) }));
    if (analysis.status === "fulfilled") setAnalysisStatus(analysis.value);
    else
      setAnalysisStatus((current) => ({
        ...current,
        state: "error",
        error: String(analysis.reason),
      }));
  }, []);

  useEffect(() => {
    void refreshStatus();
    const interval = window.setInterval(() => void refreshStatus(), 500);
    return () => window.clearInterval(interval);
  }, [refreshStatus]);

  const handleStart = async () => {
    setBusy(true);
    setPreviewMetrics(EMPTY_PREVIEW_METRICS);
    setStatus((current) => ({ ...current, state: "starting", error: null }));
    try {
      setStatus(await startCapture(broadcastFrame));
    } catch (error) {
      setStatus((current) => ({ ...current, state: "error", error: String(error) }));
    } finally {
      setBusy(false);
    }
  };

  const handleStop = async () => {
    setBusy(true);
    try {
      setStatus(await stopCapture());
    } catch (error) {
      setStatus((current) => ({ ...current, state: "error", error: String(error) }));
    } finally {
      setBusy(false);
    }
  };

  const handleTelemetryToggle = async () => {
    setTelemetryBusy(true);
    try {
      setStatus(await setTelemetryEnabled(!status.telemetryEnabled));
      setPreviewMetrics(EMPTY_PREVIEW_METRICS);
    } catch (error) {
      setStatus((current) => ({ ...current, error: String(error) }));
    } finally {
      setTelemetryBusy(false);
    }
  };

  const running = status.state === "running";

  return (
    <main className="app-shell">
      <header className="app-header">
        <div className="brand-lockup">
          <div className="brand-mark" aria-hidden="true">
            <span />
          </div>
          <div>
            <span className="eyebrow">SHADOWCAST CONTROLLER</span>
            <h1>Live capture console</h1>
          </div>
        </div>
        <div className="connection-label">
          <span className={running ? "live-dot" : "idle-dot"} />
          WINDOWS · MEDIA FOUNDATION
        </div>
      </header>

      <div className="workspace">
        <VideoPreview
          subscribe={subscribe}
          running={running}
          telemetryEnabled={status.telemetryEnabled}
          onMetrics={handlePreviewMetrics}
        />
        <StatusPanel
          status={status}
          previewMetrics={previewMetrics}
          analysisStatus={analysisStatus}
          telemetryBusy={telemetryBusy}
          onTelemetryToggle={() => void handleTelemetryToggle()}
        />
      </div>

      <footer className="app-footer">
        <CaptureControls
          running={running}
          busy={busy}
          onStart={() => void handleStart()}
          onStop={() => void handleStop()}
        />
        <p>1280 × 720 · 60 FPS · MJPEG preferred</p>
      </footer>
    </main>
  );
}
