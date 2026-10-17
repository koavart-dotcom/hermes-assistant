import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/services/tauri", () => ({ api: {}, reportError: vi.fn(), logInfo: vi.fn() }));

import { applyCallStatus, inCall, resetCallState } from "@/lib/voicePipeline";
import { DEFAULT_PREFS, useVoiceStore } from "@/stores/voice";

const teams = { in_call: true, apps: ["Teams"] };
const none = { in_call: false, apps: [] };

describe("auto-quiet on calls", () => {
  beforeEach(() => {
    resetCallState();
    useVoiceStore.setState({ quiet: false, mode: "chat", prefs: { ...DEFAULT_PREFS } });
  });

  it("goes quiet for the call and comes back after it", async () => {
    await applyCallStatus(teams);
    expect(inCall()).toBe(true);
    expect(useVoiceStore.getState().quiet).toBe(true);
    expect(useVoiceStore.getState().answer).toContain("Teams");
    await applyCallStatus(none);
    expect(inCall()).toBe(false);
    expect(useVoiceStore.getState().quiet).toBe(false);
  });

  it("keeps a quiet mode that was switched on by hand", async () => {
    useVoiceStore.setState({ quiet: true });
    await applyCallStatus(teams);
    await applyCallStatus(none);
    expect(useVoiceStore.getState().quiet).toBe(true);
  });

  it("does nothing when the setting is off", async () => {
    useVoiceStore.setState({ prefs: { ...DEFAULT_PREFS, auto_quiet_calls: false } });
    await applyCallStatus(teams);
    expect(inCall()).toBe(false);
    expect(useVoiceStore.getState().quiet).toBe(false);
  });
});
