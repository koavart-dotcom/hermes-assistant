import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";

const api = vi.hoisted(() => ({
  envKeysList: vi.fn(),
  envKeysPresets: vi.fn(),
  envKeysSet: vi.fn(),
  envKeysRemove: vi.fn(),
  gatewayRestart: vi.fn(),
}));
vi.mock("@/services/tauri", () => ({ api, isTauri: () => true }));

import { EnvKeysSection } from "@/features/settings/EnvKeysSection";

function renderSection() {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <EnvKeysSection />
    </QueryClientProvider>,
  );
}

describe("EnvKeysSection", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    api.envKeysPresets.mockResolvedValue([
      { id: "freescout", title: "FreeScout", keys: ["FREESCOUT_URL", "FREESCOUT_API_KEY"], hint: "адрес и ключ" },
    ]);
    api.envKeysList.mockResolvedValue([
      { name: "API_SERVER_KEY", masked: "••••abcd" },
      { name: "FREESCOUT_API_KEY", masked: "••••1234" },
      { name: "MY_TOKEN", masked: "••••zzzz" },
    ]);
    api.envKeysSet.mockResolvedValue(undefined);
    api.envKeysRemove.mockResolvedValue(undefined);
    api.gatewayRestart.mockResolvedValue(undefined);
  });

  it("shows presets with masked values, hides internal keys, lists custom ones", async () => {
    renderSection();
    expect(await screen.findByText("FreeScout")).toBeInTheDocument();
    expect(screen.getByPlaceholderText("••••1234")).toBeInTheDocument();
    expect(screen.getByText("не задано")).toBeInTheDocument(); // the preset as a whole isn't complete
    expect(screen.getByText("MY_TOKEN")).toBeInTheDocument();
    expect(screen.queryByText("API_SERVER_KEY")).toBeNull();
  });

  it("saves a key, then offers a Hermes restart", async () => {
    renderSection();
    const input = await screen.findByLabelText("FREESCOUT_URL");
    fireEvent.change(input, { target: { value: "https://help.example.com" } });
    fireEvent.keyDown(input, { key: "Enter" });
    await waitFor(() => expect(api.envKeysSet).toHaveBeenCalledWith("FREESCOUT_URL", "https://help.example.com"));
    const restart = await screen.findByRole("button", { name: /Перезапустить Hermes/ });
    fireEvent.click(restart);
    await waitFor(() => expect(api.gatewayRestart).toHaveBeenCalled());
    await waitFor(() => expect(screen.queryByRole("status")).toBeNull());
  });

  it("removes a configured key", async () => {
    renderSection();
    fireEvent.click(await screen.findByRole("button", { name: "Удалить FREESCOUT_API_KEY" }));
    await waitFor(() => expect(api.envKeysRemove).toHaveBeenCalledWith("FREESCOUT_API_KEY"));
  });
});
