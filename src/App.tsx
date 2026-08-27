import { useCallback, useEffect, useRef, useState } from "react";
import { CaptureControls } from "./components/CaptureControls";
import { StatusPanel } from "./components/StatusPanel";
import { VideoPreview } from "./components/VideoPreview";
import {
  type CaptureStatus,
  type FrameBytes,
  type FrameListener,
  getCaptureStatus,
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
  error: null,
};

export default function App() {
  const [status, setStatus] = useState<CaptureStatus>(INITIAL_STATUS);
  const [busy, setBusy] = useState(false);
  const frameListeners = useRef(new Set<FrameListener>());

  const subscribe = useCallback((listener: FrameListener) => {
    frameListeners.current.add(listener);
    return () => frameListeners.current.delete(listener);
  }, []);

  const broadcastFrame = useCallback((frame: FrameBytes) => {
    for (const listener of frameListeners.current) listener(frame);
  }, []);

  const refreshStatus = useCallback(async () => {
    try {
      setStatus(await getCaptureStatus());
    } catch (error) {
      setStatus((current) => ({ ...current, state: "error", error: String(error) }));
    }
  }, []);

  useEffect(() => {
    void refreshStatus();
    const interval = window.setInterval(() => void refreshStatus(), 500);
    return () => window.clearInterval(interval);
  }, [refreshStatus]);

  const handleStart = async () => {
    setBusy(true);
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
        <VideoPreview subscribe={subscribe} running={running} />
        <StatusPanel status={status} />
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
