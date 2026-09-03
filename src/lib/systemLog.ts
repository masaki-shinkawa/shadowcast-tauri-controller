import type { ScenarioInputLog, SceneDetection, SceneTransition } from "./tauri";

export interface SystemLogEntry {
  id: string;
  atMs: number;
  type: "game-state" | "input";
  title: string;
  detail: string;
}

export function buildSystemLogEntries(
  sceneDetection: SceneDetection,
  sceneTransitions: SceneTransition[],
  inputLogs: ScenarioInputLog[],
): SystemLogEntry[] {
  const gameStateEntries = sceneTransitions
    .filter(({ detection }) => detection.sceneId !== "unknown")
    .map(({ detection }) => ({
      id: `game-state-${detection.detectedAtMs}-${detection.frameNumber}`,
      atMs: detection.detectedAtMs,
      type: "game-state" as const,
      title: detection.sceneId,
      detail: `${(detection.confidence * 100).toFixed(1)}% confidence`,
    }));

  if (
    sceneDetection.sceneId !== "unknown" &&
    !gameStateEntries.some(
      (entry) =>
        entry.atMs === sceneDetection.detectedAtMs && entry.title === sceneDetection.sceneId,
    )
  ) {
    gameStateEntries.push({
      id: `game-state-current-${sceneDetection.detectedAtMs}-${sceneDetection.frameNumber}`,
      atMs: sceneDetection.detectedAtMs,
      type: "game-state",
      title: sceneDetection.sceneId,
      detail: `${(sceneDetection.confidence * 100).toFixed(1)}% confidence`,
    });
  }

  const inputEntries = inputLogs.map((input, index) => ({
    id: `input-${input.atMs}-${input.stepId}-${index}`,
    atMs: input.atMs,
    type: "input" as const,
    title: input.button,
    detail: `${input.stepId} · ${input.inputType} · ${input.holdMs} ms`,
  }));

  return [...gameStateEntries, ...inputEntries].sort(
    (left, right) => left.atMs - right.atMs || left.id.localeCompare(right.id),
  );
}

export function formatLogTime(atMs: number): string {
  return new Intl.DateTimeFormat(undefined, {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
    hour12: false,
  }).format(new Date(atMs));
}
