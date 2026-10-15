import { useQuery } from "@tanstack/react-query";
import { PageShell } from "@/app/PageShell";
import { api, isTauri } from "@/services/tauri";

const stateLabel: Record<string, string> = {
  not_installed: "не установлен",
  stopped: "остановлен",
  running: "работает",
};

export function StatusPage() {
  const info = useQuery({ queryKey: ["app_info"], queryFn: api.appInfo, enabled: isTauri() });

  return (
    <PageShell title="Состояние" description="Приложение, Hermes Agent, локальные модели.">
      <dl className="grid max-w-md grid-cols-[auto_1fr] gap-x-6 gap-y-2 text-sm">
        <dt className="text-muted-foreground">Приложение</dt>
        <dd data-testid="app-version">
          {!isTauri()
            ? "браузерный режим (без Tauri)"
            : info.isPending
              ? "…"
              : info.isError
                ? "ошибка запроса"
                : `${info.data.name} ${info.data.version}`}
        </dd>
        <dt className="text-muted-foreground">Hermes Agent</dt>
        <dd>{info.data ? (stateLabel[info.data.hermes_state] ?? info.data.hermes_state) : "—"}</dd>
        <dt className="text-muted-foreground">Ollama</dt>
        <dd>—</dd>
      </dl>
    </PageShell>
  );
}
