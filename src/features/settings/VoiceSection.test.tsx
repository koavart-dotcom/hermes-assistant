import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";

const api = vi.hoisted(() => ({
  prefsGet: vi.fn(),
  prefsSet: vi.fn(),
  voicePacks: vi.fn(),
  voicePackImport: vi.fn(),
}));
vi.mock("@/services/tauri", () => ({ api, isTauri: () => true }));

import { VoiceSection } from "@/features/settings/VoiceSection";
import { useVoiceStore } from "@/stores/voice";

const prefs = {
  voice_volume: 0.8,
  silent: false,
  wake_words: ["джарвис", "гермес"],
  wake_required: true,
  voice_pack: "Jarvis",
  digest: { enabled: false, times: ["09:00"], speak: true, telegram: true },
  claude_project: "hermes-assistant",
  hotkey: "F9",
};

describe("VoiceSection", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    api.prefsGet.mockResolvedValue(prefs);
    api.voicePacks.mockResolvedValue({ selected: "Jarvis", installed: [{ name: "Jarvis", clips: 122 }], importable: ["Friday"] });
    api.prefsSet.mockImplementation(async (p: typeof prefs) => p);
    api.voicePackImport.mockResolvedValue(10);
  });

  it("saves changed wake words and silent mode into the store", async () => {
    render(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <VoiceSection />
      </QueryClientProvider>,
    );
    const wake = await screen.findByLabelText("Слова-обращения");
    fireEvent.change(wake, { target: { value: "джарвис, пятница" } });
    fireEvent.click(screen.getByLabelText("Тихий режим"));
    fireEvent.click(screen.getByRole("button", { name: "Сохранить" }));
    await waitFor(() => expect(api.prefsSet).toHaveBeenCalledWith({ ...prefs, silent: true, wake_words: ["джарвис", "пятница"] }));
    await waitFor(() => expect(useVoiceStore.getState().prefs.silent).toBe(true));
  });

  it("imports a Luxify voice pack", async () => {
    render(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <VoiceSection />
      </QueryClientProvider>,
    );
    fireEvent.click(await screen.findByRole("button", { name: "+ Friday" }));
    await waitFor(() => expect(api.voicePackImport).toHaveBeenCalledWith("Friday"));
  });
});
