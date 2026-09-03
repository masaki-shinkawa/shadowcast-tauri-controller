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
  telemetryEnabled: boolean;
  averageAnalysisSubmitMs: number;
  error: string | null;
}

export interface Roi {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface RgbColor {
  red: number;
  green: number;
  blue: number;
}

export interface AnalysisConfig {
  enabled: boolean;
  roi: Roi;
  targetColor: RgbColor;
  colorTolerance: number;
  maxFps: number;
}

export interface ColorAnalysis {
  target: RgbColor;
  tolerance: number;
  average: RgbColor;
  matchingPixels: number;
  totalPixels: number;
  matchRatio: number;
}

export interface TemplateMatch {
  x: number;
  y: number;
  width: number;
  height: number;
  score: number;
  searchStep: number;
}

export interface AnalysisResult {
  frameNumber: number;
  sourceWidth: number;
  sourceHeight: number;
  roi: Roi;
  color: ColorAnalysis;
  templateMatch: TemplateMatch | null;
  queueDelayMs: number;
  analysisMs: number;
  jpegDecodeMs: number;
  colorAnalysisMs: number;
  templateMatchMs: number;
}

export interface AnalysisStatus {
  state: "running" | "stopped" | "error";
  config: AnalysisConfig;
  submittedFrames: number;
  analyzedFrames: number;
  droppedFrames: number;
  failedFrames: number;
  measuredFps: number;
  averageAnalysisMs: number;
  lastResult: AnalysisResult | null;
  gameProfile: GameProfileSummary;
  sceneDetection: SceneDetection;
  sceneTransitions: SceneTransition[];
  error: string | null;
}

export type ScenarioState = "idle" | "running" | "stopping" | "stopped" | "completed" | "error";

export interface ScenarioStatus {
  state: ScenarioState;
  gameId: string | null;
  scenarioId: string | null;
  scenarioName: string | null;
  currentStepId: string | null;
  currentAttempt: number | null;
  lastSceneId: string | null;
  controllerPort: string | null;
  completedSteps: number;
  startedAtMs: number | null;
  inputLogs: ScenarioInputLog[];
  runId: string | null;
  resumedFromRunId: string | null;
  logDirectory: string | null;
  evidencePath: string | null;
  resumeCandidates: ScenarioResumeCandidate[];
  error: string | null;
}

export interface ScenarioResumeCandidate {
  stepId: string;
  sceneId: string;
}

export interface ScenarioInputLog {
  atMs: number;
  stepId: string;
  inputType: "tap" | "hold";
  button: string;
  holdMs: number;
}

export type ManualControllerConnectionState = "disconnected" | "connected" | "error";

export interface ManualControllerStatus {
  state: ManualControllerConnectionState;
  port: string | null;
  availablePorts: string[];
  error: string | null;
}

export type ManualControllerButton =
  | "A"
  | "B"
  | "X"
  | "Y"
  | "UP"
  | "DOWN"
  | "LEFT"
  | "RIGHT"
  | "L"
  | "R"
  | "ZL"
  | "ZR"
  | "PLUS"
  | "MINUS"
  | "L_STICK"
  | "R_STICK"
  | "HOME"
  | "CAPTURE";

export type ManualControllerStick = "left" | "right";

export interface StabilityConfig {
  consecutiveFrames: number;
  timeoutMs: number;
}

export interface SceneSummary {
  id: string;
  detectorCount: number;
  combination: "all" | "any";
  stability: StabilityConfig;
}

export interface GameProfileSummary {
  gameId: string;
  gameName: string;
  resolution: [number, number];
  scenes: SceneSummary[];
}

export interface DetectorEvidence {
  detectorType: string;
  matched: boolean;
  confidence: number;
  observed: number;
  expected: string;
  region: [number, number, number, number];
  detail: string;
}

export interface SceneDetection {
  gameId: string;
  sceneId: string;
  confidence: number;
  detectedAtMs: number;
  frameNumber: number;
  evidence: DetectorEvidence[];
  consecutiveFrames: number;
  candidateSceneId: string | null;
  candidateConsecutiveFrames: number;
}

export interface SceneTransition {
  fromSceneId: string;
  detection: SceneDetection;
  reason: string;
}

export interface AnalysisTemplateInput {
  width: number;
  height: number;
  grayscale: number[];
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

export async function setTelemetryEnabled(enabled: boolean): Promise<CaptureStatus> {
  return invoke<CaptureStatus>("set_telemetry_enabled", { enabled });
}

export async function reportPreviewMetrics(metrics: PreviewMetrics): Promise<void> {
  return invoke("report_preview_metrics", { metrics });
}

export async function getAnalysisStatus(): Promise<AnalysisStatus> {
  return invoke<AnalysisStatus>("get_analysis_status");
}

export async function loadGameConfig(gameId: string): Promise<AnalysisStatus> {
  return invoke<AnalysisStatus>("load_game_config", { gameId });
}

export async function getScenarioStatus(): Promise<ScenarioStatus> {
  return invoke<ScenarioStatus>("get_scenario_status");
}

export async function startScenario(gameId: string, scenarioId: string): Promise<ScenarioStatus> {
  return invoke<ScenarioStatus>("start_scenario", { gameId, scenarioId });
}

export async function resumeScenario(
  gameId: string,
  scenarioId: string,
  stepId: string,
): Promise<ScenarioStatus> {
  return invoke<ScenarioStatus>("resume_scenario", { gameId, scenarioId, stepId });
}

export async function stopScenario(): Promise<ScenarioStatus> {
  return invoke<ScenarioStatus>("stop_scenario");
}

export async function getManualControllerStatus(): Promise<ManualControllerStatus> {
  return invoke<ManualControllerStatus>("get_manual_controller_status");
}

export async function connectManualController(port: string): Promise<ManualControllerStatus> {
  return invoke<ManualControllerStatus>("connect_manual_controller", { port });
}

export async function disconnectManualController(): Promise<ManualControllerStatus> {
  return invoke<ManualControllerStatus>("disconnect_manual_controller");
}

export async function setManualControllerButton(
  button: ManualControllerButton,
  pressed: boolean,
): Promise<void> {
  return invoke("set_manual_controller_button", { button, pressed });
}

export async function setManualControllerStick(
  stick: ManualControllerStick,
  x: number,
  y: number,
): Promise<void> {
  return invoke("set_manual_controller_stick", { stick, x, y });
}

export async function neutralizeManualController(): Promise<void> {
  return invoke("neutralize_manual_controller");
}

export async function saveGameScreenshot(frame: FrameBytes): Promise<string> {
  return invoke<string>("save_game_screenshot", { jpeg: Array.from(frame) });
}

export async function configureAnalysis(config: AnalysisConfig): Promise<AnalysisStatus> {
  return invoke<AnalysisStatus>("configure_analysis", { config });
}

export async function setAnalysisTemplate(template: AnalysisTemplateInput | null): Promise<void> {
  return invoke("set_analysis_template", { template });
}
