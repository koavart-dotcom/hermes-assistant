import { create } from "zustand";
import type { SetupEvent } from "@/types/generated/SetupEvent";
import type { SetupStep } from "@/types/generated/SetupStep";

export type StepState = "pending" | "running" | "done";
export type SetupPhase = "idle" | "running" | "done" | "failed";

export interface ProgressInfo {
  label: string;
  received: bigint;
  total: bigint | null;
}

interface SetupState {
  phase: SetupPhase;
  steps: Record<SetupStep, StepState>;
  currentStage: string | null;
  progress: ProgressInfo | null;
  log: string[];
  error: string | null;
  reset: () => void;
  apply: (event: SetupEvent) => void;
}

export const STEP_ORDER: SetupStep[] = ["hermes", "config", "ollama", "model", "gateway"];

export const STEP_TITLES: Record<SetupStep, string> = {
  hermes: "Hermes Agent",
  config: "Настройка Hermes",
  ollama: "Ollama",
  model: "Локальная модель",
  gateway: "Запуск gateway",
};

const emptySteps = (): Record<SetupStep, StepState> => ({
  hermes: "pending",
  config: "pending",
  ollama: "pending",
  model: "pending",
  gateway: "pending",
});

const MAX_LOG = 500;

export const useSetupStore = create<SetupState>((set) => ({
  phase: "idle",
  steps: emptySteps(),
  currentStage: null,
  progress: null,
  log: [],
  error: null,
  reset: () => set({ phase: "idle", steps: emptySteps(), currentStage: null, progress: null, log: [], error: null }),
  apply: (event) =>
    set((s) => {
      switch (event.kind) {
        case "step_started":
          return { phase: "running", steps: { ...s.steps, [event.step]: "running" }, progress: null };
        case "step_finished":
          return { steps: { ...s.steps, [event.step]: "done" }, currentStage: null, progress: null };
        case "stage_started":
          return { currentStage: event.title };
        case "stage_finished":
          return {
            currentStage: null,
            log: pushLog(s.log, `${event.stage}: ${event.skipped ? "пропущено" : event.ok ? "готово" : "ошибка"}${event.reason ? ` (${event.reason})` : ""}`),
          };
        case "progress":
          return { progress: { label: event.label, received: event.received, total: event.total } };
        case "log":
          return { log: pushLog(s.log, event.line) };
        case "done":
          return { phase: "done", progress: null, currentStage: null };
        case "failed":
          return { phase: "failed", error: event.message, progress: null };
      }
    }),
}));

function pushLog(log: string[], line: string): string[] {
  const next = log.length >= MAX_LOG ? log.slice(log.length - MAX_LOG + 1) : log.slice();
  next.push(line);
  return next;
}
