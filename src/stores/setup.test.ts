import { useSetupStore } from "@/stores/setup";

describe("setup store", () => {
  beforeEach(() => useSetupStore.getState().reset());

  it("tracks steps, progress and completion", () => {
    const { apply } = useSetupStore.getState();
    apply({ kind: "step_started", step: "hermes" });
    expect(useSetupStore.getState().phase).toBe("running");
    expect(useSetupStore.getState().steps.hermes).toBe("running");
    apply({ kind: "progress", label: "ollama", received: 50n, total: 100n });
    expect(useSetupStore.getState().progress?.label).toBe("ollama");
    apply({ kind: "step_finished", step: "hermes" });
    expect(useSetupStore.getState().steps.hermes).toBe("done");
    apply({ kind: "done" });
    expect(useSetupStore.getState().phase).toBe("done");
  });

  it("records failures", () => {
    useSetupStore.getState().apply({ kind: "failed", message: "boom" });
    expect(useSetupStore.getState().phase).toBe("failed");
    expect(useSetupStore.getState().error).toBe("boom");
  });
});
