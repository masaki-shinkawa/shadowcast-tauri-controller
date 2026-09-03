import { describe, expect, it } from "vitest";
import type { ScenarioStatus } from "../lib/tauri";
import { getStepVisualState } from "./AutomationStepper";

const BASE_STATUS: ScenarioStatus = {
  state: "running",
  gameId: "culdcept-begins",
  scenarioId: "money-collect-automation",
  scenarioName: "金策自動化",
  currentStepId: "step-03",
  currentAttempt: 0,
  lastSceneId: "win-message",
  controllerPort: "COM3",
  completedSteps: 2,
  startedAtMs: 0,
  inputLogs: [],
  runId: "run-1",
  resumedFromRunId: null,
  logDirectory: null,
  evidencePath: null,
  resumeCandidates: [],
  error: null,
};

describe("getStepVisualState", () => {
  it("marks the current step active and prior visited steps complete", () => {
    const visited = new Set(["step-01", "step-02"]);
    expect(getStepVisualState(BASE_STATUS, "step-01", visited)).toBe("complete");
    expect(getStepVisualState(BASE_STATUS, "step-03", visited)).toBe("active");
    expect(getStepVisualState(BASE_STATUS, "step-04", visited)).toBe("pending");
  });

  it("marks the current step as an error without assuming later steps are complete", () => {
    const status = { ...BASE_STATUS, state: "error" as const };
    const visited = new Set(["step-01", "step-02", "step-04"]);
    expect(getStepVisualState(status, "step-03", visited)).toBe("error");
    expect(getStepVisualState(status, "step-04", visited)).toBe("complete");
    expect(getStepVisualState(status, "step-05", visited)).toBe("pending");
  });

  it("marks every step complete when a finite scenario completes", () => {
    const status = { ...BASE_STATUS, state: "completed" as const, currentStepId: null };
    expect(getStepVisualState(status, "step-08", new Set())).toBe("complete");
  });
});
