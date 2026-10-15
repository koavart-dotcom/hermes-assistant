import { useEffect, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { CheckCircle2, Circle, Loader2, XCircle } from "lucide-react";
import { PageShell } from "@/app/PageShell";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Switch } from "@/components/ui/switch";
import { formatBytes, percent } from "@/lib/format";
import { api, isTauri, onSetupEvent } from "@/services/tauri";
import { STEP_ORDER, STEP_TITLES, useSetupStore } from "@/stores/setup";
import type { SetupPlan } from "@/types/generated/SetupPlan";

export function SetupPage() {
  const plan = useQuery({ queryKey: ["setup_plan"], queryFn: api.setupPlan, enabled: isTauri() });
  const phase = useSetupStore((s) => s.phase);
  const apply = useSetupStore((s) => s.apply);

  useEffect(() => {
    if (!isTauri()) return;
    let unlisten: (() => void) | undefined;
    onSetupEvent(apply).then((u) => (unlisten = u));
    return () => unlisten?.();
  }, [apply]);

  if (!isTauri()) {
    return <PageShell title="Установка" description="Мастер доступен только внутри приложения." />;
  }
  if (plan.isPending) {
    return <PageShell title="Установка" description="Проверяю, что уже установлено…" />;
  }
  if (plan.isError) {
    return <PageShell title="Установка" description={`Не удалось получить план: ${String(plan.error)}`} />;
  }

  return (
    <PageShell title="Установка" description="Hermes Agent, Ollama и локальная модель устанавливаются приложением. Вручную ничего ставить не нужно.">
      {phase === "idle" ? <ConsentForm plan={plan.data} /> : <ProgressView />}
    </PageShell>
  );
}

function ConsentForm({ plan }: { plan: SetupPlan }) {
  const [model, setModel] = useState(plan.default_model);
  const [autostart, setAutostart] = useState(true);
  const [consent, setConsent] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const chosen = plan.models.find((m) => m.tag === model);
  const totalBytes =
    (plan.hermes_installed ? 0 : 1_000_000_000) +
    (plan.ollama_installed ? 0 : Number(plan.ollama_zip_bytes)) +
    Number(chosen?.bytes ?? 0);

  const start = async () => {
    setError(null);
    try {
      await api.setupStart({ model, consent, autostart });
    } catch (e) {
      setError(String(e));
    }
  };

  return (
    <div className="grid max-w-3xl gap-4">
      <Card>
        <CardHeader>
          <CardTitle className="text-base">Что будет скачано и установлено</CardTitle>
        </CardHeader>
        <CardContent className="grid gap-2 text-sm">
          <Row label={`Hermes Agent ${plan.hermes_version}`} value={plan.hermes_installed ? "уже установлен" : `~1 ГБ → ${plan.hermes_home}`} />
          <Row label={`Ollama ${plan.ollama_version}`} value={plan.ollama_installed ? "уже установлен" : `${formatBytes(plan.ollama_zip_bytes)} → ${plan.ollama_dir}`} />
          <Row label="Модели" value={plan.models_dir} />
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle className="text-base">Локальная модель</CardTitle>
        </CardHeader>
        <CardContent className="grid gap-2">
          {plan.models.map((m) => (
            <label key={m.tag} className="flex cursor-pointer items-start gap-3 rounded-md border p-3 text-sm has-[:checked]:border-primary has-[:checked]:bg-accent/40">
              <input type="radio" name="model" value={m.tag} checked={model === m.tag} onChange={() => setModel(m.tag)} className="mt-1" />
              <span className="grid gap-0.5">
                <span className="font-medium">
                  {m.label} <span className="text-muted-foreground">· {formatBytes(m.bytes)}</span>
                </span>
                <span className="text-muted-foreground">{m.note}</span>
              </span>
            </label>
          ))}
        </CardContent>
      </Card>

      <Card>
        <CardContent className="grid gap-4 pt-6 text-sm">
          <label className="flex items-center justify-between gap-4">
            <span>Запускать помощника при входе в Windows</span>
            <Switch checked={autostart} onCheckedChange={setAutostart} />
          </label>
          <label className="flex items-center justify-between gap-4">
            <span>
              Согласен скачать около <strong>{formatBytes(totalBytes)}</strong> из интернета
            </span>
            <Switch checked={consent} onCheckedChange={setConsent} />
          </label>
          {error ? <p className="text-destructive">{error}</p> : null}
          <div>
            <Button onClick={start} disabled={!consent}>
              Установить
            </Button>
          </div>
        </CardContent>
      </Card>
    </div>
  );
}

function Row({ label, value }: { label: string; value: string }) {
  return (
    <div className="grid grid-cols-[12rem_1fr] gap-2">
      <span className="text-muted-foreground">{label}</span>
      <span className="break-all">{value}</span>
    </div>
  );
}

function ProgressView() {
  const { phase, steps, currentStage, progress, log, error, reset } = useSetupStore();
  const pct = progress ? percent(progress.received, progress.total) : null;

  return (
    <div className="grid max-w-3xl gap-4">
      <Card>
        <CardContent className="grid gap-3 pt-6 text-sm">
          {STEP_ORDER.map((step) => {
            const st = steps[step];
            const Icon = st === "done" ? CheckCircle2 : st === "running" ? Loader2 : Circle;
            return (
              <div key={step} className="flex items-center gap-3">
                <Icon className={st === "running" ? "size-4 animate-spin text-primary" : st === "done" ? "size-4 text-primary" : "size-4 text-muted-foreground"} />
                <span className={st === "pending" ? "text-muted-foreground" : ""}>{STEP_TITLES[step]}</span>
                {st === "running" && currentStage ? <span className="text-muted-foreground">— {currentStage}</span> : null}
              </div>
            );
          })}
          {progress ? (
            <div className="grid gap-1">
              <div className="flex justify-between text-muted-foreground">
                <span>{progress.label}</span>
                <span>
                  {formatBytes(progress.received)}
                  {progress.total !== null ? ` / ${formatBytes(progress.total)}` : ""}
                  {pct !== null ? ` · ${pct}%` : ""}
                </span>
              </div>
              <div className="h-2 overflow-hidden rounded bg-muted">
                <div className="h-full bg-primary transition-[width]" style={{ width: `${pct ?? 5}%` }} />
              </div>
            </div>
          ) : null}
          {phase === "done" ? (
            <p className="flex items-center gap-2 text-primary">
              <CheckCircle2 className="size-4" /> Всё установлено. Hermes Agent и Ollama работают.
            </p>
          ) : null}
          {phase === "failed" ? (
            <div className="grid gap-2">
              <p className="flex items-center gap-2 text-destructive">
                <XCircle className="size-4" /> {error}
              </p>
              <div>
                <Button variant="outline" onClick={reset}>
                  Попробовать снова
                </Button>
              </div>
            </div>
          ) : null}
          {phase === "running" ? (
            <div>
              <Button variant="outline" onClick={() => api.setupCancel()}>
                Отменить
              </Button>
            </div>
          ) : null}
        </CardContent>
      </Card>
      <Card>
        <CardHeader>
          <CardTitle className="text-base">Журнал</CardTitle>
        </CardHeader>
        <CardContent>
          <pre className="max-h-72 overflow-auto rounded bg-muted p-3 text-xs leading-5">{log.slice(-200).join("\n")}</pre>
        </CardContent>
      </Card>
    </div>
  );
}
