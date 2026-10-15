import { applyTheme, useUiStore } from "@/stores/ui";

describe("ui store", () => {
  it("applies dark class and persists theme", () => {
    useUiStore.getState().setTheme("dark");
    expect(document.documentElement.classList.contains("dark")).toBe(true);
    expect(localStorage.getItem("hermes.theme")).toBe("dark");
    applyTheme("light");
    expect(document.documentElement.classList.contains("dark")).toBe(false);
  });
});
