import { useState } from "react";
import { useMutation, useQuery } from "@tanstack/react-query";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import type { Host } from "@/types/generated/Host";
import { api } from "./api";
import { DiagResult } from "./DiagResult";
import { ErrorText, Field, Pre, Select } from "./ui";

/**
 * A fix on one host: choose an action from the service's catalogue, see the exact
 * command, confirm, see the output and the check afterwards. The server checks the
 * same locks again (catalogue, host `fix`, `fix_services`, security incidents).
 */
export function FixDialog({
  host,
  incidentId,
  onClose,
}: {
  host: Host;
  incidentId?: number;
  onClose: () => void;
}) {
  const actions = useQuery({ queryKey: ["fix-actions"], queryFn: api.fixActions });
  const own = (actions.data ?? []).filter((a) => a.os === host.os);
  const [actionId, setActionId] = useState("");
  const [service, setService] = useState(host.fix_services[0] ?? "");
  const [checked, setChecked] = useState(false);
  const act = own.find((a) => a.id === actionId);
  const request = {
    host: host.name,
    action: actionId,
    service: act?.needs_service ? service : null,
    incident_id: incidentId ?? null,
  };
  const preview = useQuery({
    queryKey: ["fix-preview", request],
    queryFn: () => api.fixPreview(request),
    enabled: Boolean(act) && (!act?.needs_service || Boolean(service)),
    retry: false,
  });
  const run = useMutation({ mutationFn: () => api.fixRun(request) });
  const report = run.data;

  return (
    <Dialog open onOpenChange={(o) => !o && !run.isPending && onClose()}>
      <DialogContent className="max-h-[90vh] overflow-y-auto sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle>Починка · {host.name}</DialogTitle>
          <DialogDescription>
            Только действия из каталога службы. Перед запуском видна точная команда; результат
            пишется в журнал fix.jsonl и уходит в Telegram.
            {incidentId ? ` Инцидент #${incidentId}.` : ""}
          </DialogDescription>
        </DialogHeader>

        {report ? (
          <div className="space-y-3">
            <p className="flex items-center gap-2 text-sm">
              {report.ok ? <Badge>выполнено</Badge> : <Badge variant="destructive">ошибка</Badge>}
              {report.plan.title}
              {report.plan.service ? ` (${report.plan.service})` : ""} за {report.duration_ms} мс
              {report.exit_code !== null ? `, код ${report.exit_code}` : ""}
            </p>
            <Pre>{report.output || "(нет вывода)"}</Pre>
            {report.verify ? (
              <div className="space-y-1">
                <p className="text-sm font-medium">Проверка после</p>
                <DiagResult report={report.verify} />
              </div>
            ) : null}
            <DialogFooter>
              <Button onClick={onClose}>Закрыть</Button>
            </DialogFooter>
          </div>
        ) : (
          <div className="grid gap-3">
            <Field label="Действие">
              <Select
                value={actionId}
                onChange={(v) => {
                  setActionId(v);
                  setChecked(false);
                }}
                options={[
                  { value: "", label: "— выберите —" },
                  ...own.map((a) => ({ value: a.id, label: a.title })),
                ]}
              />
            </Field>
            {act?.needs_service ? (
              host.fix_services.length ? (
                <Field
                  label="Служба"
                  hint="Список задаётся в инвентаре и в правилах на самой машине."
                >
                  <Select
                    value={service}
                    onChange={(v) => {
                      setService(v);
                      setChecked(false);
                    }}
                    options={host.fix_services.map((s) => ({ value: s, label: s }))}
                  />
                </Field>
              ) : (
                <p className="text-sm text-destructive">
                  У хоста нет служб для перезапуска — добавьте их в инвентаре.
                </p>
              )
            ) : null}
            {act ? <p className="text-sm text-muted-foreground">{act.description}</p> : null}
            <ErrorText error={preview.error ?? actions.error} />
            {preview.data ? (
              <div className="space-y-2">
                <p className="text-sm">
                  Где: <span className="font-mono text-xs">{preview.data.via}</span>
                </p>
                <Pre>{preview.data.command}</Pre>
                <label className="flex items-center gap-2 text-sm">
                  <input
                    type="checkbox"
                    checked={checked}
                    onChange={(e) => setChecked(e.target.checked)}
                  />
                  Я проверил команду и хост
                </label>
              </div>
            ) : null}
            <ErrorText error={run.error} />
            <DialogFooter>
              <Button variant="ghost" onClick={onClose} disabled={run.isPending}>
                Отмена
              </Button>
              <Button
                variant="destructive"
                disabled={!preview.data || !checked || run.isPending}
                onClick={() => run.mutate()}
              >
                {run.isPending ? "Выполняю…" : "Выполнить"}
              </Button>
            </DialogFooter>
          </div>
        )}
      </DialogContent>
    </Dialog>
  );
}
