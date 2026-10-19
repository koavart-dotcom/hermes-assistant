import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import type { WazuhConfig } from "@/types/generated/WazuhConfig";
import { api } from "../api";
import { ErrorText, Field, Pre, Section } from "../ui";

function Toggle({
  label,
  hint,
  checked,
  onChange,
}: {
  label: string;
  hint?: string;
  checked: boolean;
  onChange: (v: boolean) => void;
}) {
  return (
    <label className="flex items-start justify-between gap-4 text-sm">
      <span>
        <span className="font-medium">{label}</span>
        {hint ? <span className="block text-xs text-muted-foreground">{hint}</span> : null}
      </span>
      <Switch checked={checked} onCheckedChange={onChange} />
    </label>
  );
}

const list = (s: string) =>
  s
    .split(/[,\s]+/)
    .map((x) => x.trim())
    .filter(Boolean);

export function WazuhPage() {
  const config = useQuery({ queryKey: ["wazuh-config"], queryFn: api.wazuhConfig });
  if (!config.data) {
    return (
      <>
        <h1 className="text-xl font-semibold">Wazuh</h1>
        <ErrorText error={config.error} />
      </>
    );
  }
  return <WazuhForm initial={config.data} />;
}

function WazuhForm({ initial }: { initial: WazuhConfig }) {
  const qc = useQueryClient();
  const [c, setC] = useState<WazuhConfig>(initial);
  const [ips, setIps] = useState(initial.allowed_ips.join(", "));
  const [rules, setRules] = useState(initial.ignore_rules.join(", "));
  const [showKey, setShowKey] = useState(false);
  const integration = useQuery({
    queryKey: ["wazuh-integration"],
    queryFn: api.wazuhIntegration,
    enabled: showKey,
  });

  const save = useMutation({
    mutationFn: (next: WazuhConfig) => api.wazuhConfigSave(next),
    onSuccess: (r) => {
      setC(r.config);
      qc.setQueryData(["wazuh-config"], r.config);
      void qc.invalidateQueries({ queryKey: ["wazuh-integration"] });
    },
  });

  const set = <K extends keyof WazuhConfig>(k: K, v: WazuhConfig[K]) => setC({ ...c, [k]: v });
  const num = (v: string) => Number(v.replace(/\D/g, "")) || 0;

  return (
    <>
      <h1 className="text-xl font-semibold">Wazuh</h1>
      <form
        className="grid gap-4 lg:grid-cols-2"
        onSubmit={(e) => {
          e.preventDefault();
          save.mutate({ ...c, allowed_ips: list(ips), ignore_rules: list(rules) });
        }}
      >
        <Section
          title="Приём алертов"
          description="Наружу открыт только этот адрес; применяется сразу после сохранения."
        >
          <Toggle
            label="Принимать алерты"
            checked={c.enabled}
            onChange={(v) => set("enabled", v)}
          />
          <Field label="Адрес приёма" hint="host:port, обычно 0.0.0.0:8646">
            <Input value={c.listen} onChange={(e) => set("listen", e.target.value)} />
          </Field>
          <Field
            label="Разрешённые адреса"
            hint="IP Wazuh manager через запятую. Пусто — с любых (не оставлять так)."
          >
            <Input value={ips} onChange={(e) => setIps(e.target.value)} placeholder="10.0.0.2" />
          </Field>
          {c.enabled && list(ips).length === 0 ? (
            <p className="text-sm text-[var(--warning)]">
              Приём открыт для любых адресов — укажите IP manager'а.
            </p>
          ) : null}
        </Section>

        <Section title="Что разбирать">
          <div className="grid grid-cols-2 gap-3">
            <Field label="Минимальный уровень" hint="Ниже — только сохраняется">
              <Input
                inputMode="numeric"
                value={c.min_level}
                onChange={(e) => set("min_level", num(e.target.value))}
              />
            </Field>
            <Field label="Окно склейки, мин" hint="Тот же хост и правило">
              <Input
                inputMode="numeric"
                value={c.window_min}
                onChange={(e) => set("window_min", num(e.target.value) || 1)}
              />
            </Field>
          </div>
          <Field label="Игнорировать правила" hint="id через запятую">
            <Input value={rules} onChange={(e) => setRules(e.target.value)} />
          </Field>
          <Toggle
            label="Разбор Hermes"
            hint="Модель пишет причину и что сделать"
            checked={c.analyze}
            onChange={(v) => set("analyze", v)}
          />
          <Toggle
            label="Проверки на хосте"
            hint="Перед разбором — команды только на чтение на машине из инвентаря"
            checked={c.diagnose}
            onChange={(v) => set("diagnose", v)}
          />
          <Field label="Разборов в час, не больше">
            <Input
              inputMode="numeric"
              value={c.max_analyses_per_hour}
              onChange={(e) => set("max_analyses_per_hour", num(e.target.value))}
            />
          </Field>
        </Section>

        <Section title="Telegram">
          <Toggle
            label="Слать в Telegram"
            checked={c.telegram}
            onChange={(v) => set("telegram", v)}
          />
          <Field
            label="Чат для алертов"
            hint="Пусто — TELEGRAM_CHAT_ID из ключей. Бот — TELEGRAM_BOT_TOKEN."
          >
            <Input
              value={c.telegram_chat_id}
              onChange={(e) => set("telegram_chat_id", e.target.value)}
            />
          </Field>
        </Section>

        <Section title="Сохранение">
          <ErrorText error={save.error} />
          {save.data ? (
            <p className="text-sm text-muted-foreground">
              Сохранено. Приём {save.data.listening ? "работает" : "выключен"}.
            </p>
          ) : null}
          <Button type="submit" disabled={save.isPending}>
            Сохранить
          </Button>
        </Section>
      </form>

      <Section
        title="Настройка Wazuh manager"
        description="Скрипт tools/wazuh/custom-hermes → /var/ossec/integrations/, блок ниже → /var/ossec/etc/ossec.conf, затем перезапуск wazuh-manager. Подробно — docs/WAZUH.md."
        actions={
          <Button size="sm" variant="outline" onClick={() => setShowKey(!showKey)}>
            {showKey ? "Скрыть" : "Показать блок с ключом"}
          </Button>
        }
      >
        <ErrorText error={integration.error} />
        {showKey && integration.data ? <Pre>{integration.data.ossec_conf}</Pre> : null}
      </Section>
    </>
  );
}
