import { useEffect, useRef } from "react";
import type { FrameBytes, FrameListener, PreviewMetrics } from "../lib/tauri";

interface VideoPreviewProps {
  subscribe: (listener: FrameListener) => () => void;
  running: boolean;
  telemetryEnabled: boolean;
  onMetrics: (metrics: PreviewMetrics) => void;
}

interface PendingFrame {
  bytes: FrameBytes;
  receivedAt: number;
}

export function VideoPreview({
  subscribe,
  running,
  telemetryEnabled,
  onMetrics,
}: VideoPreviewProps) {
  const imageRef = useRef<HTMLImageElement>(null);

  useEffect(() => {
    if (!running) {
      imageRef.current?.removeAttribute("src");
      return;
    }

    let latestFrame: PendingFrame | null = null;
    let animationFrame: number | null = null;
    let pendingLoad = false;
    const objectUrls = new Set<string>();
    let intervalStarted = telemetryEnabled ? performance.now() : 0;
    let intervalReceived = 0;
    let intervalRendered = 0;
    let intervalBytes = 0;
    let intervalDrawLatency = 0;
    let intervalDrawSamples = 0;
    let droppedFrames = 0;

    const paintLatestFrame = () => {
      animationFrame = null;
      const image = imageRef.current;
      if (!image || !latestFrame) return;

      const frame = latestFrame;
      const url = URL.createObjectURL(new Blob([frame.bytes], { type: "image/jpeg" }));
      objectUrls.add(url);
      latestFrame = null;

      if (telemetryEnabled && pendingLoad) droppedFrames += 1;
      pendingLoad = telemetryEnabled;
      image.onload = () => {
        pendingLoad = false;
        if (telemetryEnabled) {
          intervalRendered += 1;
          intervalDrawLatency += performance.now() - frame.receivedAt;
          intervalDrawSamples += 1;
        }
        for (const existingUrl of objectUrls) {
          if (existingUrl !== url) {
            URL.revokeObjectURL(existingUrl);
            objectUrls.delete(existingUrl);
          }
        }
      };
      image.src = url;

      // Bound memory even when WebView2 skips load callbacks while frames arrive quickly.
      while (objectUrls.size > 4) {
        const staleUrl = objectUrls.values().next().value;
        if (!staleUrl || staleUrl === url) break;
        URL.revokeObjectURL(staleUrl);
        objectUrls.delete(staleUrl);
      }
    };

    const unsubscribe = subscribe((frame) => {
      if (telemetryEnabled && latestFrame) droppedFrames += 1;
      latestFrame = {
        bytes: frame,
        receivedAt: telemetryEnabled ? performance.now() : 0,
      };
      if (telemetryEnabled) {
        intervalReceived += 1;
        intervalBytes += frame.byteLength;
      }
      if (animationFrame === null) {
        animationFrame = requestAnimationFrame(paintLatestFrame);
      }
    });

    const metricsInterval = telemetryEnabled
      ? window.setInterval(() => {
          const now = performance.now();
          const elapsedSeconds = (now - intervalStarted) / 1_000;
          if (elapsedSeconds <= 0) return;

          onMetrics({
            receivedFps: intervalReceived / elapsedSeconds,
            renderedFps: intervalRendered / elapsedSeconds,
            receiveMbps: (intervalBytes * 8) / elapsedSeconds / 1_000_000,
            receiveToDrawMs:
              intervalDrawSamples === 0 ? 0 : intervalDrawLatency / intervalDrawSamples,
            droppedFrames,
          });
          intervalStarted = now;
          intervalReceived = 0;
          intervalRendered = 0;
          intervalBytes = 0;
          intervalDrawLatency = 0;
          intervalDrawSamples = 0;
        }, 1_000)
      : null;

    return () => {
      unsubscribe();
      if (metricsInterval !== null) window.clearInterval(metricsInterval);
      if (animationFrame !== null) cancelAnimationFrame(animationFrame);
      for (const url of objectUrls) URL.revokeObjectURL(url);
      imageRef.current?.removeAttribute("src");
    };
  }, [running, telemetryEnabled, subscribe, onMetrics]);

  return (
    <section className="preview-shell" aria-label="ShadowCast video preview">
      <div className="preview-grid" aria-hidden="true" />
      <img ref={imageRef} className="preview-image" alt="ShadowCast live video" />
      <div className={`preview-placeholder ${running ? "preview-placeholder--waiting" : ""}`}>
        <div className="signal-mark" aria-hidden="true">
          <span />
          <span />
          <span />
        </div>
        <p>{running ? "Waiting for video signal" : "ShadowCast is standing by"}</p>
        <span>{running ? "The first frame will appear here" : "Press Start to begin capture"}</span>
      </div>
      <div className="preview-badge">
        <i className={running ? "live-dot" : "idle-dot"} />
        {running ? "LIVE" : "OFFLINE"}
      </div>
      <div className="preview-label">GENKI SHADOWCAST · DIRECT CAPTURE</div>
    </section>
  );
}
