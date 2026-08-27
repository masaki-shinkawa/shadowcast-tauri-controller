import { Channel, invoke } from "@tauri-apps/api/core";

export type CaptureState = "starting" | "running" | "stopped" | "error";

export interface CaptureStatus {
  state: CaptureState;
  deviceName: string | null;
  width: number | null;
  height: number | null;
  targetFps: number | null;
  measuredFps: number;
  frameFormat: string | null;
  frameCount: number;
  jpegBytes: number;
  averageJpegBytes: number;
  channelMbps: number;
  averageChannelSendMs: number;
  error: string | null;
}

export interface PreviewMetrics {
  receivedFps: number;
  renderedFps: number;
  receiveMbps: number;
  receiveToDrawMs: number;
  droppedFrames: number;
}

export const EMPTY_PREVIEW_METRICS: PreviewMetrics = {
  receivedFps: 0,
  renderedFps: 0,
  receiveMbps: 0,
  receiveToDrawMs: 0,
  droppedFrames: 0,
};

type FramePayload = ArrayBuffer | Uint8Array | number[];

export type FrameBytes = Uint8Array<ArrayBuffer>;
export type FrameListener = (frame: FrameBytes) => void;

export function framePayloadToBytes(payload: FramePayload): FrameBytes {
  if (payload instanceof Uint8Array) {
    if (payload.buffer instanceof ArrayBuffer) {
      return new Uint8Array(payload.buffer, payload.byteOffset, payload.byteLength);
    }
    return new Uint8Array(payload);
  }
  if (payload instanceof ArrayBuffer) {
    return new Uint8Array(payload);
  }
  return Uint8Array.from(payload);
}

export async function startCapture(onFrame: FrameListener): Promise<CaptureStatus> {
  const channel = new Channel<FramePayload>();
  channel.onmessage = (payload) => onFrame(framePayloadToBytes(payload));
  return invoke<CaptureStatus>("start_capture", { onFrame: channel });
}

export async function stopCapture(): Promise<CaptureStatus> {
  return invoke<CaptureStatus>("stop_capture");
}

export async function getCaptureStatus(): Promise<CaptureStatus> {
  return invoke<CaptureStatus>("get_capture_status");
}

export async function reportPreviewMetrics(metrics: PreviewMetrics): Promise<void> {
  return invoke("report_preview_metrics", { metrics });
}
