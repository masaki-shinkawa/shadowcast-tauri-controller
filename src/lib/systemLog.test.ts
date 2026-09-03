import { describe, expect, it } from "vitest";
import { buildSystemLogEntries } from "./systemLog";
import type { ScenarioInputLog, SceneDetection, SceneTransition } from "./tauri";

const detection = (sceneId: string, detectedAtMs: number): SceneDetection => ({
  gameId: "test-game",
  sceneId,
  confidence: 0.92,
  detectedAtMs,
  frameNumber: detectedAtMs,
  evidence: [],
  consecutiveFrames: 3,
  candidateSceneId: null,
  candidateConsecutiveFrames: 0,
});

describe("buildSystemLogEntries", () => {
  it("omits unknown game states and merges recognized states with inputs chronologically", () => {
    const transitions: SceneTransition[] = [
      { fromSceneId: "title", detection: detection("unknown", 200), reason: "expired" },
      { fromSceneId: "unknown", detection: detection("battle", 100), reason: "stable" },
    ];
    const inputs: ScenarioInputLog[] = [
      { atMs: 150, stepId: "step-01", inputType: "tap", button: "A", holdMs: 80 },
    ];

    expect(buildSystemLogEntries(detection("battle", 100), transitions, inputs)).toEqual([
      expect.objectContaining({ type: "game-state", title: "battle", atMs: 100 }),
      expect.objectContaining({ type: "input", title: "A", atMs: 150 }),
    ]);
  });

  it("includes a recognized current state when no transition is available", () => {
    expect(buildSystemLogEntries(detection("result", 300), [], [])).toEqual([
      expect.objectContaining({ type: "game-state", title: "result", atMs: 300 }),
    ]);
  });
});
