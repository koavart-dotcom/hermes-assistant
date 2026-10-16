import { useEffect, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Brain, Download, Loader2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { api, isTauri, onSetupEvent } from "@/services/tauri";
import { useVoiceStore } from "@/stores/voice";
import type { ModelOption } from "@/types/generated/ModelOption";

function errorText(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

/** "Модель": which LLM Hermes talks to — local Ollama models, Jarvis Deka, cloud providers. */
export function ModelSection() {
  const qc = useQueryClient();
  const enabled = isTauri();
  const current = useQuery({ queryKey: ["model_current"], queryFn: api.modelCurrent, enabled });
  const options = useQuery({ queryKey: ["model_options"], queryFn: api.modelOptions, enabled });
  const [picked, setPicked] = useState<string | null>(null);
  const [modelName, setModelName] = useState<string | null>(null);
  const [pullTag, setPullTag] = useState("");
  const [pulling, setPulling] = useState<string | null>(null);
  const [pullLog, setPullLog] = useState("");
  const [error, setError] = useState<string | null>(null);

  const cur = current.data;
  const opts = options.data ?? [];
  const currentId = cur ? (opts.find((o) => o.provider === cur.provider && o.model === cur.model && (o.provider !== "custom" || o.base_url === cur.base_url))?.id ?? (opts.find((o) => o.provider === cur.provider && cur.provider !== "custom")?.id ?? null)) : null;
  const selectedId = picked ?? currentId;
  const selected = opts.find((o) => o.id === selectedId);

  // Model name for cloud providers: what the user typed, else the configured one, else the preset.
  const effectiveName = modelName ?? (selected && cur?.provider === selected.provider ? cur.model : (selected?.model ?? ""));

  // Pull progress from the shared setup event stream.
  useEffect(() => {
    if (!enabled || !pulling) return;
    let unlisten: (() => void) | undefined;
    onSetupEvent((ev) => {
      if (ev.kind === "log") setPullLog(ev.line);
      if (ev.kind === "failed") {
        setError(ev.message);
        setPulling(null);
      }
      if (ev.kind === "log" && ev.line === `model: ready ${pulling}`) {
        setPulling(null);
        setPullLog("");
        void qc.invalidateQueries({ queryKey: ["model_options"] });
      }
    }).then((u) => (unlisten = u));
    return () => unlisten?.();
  }, [enabled, pulling, qc]);

  const apply = useMutation({
    mutationFn: (o: ModelOption) =>
      api.modelApply({
        provider: o.provider,
        model: o.provider === "custom" ? o.model : effectiveName.trim() || o.model,
        base_url: o.base_url,
      }),
    onSuccess: () => {
      setError(null);
      setPicked(null);
      setModelName(null);
      void qc.invalidateQueries({ queryKey: ["model_current"] });
    },
    onError: (e) => setError(errorText(e)),
  });

  const pull = async () => {
    const tag = pullTag.trim();
    if (!tag) return;
    setError(null);
    setPulling(tag);
    setPullLog("Запрос отправлен…");
    try {
      await api.modelPull(tag);
    } catch (e) {
      setError(errorText(e));
      setPulling(null);
    }
  };

  const changed = selected && (selected.id !== currentId || (selected.provider !== "custom" && effectiveName.trim() !== (cur?.model ?? "")));

  return (
    <section className="hud-panel mt-6 max-w-xl space-y-3 p-4 text-sm" aria-labelledby="model-title">
      <h2 id="model-title" className="flex items-center gap-2 text-sm font-semibold tracking-tight">
        <Brain className="size-4" /> Модель
      </h2>
      <p className="text-muted-foreground">
        Сейчас: <span data-testid="model-current">{cur ? `${cur.model} (${cur.provider}${cur.base_url ? `, ${cur.base_url}` : ""})` : "…"}</span>.
        Смена модели перезапускает Hermes.
      </p>
      {!enabled ? <p className="text-muted-foreground">Доступно только внутри приложения.</p> : null}
      <div className="space-y-2">
        {opts.map((o) => (
          <label key={o.id} className={`flex cursor-pointer items-start gap-2 rounded-lg border p-2 ${selectedId === o.id ? "border-primary bg-primary/10" : ""}`}>
            <input type="radio" name="model" className="mt-1" checked={selectedId === o.id} onChange={() => setPicked(o.id)} />
            <span className="min-w-0 flex-1">
              <span className="font-medium">{o.title}</span>
              <span className="block text-xs text-muted-foreground">{o.hint}</span>
              {!o.ready ? <span className="block text-xs text-amber-600">Нет доступа: ключ {o.needs_key} в «Интеграции и ключи» или вход по подписке.</span> : null}
              {selectedId === o.id && o.provider !== "custom" ? (
                <Input className="mt-2 font-mono" aria-label="Имя модели" value={effectiveName} onChange={(e) => setModelName(e.target.value)} placeholder={o.model} />
              ) : null}
            </span>
          </label>
        ))}
      </div>
      <div className="flex flex-wrap items-center gap-2">
        <Button onClick={() => selected && apply.mutate(selected)} disabled={!selected || !changed || apply.isPending || !selected.ready}>
          {apply.isPending ? <Loader2 className="mr-1 size-4 animate-spin" /> : null}
          {apply.isPending ? "Применяю…" : "Применить и перезапустить Hermes"}
        </Button>
      </div>
      <div className="rounded-lg border border-border/70 bg-background/40 p-3">
        <p className="font-medium">Скачать модель Ollama</p>
        <p className="mb-2 text-xs text-muted-foreground">Например, qwen3.5:14b или gemma4:12b. Файлы идут на диск D:, прогресс ниже.</p>
        <div className="flex gap-2">
          <Input className="font-mono" aria-label="Тег модели" value={pullTag} onChange={(e) => setPullTag(e.target.value)} placeholder="qwen3.5:14b" disabled={Boolean(pulling)} />
          <Button variant="outline" onClick={() => void pull()} disabled={!pullTag.trim() || Boolean(pulling)}>
            {pulling ? <Loader2 className="mr-1 size-4 animate-spin" /> : <Download className="mr-1 size-4" />}
            Скачать
          </Button>
        </div>
        {pullLog ? <p className="mt-2 font-mono text-xs text-muted-foreground">{pullLog}</p> : null}
      </div>
      <ClaudeBridgeField />
      {error ? (
        <p className="text-destructive" role="alert">
          {error}
        </p>
      ) : null}
    </section>
  );
}

/** «Попроси Клода …»: which project of the Hermes → Claude Code bridge gets the task. */
function ClaudeBridgeField() {
  const qc = useQueryClient();
  const prefs = useQuery({ queryKey: ["prefs"], queryFn: api.prefsGet, enabled: isTauri() });
  const [typed, setTyped] = useState<string | null>(null);
  const value = typed ?? prefs.data?.claude_project ?? "";
  const save = useMutation({
    mutationFn: () => api.prefsSet({ ...prefs.data!, claude_project: value.trim() || "hermes-assistant" }),
    onSuccess: (p) => {
      setTyped(null);
      useVoiceStore.getState().setPrefs(p);
      void qc.invalidateQueries({ queryKey: ["prefs"] });
    },
  });
  return (
    <div className="rounded-lg border border-border/70 bg-background/40 p-3">
      <p className="font-medium">Мост к Claude Code</p>
      <p className="mb-2 text-xs text-muted-foreground">«Джарвис, попроси Клода …» передаёт задачу Claude Code под вашей подпиской через мост Hermes. Проект из списка моста (projects.json).</p>
      <div className="flex gap-2">
        <Input className="font-mono" aria-label="Проект для Клода" value={value} onChange={(e) => setTyped(e.target.value)} placeholder="hermes-assistant" />
        <Button variant="outline" onClick={() => save.mutate()} disabled={!prefs.data || typed === null || save.isPending}>
          Сохранить
        </Button>
      </div>
    </div>
  );
}
