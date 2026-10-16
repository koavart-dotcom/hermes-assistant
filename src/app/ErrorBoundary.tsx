import { Component, type ReactNode } from "react";
import { reportError } from "@/services/tauri";

interface State {
  error: Error | null;
}

/** Last line of defence: a render crash shows its message instead of a blank window. */
export class ErrorBoundary extends Component<{ children: ReactNode }, State> {
  state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  componentDidCatch(error: Error, info: { componentStack?: string | null }) {
    reportError("render", `${error.message}\n${info.componentStack ?? ""}`);
  }

  render() {
    if (this.state.error) {
      return (
        <div className="p-4 text-sm" role="alert">
          <p className="font-semibold">Ошибка интерфейса</p>
          <pre className="mt-2 whitespace-pre-wrap text-xs text-destructive">{this.state.error.message}</pre>
          <p className="mt-2 text-muted-foreground">Запись добавлена в журнал приложения. Закройте окно и откройте снова.</p>
        </div>
      );
    }
    return this.props.children;
  }
}
