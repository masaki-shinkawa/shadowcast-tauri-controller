import type { ScenarioStatus } from "../lib/tauri";

const AUTOMATION_STEPS = [
  { id: "step-01", label: "対戦設定" },
  { id: "step-02", label: "CPU対戦" },
  { id: "step-03", label: "勝利表示" },
  { id: "step-04", label: "勝者情報" },
  { id: "step-05", label: "リザルト" },
  { id: "step-06", label: "MVP" },
  { id: "step-07", label: "カード取得" },
  { id: "step-08", label: "報酬確認" },
] as const;

type StepVisualState = "pending" | "complete" | "active" | "stopped" | "error";

interface AutomationStepperProps {
  status: ScenarioStatus;
}

export function getStepVisualState(
  status: ScenarioStatus,
  stepId: string,
  visitedStepIds: ReadonlySet<string>,
): StepVisualState {
  if (status.state === "completed") return "complete";
  if (status.currentStepId === stepId) {
    if (status.state === "error") return "error";
    if (status.state === "stopped") return "stopped";
    return "active";
  }
  return visitedStepIds.has(stepId) ? "complete" : "pending";
}

function stateLabel(status: ScenarioStatus) {
  if (status.state === "idle") return "開始待ち";
  if (status.state === "completed") return "完了";
  if (status.state === "stopping") return "停止中";
  if (status.state === "stopped") return "停止済み";
  if (status.state === "error") return "エラー";
  return "実行中";
}

function stepSummary(status: ScenarioStatus) {
  const currentIndex = AUTOMATION_STEPS.findIndex((step) => step.id === status.currentStepId);
  if (currentIndex < 0) return stateLabel(status);

  const attempt = status.currentAttempt === null ? "" : ` · TRY ${status.currentAttempt + 1}`;
  return `STEP ${String(currentIndex + 1).padStart(2, "0")} / ${String(AUTOMATION_STEPS.length).padStart(2, "0")}${attempt}`;
}

export function AutomationStepper({ status }: AutomationStepperProps) {
  const visitedStepIds = new Set(status.inputLogs.map((entry) => entry.stepId));
  const visualStates = AUTOMATION_STEPS.map((step) =>
    getStepVisualState(status, step.id, visitedStepIds),
  );

  return (
    <section className="automation-progress" aria-labelledby="automation-progress-title">
      <div className="automation-progress-heading">
        <div>
          <span className="eyebrow">AUTOMATION FLOW</span>
          <h2 id="automation-progress-title">{status.scenarioName ?? "金策自動化"}</h2>
        </div>
        <div className="automation-progress-status" aria-live="polite">
          <span className={`automation-state automation-state--${status.state}`}>
            <i aria-hidden="true" />
            {stateLabel(status)}
          </span>
          <strong>{stepSummary(status)}</strong>
          <small>{status.completedSteps.toLocaleString()} steps completed</small>
        </div>
      </div>

      <ol className="automation-stepper" aria-label="Automation steps">
        {AUTOMATION_STEPS.map((step, index) => {
          const visualState = visualStates[index];
          const nextState = visualStates[index + 1];
          const connectorPassed =
            visualState === "complete" &&
            (nextState === "complete" || nextState === "active" || nextState === "error");

          return (
            <li
              key={step.id}
              className={`automation-step automation-step--${visualState}${connectorPassed ? " automation-step--connector-passed" : ""}`}
              aria-current={
                visualState === "active" || visualState === "error" ? "step" : undefined
              }
            >
              <span className="automation-step-icon" aria-hidden="true">
                {visualState === "complete" ? "✓" : visualState === "error" ? "!" : index + 1}
              </span>
              <span className="automation-step-id">{step.id}</span>
              <strong>{step.label}</strong>
              {visualState === "active" && <small>RUNNING</small>}
              {visualState === "error" && <small>ERROR</small>}
              {visualState === "stopped" && <small>STOPPED</small>}
            </li>
          );
        })}
      </ol>
    </section>
  );
}
