import { render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { App } from "@/app/App";

function renderAt(path: string) {
  const qc = new QueryClient();
  return render(
    <QueryClientProvider client={qc}>
      <MemoryRouter initialEntries={[path]}>
        <App />
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

describe("App shell", () => {
  it("redirects root to chat and shows navigation", () => {
    renderAt("/");
    expect(screen.getByRole("heading", { name: "Чат" })).toBeInTheDocument();
    expect(screen.getByRole("navigation", { name: "Разделы" })).toBeInTheDocument();
  });

  it("renders status page in browser mode without Tauri", () => {
    renderAt("/status");
    expect(screen.getByTestId("app-version")).toHaveTextContent("браузерный режим");
  });
});
