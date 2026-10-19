import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { api } from "../api";
import { ago } from "../format";
import { ErrorText, Pre, Section } from "../ui";

export function OverviewPage() {
  const qc = useQueryClient();
  const info = useQuery({ queryKey: ["info"], queryFn: () => api.info(), refetchInterval: 10_000 });
  const status = useQuery({ queryKey: ["status"], queryFn: api.status, refetchInterval: 10_000 });
  const wazuh = useQuery({ queryKey: ["wazuh-config"], queryFn: api.wazuhConfig });
  const incidents = useQuery({
    queryKey: ["incidents"],
    queryFn: api.wazuhIncidents,
    refetchInterval: 15_000,
  });
  const inventory = useQuery({ queryKey: ["inventory"], queryFn: api.inventory });
  const events = useQuery({
    queryKey: ["setup-events"],
    queryFn: () => api.setupEvents(0),
    refetchInterval: info.data?.setup_running ? 3000 : false,
  });
  const restart = useMutation({
    mutationFn: api.gatewayRestart,
    onSettled: () => qc.invalidateQueries({ queryKey: ["status"] }),
  });
  const setup = useMutation({
    mutationFn: api.setupStart,
    onSettled: () => qc.invalidateQueries({ queryKey: ["info"] }),
  });

  const s = status.data;
  const i = info.data;
  const lastIncident = incidents.data?.[0];
  const setupLines = (events.data ?? [])
    .slice(-15)
    .map(
      (e) =>
        e.event.line ??
        e.event.message ??
        `${e.event.kind}${e.event.step ? ` ${e.event.step}` : ""}`,
    )
    .join("\n");

  return (
    <>
      <h1 className="text-xl font-semibold">Обзор</h1>
      <div className="grid gap-4 lg:grid-cols-2">
        <Section
          title="Hermes Agent"
          actions={
            i?.hermes_installed ? (
              <Button
                size="sm"
                variant="outline"
                onClick={() => restart.mutate()}
                disabled={restart.isPending}
              >
                Перезапустить
              </Button>
            ) : (
              <Button
                size="sm"
                onClick={() => setup.mutate()}
                disabled={setup.isPending || i?.setup_running}
              >
                Установить
              </Button>
            )
          }
        >
          <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-2 text-sm">
            <dt className="text-muted-foreground">Состояние</dt>
            <dd>
              {!s || !i ? (
                "…"
              ) : i.setup_running ? (
                <Badge variant="secondary">устанавливается</Badge>
              ) : !i.hermes_installed ? (
                <Badge variant="outline">не установлен</Badge>
              ) : s.gateway_running ? (
                <Badge>работает</Badge>
              ) : (
                <Badge variant="destructive">остановлен</Badge>
              )}
            </dd>
            <dt className="text-muted-foreground">Версия</dt>
            <dd>{i ? `${i.pin.version} (${i.pin.commit.slice(0, 8)})` : "…"}</dd>
            <dt className="text-muted-foreground">Служба</dt>
            <dd>{i ? `${i.name} ${i.version}` : "…"}</dd>
            <dt className="text-muted-foreground">Данные</dt>
            <dd className="break-all text-xs">{i?.app_data_dir ?? "…"}</dd>
          </dl>
          <ErrorText error={restart.error ?? setup.error ?? status.error} />
          {i?.setup_running || (!i?.hermes_installed && setupLines) ? (
            <Pre>{setupLines || "…"}</Pre>
          ) : null}
        </Section>

        <Section title="Алерты Wazuh">
          <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-2 text-sm">
            <dt className="text-muted-foreground">Приём</dt>
            <dd>
              {wazuh.data ? (
                wazuh.data.enabled ? (
                  <Badge>включён · {wazuh.data.listen}</Badge>
                ) : (
                  <Badge variant="outline">выключен</Badge>
                )
              ) : (
                "…"
              )}
            </dd>
            <dt className="text-muted-foreground">Инцидентов</dt>
            <dd>{incidents.data ? incidents.data.length : "…"}</dd>
            <dt className="text-muted-foreground">Последний</dt>
            <dd>
              {lastIncident
                ? `#${lastIncident.id} · ${lastIncident.agent_name} · ${ago(lastIncident.last_unix)}`
                : "нет"}
            </dd>
            <dt className="text-muted-foreground">Хостов в инвентаре</dt>
            <dd>
              {inventory.data
                ? `${inventory.data.hosts.length} (с доступом: ${inventory.data.hosts.filter((h) => h.access !== "none").length})`
                : "…"}
            </dd>
          </dl>
        </Section>
      </div>
    </>
  );
}
