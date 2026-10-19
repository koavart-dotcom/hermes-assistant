import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import type { Host } from "@/types/generated/Host";
import type { Incident } from "@/types/generated/Incident";
import { FixDialog } from "../FixDialog";
import { matchHost } from "../matchHost";
import { api, sampleAlert } from "../api";
import { ago } from "../format";
import { ErrorText, Pre, Section } from "../ui";

function StateBadge({ state }: { state: string }) {
  if (state === "done") return <Badge>разобран</Badge>;
  if (state === "running") return <Badge variant="secondary">разбирается</Badge>;
  if (state === "pending") return <Badge variant="secondary">в очереди</Badge>;
  if (state === "failed") return <Badge variant="destructive">ошибка разбора</Badge>;
  if (state === "off") return <Badge variant="outline">без разбора</Badge>;
  return <Badge variant="outline">{state.replace(/^skipped: /, "пропущен: ")}</Badge>;
}

function levelClass(level: number) {
  if (level >= 12) return "text-destructive";
  if (level >= 8) return "text-[var(--warning)]";
  return "text-muted-foreground";
}

function Row({ inc, hosts }: { inc: Incident; hosts: Host[] }) {
  const [open, setOpen] = useState(false);
  const [fixing, setFixing] = useState(false);
  const host = matchHost(hosts, inc.agent_name, inc.agent_ip);
  return (
    <li className="rounded-lg border border-border">
      <button
        type="button"
        className="flex w-full flex-wrap items-center gap-x-3 gap-y-1 p-3 text-left text-sm hover:bg-muted/40"
        onClick={() => setOpen(!open)}
      >
        <span className={`font-mono font-semibold ${levelClass(inc.level)}`}>L{inc.level}</span>
        <span className="font-medium">#{inc.id}</span>
        <span>{inc.agent_name}</span>
        <span className="min-w-0 flex-1 truncate text-muted-foreground">
          {inc.rule_id}: {inc.description}
        </span>
        {inc.security ? <Badge variant="destructive">безопасность</Badge> : null}
        {inc.count > 1 ? <Badge variant="outline">×{inc.count}</Badge> : null}
        <StateBadge state={inc.analysis_state} />
        <span className="text-xs text-muted-foreground">{ago(inc.last_unix)}</span>
      </button>
      {open ? (
        <div className="space-y-2 border-t border-border p-3 text-sm">
          <p className="text-muted-foreground">
            Хост {inc.agent_name}
            {inc.agent_ip ? ` (${inc.agent_ip})` : ""} · первый алерт {ago(inc.first_unix)} ·
            повторов {inc.count}
          </p>
          {inc.diag ? <p>🔎 {inc.diag}</p> : null}
          {inc.analysis ? (
            <Pre>{inc.analysis}</Pre>
          ) : (
            <p className="text-muted-foreground">Разбора пока нет.</p>
          )}
          {inc.security ? (
            <p className="text-sm text-muted-foreground">
              Алерт безопасности: починка из панели отключена, только эскалация людям.
            </p>
          ) : host?.fix && host.access !== "none" ? (
            <Button size="sm" variant="outline" onClick={() => setFixing(true)}>
              Починить {host.name}
            </Button>
          ) : (
            <p className="text-xs text-muted-foreground">
              {host
                ? "Починка для этого хоста не разрешена (инвентарь)."
                : "Хоста нет в инвентаре — починка недоступна."}
            </p>
          )}
          {fixing && host ? (
            <FixDialog host={host} incidentId={inc.id} onClose={() => setFixing(false)} />
          ) : null}
        </div>
      ) : null}
    </li>
  );
}

export function IncidentsPage() {
  const qc = useQueryClient();
  const [host, setHost] = useState("test-host");
  const incidents = useQuery({
    queryKey: ["incidents"],
    queryFn: api.wazuhIncidents,
    refetchInterval: 5000,
  });
  const inventory = useQuery({ queryKey: ["inventory"], queryFn: api.inventory });
  const test = useMutation({
    mutationFn: () => api.wazuhTest(sampleAlert(host)),
    onSettled: () => qc.invalidateQueries({ queryKey: ["incidents"] }),
  });
  const list = incidents.data ?? [];
  return (
    <>
      <h1 className="text-xl font-semibold">Инциденты</h1>
      <Section
        title="Последние 200"
        description="Повторы того же хоста и правила за окно склейки — один инцидент. Нажмите на строку, чтобы увидеть разбор."
        actions={
          <div className="flex items-center gap-2">
            <Input
              className="h-7 w-36"
              value={host}
              onChange={(e) => setHost(e.target.value)}
              placeholder="хост"
            />
            <Button
              size="sm"
              variant="outline"
              onClick={() => test.mutate()}
              disabled={test.isPending}
            >
              Тестовый алерт
            </Button>
          </div>
        }
      >
        <ErrorText error={incidents.error ?? test.error} />
        {test.data ? (
          <p className="text-sm text-muted-foreground">
            Принято: новых {test.data.new_incidents.length}, повторов {test.data.repeats},
            отфильтровано {test.data.filtered}.
          </p>
        ) : null}
        {list.length === 0 ? (
          <p className="text-sm text-muted-foreground">
            Инцидентов нет. Список хранится в памяти службы до перезапуска; все алерты — в
            alerts.jsonl.
          </p>
        ) : (
          <ul className="space-y-2">
            {list.map((i) => (
              <Row key={i.id} inc={i} hosts={inventory.data?.hosts ?? []} />
            ))}
          </ul>
        )}
      </Section>
    </>
  );
}
