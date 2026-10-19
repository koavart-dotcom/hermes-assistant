import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Trash2 } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import type { NetworkConfig } from "@/types/generated/NetworkConfig";
import type { Role } from "@/types/generated/Role";
import { api } from "../api";
import { ago } from "../format";
import { ErrorText, Field, Pre, Section, Select } from "../ui";

const list = (s: string) =>
  s
    .split(/[,\s]+/)
    .map((x) => x.trim())
    .filter(Boolean);

function Keys() {
  const qc = useQueryClient();
  const keys = useQuery({ queryKey: ["access-keys"], queryFn: api.accessKeys });
  const [name, setName] = useState("");
  const [role, setRole] = useState<Role>("read");
  const [created, setCreated] = useState<{ name: string; key: string } | null>(null);
  const create = useMutation({
    mutationFn: () => api.accessKeyCreate(name.trim(), role),
    onSuccess: (r) => {
      setCreated({ name: r.info.name, key: r.key });
      setName("");
      void qc.invalidateQueries({ queryKey: ["access-keys"] });
    },
  });
  const del = useMutation({
    mutationFn: (id: string) => api.accessKeyDelete(id),
    onSuccess: () => void qc.invalidateQueries({ queryKey: ["access-keys"] }),
  });
  return (
    <Section
      title="Ключи доступа"
      description="Свой ключ каждому человеку: «чтение» — только смотреть, «администратор» — менять настройки и чинить. По сети работают только эти ключи; в журнале починок виден владелец ключа."
    >
      <ul className="space-y-1 text-sm">
        {(keys.data ?? []).map((k) => (
          <li key={k.id} className="flex items-center gap-2">
            <span className="min-w-0 flex-1 truncate font-medium">{k.name}</span>
            <Badge variant={k.role === "admin" ? "default" : "secondary"}>
              {k.role === "admin" ? "администратор" : "чтение"}
            </Badge>
            <span className="text-xs text-muted-foreground">{ago(k.created_unix)}</span>
            <Button
              size="icon-sm"
              variant="ghost"
              title="Отозвать"
              onClick={() => window.confirm(`Отозвать ключ «${k.name}»?`) && del.mutate(k.id)}
            >
              <Trash2 />
            </Button>
          </li>
        ))}
        {keys.data?.length === 0 ? <li className="text-muted-foreground">Ключей нет.</li> : null}
      </ul>
      <form
        className="grid gap-2 sm:grid-cols-[1fr_auto_auto]"
        onSubmit={(e) => {
          e.preventDefault();
          create.mutate();
        }}
      >
        <Input placeholder="Кому (имя)" value={name} onChange={(e) => setName(e.target.value)} />
        <Select
          value={role}
          onChange={(v) => setRole(v as Role)}
          options={[
            { value: "read", label: "чтение" },
            { value: "admin", label: "администратор" },
          ]}
        />
        <Button type="submit" disabled={!name.trim() || create.isPending}>
          Создать ключ
        </Button>
      </form>
      <ErrorText error={keys.error ?? create.error ?? del.error} />
      {created ? (
        <div className="space-y-1">
          <p className="text-sm">
            Ключ для «{created.name}» — <b>показывается один раз</b>, передайте его владельцу
            защищённым каналом:
          </p>
          <Pre>{created.key}</Pre>
        </div>
      ) : null}
    </Section>
  );
}

