import type { AnalysisStatus, CaptureStatus, PreviewMetrics, RgbColor, Roi } from "./tauri";

function formatRgb(color: RgbColor) {
  return `rgb(${color.red}, ${color.green}, ${color.blue})`;
}

function formatRoi(roi: Roi) {
  return `x=${roi.x}, y=${roi.y}, width=${roi.width}, height=${roi.height}`;
}

function formatInteger(value: number) {
  return value.toLocaleString("en-US");
}

export function formatCaptureDebugStatus(
  capture: CaptureStatus,
  preview: PreviewMetrics,
  analysis: AnalysisStatus,
  capturedAt = new Date(),
) {
  const result = analysis.lastResult;
  const templateMatch = result?.templateMatch;

  return [
    "ShadowCast capture debug",
    `Captured at: ${capturedAt.toISOString()}`,
    `Capture state: ${capture.state}`,
    `Analysis state: ${analysis.state}`,
    `Device: ${capture.deviceName ?? "Not connected"}`,
    `Resolution: ${capture.width && capture.height ? `${capture.width} x ${capture.height}` : "-"}`,
    `Capture FPS: ${capture.measuredFps.toFixed(1)}`,
    `Rendered FPS: ${preview.renderedFps.toFixed(1)}`,
    `Received FPS: ${preview.receivedFps.toFixed(1)}`,
    `Analysis FPS: ${analysis.measuredFps.toFixed(1)}`,
    `Analysis / submit: ${analysis.averageAnalysisMs.toFixed(2)} / ${capture.averageAnalysisSubmitMs.toFixed(3)} ms`,
    `Analysis enabled / max FPS: ${analysis.config.enabled} / ${analysis.config.maxFps}`,
    `Analysis frames (submitted / analyzed / dropped / failed): ${formatInteger(analysis.submittedFrames)} / ${formatInteger(analysis.analyzedFrames)} / ${formatInteger(analysis.droppedFrames)} / ${formatInteger(analysis.failedFrames)}`,
    `Analysis queue delay (latest): ${result ? `${result.queueDelayMs.toFixed(2)} ms` : "-"}`,
    `Analysis time (latest): ${result ? `${result.analysisMs.toFixed(2)} ms` : "-"}`,
    `Analysis stages, decode / color / template (latest): ${
      result
        ? `${result.jpegDecodeMs.toFixed(2)} / ${result.colorAnalysisMs.toFixed(2)} / ${result.templateMatchMs.toFixed(2)} ms`
        : "-"
    }`,
    `Configured ROI: ${formatRoi(analysis.config.roi)}`,
    `Analyzed ROI: ${result ? formatRoi(result.roi) : "-"}`,
    `Target color / tolerance: ${formatRgb(analysis.config.targetColor)} / ${analysis.config.colorTolerance}`,
    `Average ROI color: ${result ? formatRgb(result.color.average) : "-"}`,
    `ROI color match: ${result ? `${(result.color.matchRatio * 100).toFixed(1)}%` : "-"}`,
    `Template match: ${
      templateMatch
        ? `x=${templateMatch.x}, y=${templateMatch.y}, score=${templateMatch.score.toFixed(4)}, step=${templateMatch.searchStep}`
        : "-"
    }`,
    `JPEG average: ${capture.averageJpegBytes > 0 ? `${(capture.averageJpegBytes / 1024).toFixed(1)} KiB` : "-"}`,
    `Channel / received: ${capture.channelMbps.toFixed(1)} / ${preview.receiveMbps.toFixed(1)} Mb/s`,
    `Receive -> draw: ${preview.receiveToDrawMs.toFixed(1)} ms`,
    `Preview dropped / capture frames: ${formatInteger(preview.droppedFrames)} / ${formatInteger(capture.frameCount)}`,
    `Target / format: ${capture.targetFps ?? "-"} / ${capture.frameFormat ?? "-"}`,
    `Send call: ${capture.averageChannelSendMs.toFixed(3)} ms`,
    `Telemetry enabled: ${capture.telemetryEnabled}`,
    `Capture error: ${capture.error ?? "none"}`,
    `Analysis error: ${analysis.error ?? "none"}`,
  ].join("\n");
}

export async function copyText(text: string) {
  try {
    if (!navigator.clipboard?.writeText) throw new Error("Clipboard API unavailable");
    await navigator.clipboard.writeText(text);
    return;
  } catch {
    const textarea = document.createElement("textarea");
    textarea.value = text;
    textarea.setAttribute("readonly", "");
    textarea.style.position = "fixed";
    textarea.style.opacity = "0";
    document.body.appendChild(textarea);
    textarea.select();
    const copied = document.execCommand("copy");
    textarea.remove();
    if (!copied) throw new Error("Could not copy debug status");
  }
}
