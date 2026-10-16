import React from "react";
import ReactDOM from "react-dom/client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { HashRouter } from "react-router";
import { App } from "@/app/App";
import { ErrorBoundary } from "@/app/ErrorBoundary";
import { installErrorReporting, reportError } from "@/services/tauri";
import { applyTheme, useUiStore } from "@/stores/ui";
import "./index.css";

installErrorReporting();
try {
  applyTheme(useUiStore.getState().theme);
} catch (e) {
  reportError("theme", String(e));
}

const queryClient = new QueryClient({
  defaultOptions: { queries: { retry: 1, refetchOnWindowFocus: false } },
});

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <QueryClientProvider client={queryClient}>
      <ErrorBoundary>
        <HashRouter>
          <App />
        </HashRouter>
      </ErrorBoundary>
    </QueryClientProvider>
  </React.StrictMode>,
);