function NetworkForm({ initial }: { initial: NetworkConfig }) {
  const qc = useQueryClient();
  const [c, setC] = useState<NetworkConfig>(initial);
  const [allowed, setAllowed] = useState(initial.allowed.join(", "));
  const [names, setNames] = useState(initial.hostnames.join(", "));
  const status = useQuery({
    queryKey: ["network-status"],
    queryFn: api.networkStatus,
    refetchInterval: 10_000,
  });
  const save = useMutation({
    mutationFn: () =>
      api.networkConfigSave({ ...c, allowed: list(allowed), hostnames: list(names) }),
    onSuccess: (st) => {
      qc.setQueryData(["network-status"], st);
      void qc.invalidateQueries({ queryKey: ["network-config"] });
    },
  });
  const st = status.data;
  const set = <K extends keyof NetworkConfig>(k: K, v: NetworkConfig[K]) => setC({ ...c, [k]: v });
  return (
    <Section
      title="Доступ по сети (HTTPS)"
      description="Та же панель по HTTPS с других машин. Только из перечисленных сетей и только по ключам доступа; мастер-ключ по сети не принимается."
    >
      <div className="text-sm">
        {st?.listening ? (
          <p className="flex flex-wrap items-center gap-2">
            <Badge>работает</Badge>
            <a className="text-primary underline-offset-4 hover:underline" href={st.url ?? "#"}>
              {st.url}
            </a>
          </p>
        ) : (
          <Badge variant="outline">выключен</Badge>
        )}
        {st?.error ? <p className="mt-1 text-destructive">{st.error}</p> : null}
        {st?.fingerprint ? (
          <div className="mt-2 space-y-1">
            <p className="text-muted-foreground">
              Отпечаток сертификата (SHA-256)
              {st.self_signed ? ", самоподписанный — сверьте при первом входе:" : ":"}
            </p>
            <Pre className="text-[11px]">{st.fingerprint}</Pre>
            {st.self_signed && st.names.length ? (
              <p className="text-xs text-muted-foreground">
                Действителен для: {st.names.join(", ")}
              </p>
            ) : null}
          </div>
        ) : null}
      </div>
      <form
        className="grid gap-3"
        onSubmit={(e) => {
          e.preventDefault();
          save.mutate();
        }}
      >
        <label className="flex items-center justify-between gap-4 text-sm">
          <span className="font-medium">Включить HTTPS</span>
          <Switch checked={c.enabled} onCheckedChange={(v) => set("enabled", v)} />
        </label>
        <Field
          label="Адрес"
          hint="host:port, обычно 0.0.0.0:8443. Порт откройте в брандмауэре Windows только для этих сетей."
        >
          <Input value={c.listen} onChange={(e) => set("listen", e.target.value)} />
        </Field>
        <Field
          label="Разрешённые сети"
          hint="Через запятую: 10.10.0.0/24, 192.168.229.15. Обязательно."
        >
          <Input value={allowed} onChange={(e) => setAllowed(e.target.value)} />
        </Field>
        <Field
          label="Сертификат и ключ (PEM)"
          hint="Пути на сервере, например от доменного ЦС. Пусто — самоподписанный."
        >
          <div className="grid gap-2 sm:grid-cols-2">
            <Input
              placeholder="C:\…\hermes.crt"
              value={c.cert_file}
              onChange={(e) => set("cert_file", e.target.value)}
            />
            <Input
              placeholder="C:\…\hermes.key"
              value={c.key_file}
              onChange={(e) => set("key_file", e.target.value)}
            />
          </div>
        </Field>
        <Field
          label="Доп. имена для самоподписанного"
          hint="DNS-имена, по которым открывают панель (имя компьютера добавляется само)."
        >
          <Input
            value={names}
            onChange={(e) => setNames(e.target.value)}
            placeholder="hermes.deka.local"
          />
        </Field>
        <ErrorText error={save.error ?? status.error} />
        <div>
          <Button type="submit" disabled={save.isPending}>
            Сохранить и применить
          </Button>
        </div>
      </form>
    </Section>
  );
}

function Network() {
  const config = useQuery({ queryKey: ["network-config"], queryFn: api.networkConfig });
  if (!config.data) return <ErrorText error={config.error} />;
  return <NetworkForm initial={config.data} />;
}

export function AccessPage() {
  return (
    <>
      <h1 className="text-xl font-semibold">Доступ</h1>
      <div className="grid gap-4 xl:grid-cols-2">
        <Network />
        <Keys />
      </div>
    </>
  );
}
