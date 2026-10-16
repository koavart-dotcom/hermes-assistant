import { beforeEach, describe, expect, it } from "vitest";
import { turnsFromMessages, useChatStore } from "@/stores/chat";

const S = "api_test";

describe("chat store", () => {
  beforeEach(() => {
    useChatStore.setState({ sessions: [], activeId: null, turns: {}, runs: {}, approvals: {}, error: null });
  });

  it("folds stored messages into user/assistant turns", () => {
    const turns = turnsFromMessages([
      { id: "1", role: "user", content: "привет", tool_name: null, timestamp: null },
      { id: "2", role: "tool", content: "{}", tool_name: "terminal", timestamp: null },
      { id: "3", role: "assistant", content: "здравствуйте", tool_name: null, timestamp: null },
      { id: "4", role: "assistant", content: "   ", tool_name: null, timestamp: null },
    ]);
    expect(turns).toEqual([
      { role: "user", content: "привет" },
      { role: "assistant", content: "здравствуйте" },
    ]);
  });

  it("streams a turn: run → deltas → tools → completed → ended", () => {
    const { addUserTurn, apply } = useChatStore.getState();
    addUserTurn(S, "сделай");
    apply(S, { kind: "run_started", run_id: "run_1" });
    expect(useChatStore.getState().runs[S]).toBe("run_1");
    apply(S, { kind: "delta", run_id: "run_1", text: "Сей" });
    apply(S, { kind: "delta", run_id: "run_1", text: "час" });
    apply(S, { kind: "tool_started", run_id: "run_1", tool: "terminal", preview: "ls" });
    apply(S, { kind: "tool_finished", run_id: "run_1", tool: "terminal", ok: true, preview: null });
    apply(S, { kind: "completed", run_id: "run_1", content: "Сейчас. Готово." });
    let turns = useChatStore.getState().turns[S] ?? [];
    expect(turns).toHaveLength(2);
    expect(turns[1]).toMatchObject({ role: "assistant", content: "Сейчас. Готово.", streaming: false });
    expect(turns[1]?.tools).toEqual([{ tool: "terminal", preview: null, status: "ok" }]);
    apply(S, { kind: "ended", run_id: "run_1" });
    turns = useChatStore.getState().turns[S] ?? [];
    expect(useChatStore.getState().runs[S]).toBeNull();
    expect(turns).toHaveLength(2);
  });

  it("keeps a pending approval until responded or ended", () => {
    const { apply } = useChatStore.getState();
    apply(S, { kind: "run_started", run_id: "run_2" });
    apply(S, {
      kind: "approval_request",
      run_id: "run_2",
      command: "rm -rf build",
      description: "Удаление",
      choices: ["once", "deny"],
    });
    expect(useChatStore.getState().approvals[S]).toMatchObject({ runId: "run_2", command: "rm -rf build" });
    apply(S, { kind: "approval_responded", run_id: "run_2", choice: "once" });
    expect(useChatStore.getState().approvals[S]).toBeNull();
  });

  it("marks a failed turn with the error and frees the run", () => {
    const { apply } = useChatStore.getState();
    apply(S, { kind: "run_started", run_id: "run_3" });
    apply(S, { kind: "failed", run_id: "run_3", error: "boom" });
    apply(S, { kind: "ended", run_id: "run_3" });
    const turns = useChatStore.getState().turns[S] ?? [];
    expect(turns[0]).toMatchObject({ role: "assistant", streaming: false, error: "boom" });
    expect(useChatStore.getState().runs[S]).toBeNull();
  });
});
