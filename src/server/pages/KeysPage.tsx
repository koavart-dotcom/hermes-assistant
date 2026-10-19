import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Trash2 } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { api } from "../api";
import { ErrorText, Field, Section, Select } from "../ui";

const SERVER_KEYS = [
  "TELEGRAM_BOT_TOKEN",
  "TELEGRAM_CHAT_ID",
  "ANTHROPIC_API_KEY",
  "OPENAI_API_KEY",
];

export function KeysPage() {
  const qc = useQueryClient();
  const options = useQuery({ queryKey: ["model-options"], queryFn: api.modelOptions });
  const current = useQuery({ queryKey: ["model-current"], queryFn: api.modelCurrent });
  const keys = useQuery({ queryKey: ["env-keys"], queryFn: api.envKeys });
  const [choice, setChoice] = useState("");
  const [name, setName] = useState("");
  const [value, setValue] = useState("");

  const apply = useMutation({
    mutationFn: () => {
      const o = options.data?.find((x) => x.id === choice);
      if (!o) throw new Error("выберите модель");
      return api.modelApply({ provider: o.provider, model: o.model, base_url: o.base_url });
    },
    onSuccess: (m) => qc.setQueryData(["model-current"], m),
  });
  const setKey = useMutation({
    mutationFn: () => api.envKeySet(name.trim(), value),
    onSuccess: () => {
      setValue("");
      void qc.invalidateQueries({ queryKey: ["env-keys"] });
      void qc.invalidateQueries({ queryKey: ["model-options"] });
    },
  });
  const delKey = useMutation({
    mutationFn: (n: string) => api.envKeyRemove(n),
    onSuccess: () => void qc.invalidateQueries({ queryKey: ["env-keys"] }),
  });
  const restart = useMutation({ mutationFn: api.gatewayRestart });

  const cur = current.data;
  return (
    <>
      <h1 className="text-xl font-semibold">Модель и ключи</h1>
      <div className="grid gap-4 lg:grid-cols-2">
        <Section
          title="Модель разбора"
          description="На сервере нет локальных моделей: облачный провайдер по ключу или Jarvis Deka в сети."
        >
          <p className="text-sm">
            Сейчас:{" "}
            {cur ? (
              <span className="font-medium">
                {cur.provider} · {cur.model || "не задана"}
              </span>
            ) : (
              "…"
            )}
          </p>
          <Field label="Сменить на">
            <Select
              value={choice}
              onChange={setChoice}
              options={[
                { value: "", label: "— выберите —" },
                ...(options.data ?? [])
                  .filter((o) => o.provider !== "custom" || !o.base_url.includes("127.0.0.1"))
                  .map((o) => ({
                    value: o.id,
                    label: `${o.title}${o.ready ? "" : ` (нужен ${o.needs_key})`}`,
                  })),
              ]}
            />
          </Field>
          <ErrorText error={apply.error ?? options.error} />
          <Button size="sm" onClick={() => apply.mutate()} disabled={!choice || apply.isPending}>
            Применить и перезапустить Hermes
          </Button>
        </Section>

        <Section
          title="Ключи (.env Hermes)"
          description="Значения не показываются: только последние 4 символа. После изменения — перезапуск Hermes."
          actions={
            <Button
              size="sm"
              variant="outline"
              onClick={() => restart.mutate()}
              disabled={restart.isPending}
            >
              Перезапустить Hermes
            </Button>
          }
        >
          <ul className="space-y-1 text-sm">
            {(keys.data ?? []).map((k) => (
              <li key={k.name} className="flex items-center gap-2">
                <code className="min-w-0 flex-1 truncate">{k.name}</code>
                <Badge variant="outline">{k.masked || "пусто"}</Badge>
                <Button
                  size="icon-sm"
                  variant="ghost"
                  title="Удалить"
                  onClick={() => window.confirm(`Удалить ${k.name}?`) && delKey.mutate(k.name)}
                >
                  <Trash2 />
                </Button>
              </li>
            ))}
          </ul>
          <form
            className="grid gap-2 pt-2"
            onSubmit={(e) => {
              e.preventDefault();
              setKey.mutate();
            }}
          >
            <div className="grid grid-cols-[1fr_1fr] gap-2">
              <Input
                list="server-keys"
                placeholder="ИМЯ_КЛЮЧА"
                value={name}
                onChange={(e) => setName(e.target.value.toUpperCase())}
              />
              <Input
                type="password"
                autoComplete="off"
                placeholder="значение"
                value={value}
                onChange={(e) => setValue(e.target.value)}
              />
            </div>
            <datalist id="server-keys">
              {SERVER_KEYS.map((k) => (
                <option key={k} value={k} />
              ))}
            </datalist>
            <ErrorText error={setKey.error ?? delKey.error ?? restart.error ?? keys.error} />
            <Button size="sm" type="submit" disabled={!name.trim() || !value || setKey.isPending}>
              Сохранить ключ
            </Button>
          </form>
        </Section>
      </div>
    </>
  );
}
