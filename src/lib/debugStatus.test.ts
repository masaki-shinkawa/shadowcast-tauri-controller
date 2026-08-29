import { describe, expect, it } from "vitest";
import { formatCaptureDebugStatus } from "./debugStatus";
import type { AnalysisStatus, CaptureStatus, PreviewMetrics } from "./tauri";

const capture: CaptureStatus = {
  state: "running",
  deviceName: "ShadowCast",
  width: 1280,
  height: 720,
  targetFps: 60,
  measuredFps: 59.5,
  frameFormat: "MJPEG",
  frameCount: 3582,
  jpegBytes: 172_544,
  averageJpegBytes: 172_544,
  channelMbps: 87,
  averageChannelSendMs: 0.035,
  telemetryEnabled: true,
  averageAnalysisSubmitMs: 0.012,
  error: null,
};

const preview: PreviewMetrics = {
  receivedFps: 58.1,
  renderedFps: 56,
  receiveMbps: 84.2,
  receiveToDrawMs: 8.9,
  droppedFrames: 94,
};

const analysis: AnalysisStatus = {
  state: "running",
  config: {
    enabled: true,
    roi: { x: 480, y: 270, width: 320, height: 180 },
    targetColor: { red: 0, green: 255, blue: 0 },
    colorTolerance: 48,
    maxFps: 15,
  },
  submittedFrames: 3627,
  analyzedFrames: 369,
  droppedFrames: 3258,
  failedFrames: 0,
  measuredFps: 6.3,
  averageAnalysisMs: 164.37,
  lastResult: {
    frameNumber: 3580,
    sourceWidth: 1280,
    sourceHeight: 720,
    roi: { x: 480, y: 270, width: 320, height: 180 },
    color: {
      target: { red: 0, green: 255, blue: 0 },
      tolerance: 48,
      average: { red: 72, green: 81, blue: 94 },
      matchingPixels: 0,
      totalPixels: 57_600,
      matchRatio: 0,
    },
    templateMatch: null,
    queueDelayMs: 8.25,
    analysisMs: 162.4,
    jpegDecodeMs: 145.2,
    colorAnalysisMs: 17.1,
    templateMatchMs: 0,
  },
  error: null,
};

describe("formatCaptureDebugStatus", () => {
  it("formats capture, preview, and analysis diagnostics for sharing", () => {
    const text = formatCaptureDebugStatus(
      capture,
      preview,
      analysis,
      new Date("2026-08-28T03:04:05.000Z"),
    );

    expect(text).toContain("Captured at: 2026-08-28T03:04:05.000Z");
    expect(text).toContain("Device: ShadowCast");
    expect(text).toContain("Analysis FPS: 6.3");
    expect(text).toContain(
      "Analysis stages, decode / color / template (latest): 145.20 / 17.10 / 0.00 ms",
    );
    expect(text).toContain(
      "Analysis frames (submitted / analyzed / dropped / failed): 3,627 / 369 / 3,258 / 0",
    );
    expect(text).toContain("Configured ROI: x=480, y=270, width=320, height=180");
    expect(text).toContain("Capture error: none");
  });
});
