import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Eye, EyeOff, KeyRound, Loader2, RefreshCw, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { api, isTauri } from "@/services/tauri";
import type { EnvKey } from "@/types/generated/EnvKey";
import type { EnvPreset } from "@/types/generated/EnvPreset";

function errorText(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

const NAME_RE = /^[A-Z][A-Z0-9_]{1,63}$/;

/**
 * "Интеграции и ключи": API keys for Hermes skills/tools (FreeScout, Telegram,
 * cloud providers, anything else) stored in Hermes' own `.env`. Values are
 * never read back — only `••••last4`. The gateway reads `.env` at start, so a
 * restart button appears after a change.
 */
export function EnvKeysSection() {
  const qc = useQueryClient();
  const enabled = isTauri();
  const keys = useQuery({ queryKey: ["env_keys"], queryFn: api.envKeysList, enabled });
  const presets = useQuery({
    queryKey: ["env_presets"],
    queryFn: api.envKeysPresets,
    enabled,
    staleTime: Infinity,
  });
  const [dirty, setDirty] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refresh = () => qc.invalidateQueries({ queryKey: ["env_keys"] });

  const save = useMutation({
    mutationFn: ({ name, value }: { name: string; value: string }) => api.envKeysSet(name, value),
    onSuccess: () => {
      setDirty(true);
      setError(null);
      void refresh();
    },
    onError: (e) => setError(errorText(e)),
  });
  const remove = useMutation({
    mutationFn: (name: string) => api.envKeysRemove(name),
    onSuccess: () => {
      setDirty(true);
      setError(null);
      void refresh();
    },
    onError: (e) => setError(errorText(e)),
  });
  const restart = useMutation({
    mutationFn: api.gatewayRestart,
    onSuccess: () => {
      setDirty(false);
      setError(null);
    },
    onError: (e) => setError(errorText(e)),
  });

  const byName = new Map((keys.data ?? []).map((k) => [k.name, k] as const));
  const presetKeys = new Set((presets.data ?? []).flatMap((p) => p.keys));
  const others = (keys.data ?? []).filter(
    (k) =>
      !presetKeys.has(k.name) && !k.name.startsWith("API_SERVER_") && !k.name.startsWith("HERMES_"),
  );

  return (
    <section className="mt-8 max-w-xl space-y-3 text-sm" aria-labelledby="env-keys-title">
      <h2 id="env-keys-title" className="flex items-center gap-2 font-semibold">
        <KeyRound className="size-4" /> Интеграции и ключи
      </h2>
      <p className="text-muted-foreground">
        Ключи доступа к сервисам, которыми пользуется помощник (хелпдеск, Telegram, облачные
        модели). Хранятся в файле настроек Hermes, доступном только вашему пользователю Windows.
        Значение после сохранения не показывается — только последние четыре символа.
      </p>
      {!enabled ? (
        <p className="text-muted-foreground">Доступно только внутри приложения.</p>
      ) : null}

      {(presets.data ?? []).map((p) => (
        <PresetCard
          key={p.id}
          preset={p}
          byName={byName}
          busy={save.isPending || remove.isPending}
          onSave={(n, v) => save.mutate({ name: n, value: v })}
          onRemove={(n) => remove.mutate(n)}
        />
      ))}

      <div className="rounded-lg border p-3">
        <p className="font-medium">Другой ключ</p>
        <p className="mb-2 text-xs text-muted-foreground">
          Имя переменной заглавными буквами, как её ждёт skill или инструмент Hermes (например,
          MY_SERVICE_TOKEN).
        </p>
        {others.map((k) => (
          <KeyRow
            key={k.name}
            name={k.name}
            current={k}
            busy={save.isPending || remove.isPending}
            onSave={(v) => save.mutate({ name: k.name, value: v })}
            onRemove={() => remove.mutate(k.name)}
          />
        ))}
        <CustomKeyRow busy={save.isPending} onSave={(n, v) => save.mutate({ name: n, value: v })} />
      </div>

      {dirty ? (
        <div
          className="flex flex-wrap items-center gap-2 rounded-lg border border-amber-500/50 bg-amber-500/10 p-3"
          role="status"
        >
          <span>Ключи сохранены. Чтобы Hermes их увидел, перезапустите его.</span>
          <Button size="sm" onClick={() => restart.mutate()} disabled={restart.isPending}>
            {restart.isPending ? (
              <Loader2 className="mr-1 size-4 animate-spin" />
            ) : (
              <RefreshCw className="mr-1 size-4" />
            )}
            Перезапустить Hermes
          </Button>
        </div>
      ) : null}
      {error ? (
        <p className="text-destructive" role="alert">
          {error}
        </p>
      ) : null}
    </section>
  );
}

function PresetCard({
  preset,
  byName,
  busy,
  onSave,
  onRemove,
}: {
  preset: EnvPreset;
  byName: Map<string, EnvKey>;
  busy: boolean;
  onSave: (name: string, value: string) => void;
  onRemove: (name: string) => void;
}) {
  const configured = preset.keys.every((k) => byName.get(k)?.masked);
  return (
    <div className="rounded-lg border p-3" data-testid={`preset-${preset.id}`}>
      <p className="flex items-center gap-2 font-medium">
        {preset.title}
        <span className={configured ? "text-xs text-emerald-600" : "text-xs text-muted-foreground"}>
          {configured ? "настроено" : "не задано"}
        </span>
      </p>
      <p className="mb-2 text-xs text-muted-foreground">{preset.hint}</p>
      {preset.keys.map((name) => (
        <KeyRow
          key={name}
          name={name}
          current={byName.get(name)}
          busy={busy}
          onSave={(v) => onSave(name, v)}
          onRemove={() => onRemove(name)}
        />
      ))}
    </div>
  );
}

function KeyRow({
  name,
  current,
  busy,
  onSave,
  onRemove,
}: {
  name: string;
  current?: EnvKey;
  busy: boolean;
  onSave: (value: string) => void;
  onRemove: () => void;
}) {
  const [value, setValue] = useState("");
  const [show, setShow] = useState(false);
  const submit = () => {
    const v = value.trim();
    if (!v) return;
    onSave(v);
    setValue("");
  };
  return (
    <div className="mb-2 flex flex-wrap items-center gap-2">
      <label className="w-44 shrink-0 font-mono text-xs" htmlFor={`env-${name}`}>
        {name}
      </label>
      <Input
        id={`env-${name}`}
        type={show ? "text" : "password"}
        autoComplete="off"
        spellCheck={false}
        className="min-w-40 flex-1 font-mono"
        placeholder={current?.masked ? current.masked : "не задано"}
        value={value}
        onChange={(e) => setValue(e.target.value)}
        onKeyDown={(e) => e.key === "Enter" && submit()}
        disabled={busy}
      />
      <Button
        size="icon"
        variant="ghost"
        onClick={() => setShow(!show)}
        aria-label={show ? "Скрыть" : "Показать"}
        type="button"
      >
        {show ? <EyeOff className="size-4" /> : <Eye className="size-4" />}
      </Button>
      <Button size="sm" onClick={submit} disabled={busy || !value.trim()} type="button">
        Сохранить
      </Button>
      {current?.masked ? (
        <Button
          size="icon"
          variant="ghost"
          onClick={onRemove}
          disabled={busy}
          aria-label={`Удалить ${name}`}
          type="button"
        >
          <Trash2 className="size-4" />
        </Button>
      ) : null}
    </div>
  );
}

function CustomKeyRow({
  busy,
  onSave,
}: {
  busy: boolean;
  onSave: (name: string, value: string) => void;
}) {
  const [name, setName] = useState("");
  const [value, setValue] = useState("");
  const valid = NAME_RE.test(name) && value.trim().length > 0;
  return (
    <div className="flex flex-wrap items-center gap-2">
      <Input
        aria-label="Имя переменной"
        className="w-44 font-mono uppercase"
        placeholder="MY_SERVICE_TOKEN"
        value={name}
        onChange={(e) => setName(e.target.value.toUpperCase().replace(/[^A-Z0-9_]/g, ""))}
        disabled={busy}
      />
      <Input
        aria-label="Значение"
        type="password"
        autoComplete="off"
        className="min-w-40 flex-1 font-mono"
        placeholder="значение"
        value={value}
        onChange={(e) => setValue(e.target.value)}
        disabled={busy}
      />
      <Button
        size="sm"
        type="button"
        disabled={busy || !valid}
        onClick={() => {
          onSave(name, value.trim());
          setName("");
          setValue("");
        }}
      >
        Добавить
      </Button>
    </div>
  );
}
