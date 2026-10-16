import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Link } from "react-router";
import { PageShell } from "@/app/PageShell";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { api, isTauri } from "@/services/tauri";

export function StatusPage() {
  const qc = useQueryClient();
  const info = useQuery({ queryKey: ["app_info"], queryFn: api.appInfo, enabled: isTauri() });
  const status = useQuery({
    queryKey: ["status_snapshot"],
    queryFn: api.statusSnapshot,
    enabled: isTauri(),
    refetchInterval: 5000,
  });
  const invalidate = () => qc.invalidateQueries({ queryKey: ["status_snapshot"] });
  const start = useMutation({ mutationFn: api.gatewayStart, onSettled: invalidate });
  const stop = useMutation({ mutationFn: api.gatewayStop, onSettled: invalidate });

  if (!isTauri()) {
    return (
      <PageShell title="Состояние" description="Приложение, Hermes Agent, локальные модели.">
        <p className="text-sm" data-testid="app-version">
          браузерный режим (без Tauri)
        </p>
      </PageShell>
    );
  }

  const s = status.data;
  return (
    <PageShell title="Состояние" description="Приложение, Hermes Agent, локальные модели.">
      <dl className="hud-panel grid max-w-xl grid-cols-[auto_1fr] gap-x-6 gap-y-3 p-4 text-sm">
        <dt className="text-muted-foreground">Приложение</dt>
        <dd data-testid="app-version">{info.data ? `${info.data.name} ${info.data.version}` : "…"}</dd>

        <dt className="text-muted-foreground">Hermes Agent</dt>
        <dd className="flex flex-wrap items-center gap-2">
          {s ? (
            <>
              <Badge variant={s.gateway_running ? "default" : s.hermes_installed ? "secondary" : "outline"}>
                {s.gateway_running ? "работает" : s.hermes_installed ? "остановлен" : "не установлен"}
              </Badge>
              <span className="text-muted-foreground">{s.hermes_version}</span>
              {s.hermes_installed ? (
                s.gateway_running ? (
                  <Button size="sm" variant="outline" onClick={() => stop.mutate()} disabled={stop.isPending}>
                    Остановить
                  </Button>
                ) : (
                  <Button size="sm" onClick={() => start.mutate()} disabled={start.isPending}>
                    Запустить
                  </Button>
                )
              ) : (
                <Button size="sm" asChild>
                  <Link to="/setup">Установить</Link>
                </Button>
              )}
            </>
          ) : (
            "…"
          )}
        </dd>

        <dt className="text-muted-foreground">Ollama</dt>
        <dd className="flex items-center gap-2">
          {s ? (
            <Badge variant={s.ollama_running ? "default" : s.ollama_installed ? "secondary" : "outline"}>
              {s.ollama_running ? "работает" : s.ollama_installed ? "остановлена" : "не установлена"}
            </Badge>
          ) : (
            "…"
          )}
        </dd>

        <dt className="text-muted-foreground">Модели</dt>
        <dd>{s ? (s.models.length ? s.models.join(", ") : "нет") : "…"}</dd>

        <dt className="text-muted-foreground">Папка Hermes</dt>
        <dd className="break-all">{s?.hermes_home ?? "…"}</dd>

        <dt className="text-muted-foreground">Папка моделей</dt>
        <dd className="break-all">{s?.models_dir ?? "…"}</dd>
      </dl>
      {start.isError ? <p className="mt-4 text-sm text-destructive">{String(start.error)}</p> : null}
      {stop.isError ? <p className="mt-4 text-sm text-destructive">{String(stop.error)}</p> : null}
    </PageShell>
  );
}
