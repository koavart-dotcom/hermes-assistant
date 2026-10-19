import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import { api } from "./api";
import { ago } from "./format";
import { ErrorText, Section } from "./ui";

/** Self-update of the service: version, auto-update, check / install now. */
export function UpdateSection() {
  const qc = useQueryClient();
  const status = useQuery({
    queryKey: ["update"],
    queryFn: api.updateStatus,
    refetchInterval: (q) => (q.state.data && q.state.data.state !== "idle" ? 2000 : 60_000),
    retry: true,
  });
  const set = (data: Awaited<ReturnType<typeof api.updateStatus>>) =>
    qc.setQueryData(["update"], data);
  const check = useMutation({ mutationFn: api.updateCheck, onSuccess: set });
  const apply = useMutation({ mutationFn: api.updateApply, onSuccess: set });
  const auto = useMutation({
    mutationFn: (v: boolean) => api.updateConfig(v, status.data?.check_hours ?? 6),
    onSuccess: set,
  });
  const s = status.data;
  const busy = s && s.state !== "idle";
  return (
    <Section
      title="Обновление службы"
      description="Служба сама проверяет релизы, скачивает подписанный пакет, проверяет подпись и перезапускается (~30 с). Hermes Agent при этом продолжает работать."
      actions={
        <>
          <Button
            size="sm"
            variant="outline"
            onClick={() => check.mutate()}
            disabled={check.isPending || busy}
          >
            Проверить
          </Button>
          {s?.available ? (
            <Button
              size="sm"
              onClick={() =>
                window.confirm(`Обновить службу до ${s.latest}? Она перезапустится.`) &&
                apply.mutate()
              }
              disabled={apply.isPending || busy || !s.supported}
            >
              Обновить до {s.latest}
            </Button>
          ) : null}
        </>
      }
    >
      {s ? (
        <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-2 text-sm">
          <dt className="text-muted-foreground">Версия</dt>
          <dd className="flex flex-wrap items-center gap-2">
            {s.current}
            {s.available ? (
              <Badge>доступна {s.latest}</Badge>
            ) : s.latest ? (
              <Badge variant="outline">последняя</Badge>
            ) : null}
            {busy ? <Badge variant="secondary">{s.state}</Badge> : null}
          </dd>
          <dt className="text-muted-foreground">Проверка</dt>
          <dd>
            {s.checked_unix ? ago(s.checked_unix) : "ещё не было"} · раз в {s.check_hours} ч
          </dd>
          <dt className="text-muted-foreground">Автообновление</dt>
          <dd>
            <Switch
              checked={s.auto}
              onCheckedChange={(v) => auto.mutate(v)}
              disabled={auto.isPending}
            />
          </dd>
        </dl>
      ) : null}
      {s && !s.supported ? (
        <p className="text-xs text-muted-foreground">
          Обновление работает только у службы, установленной через hermesd install.
        </p>
      ) : null}
      {s?.state === "restarting" ? (
        <p className="text-sm">
          Служба перезапускается — страница оживёт сама примерно через 30–60 секунд.
        </p>
      ) : null}
      <ErrorText
        error={s?.error ? new Error(s.error) : (check.error ?? apply.error ?? auto.error)}
      />
    </Section>
  );
}
