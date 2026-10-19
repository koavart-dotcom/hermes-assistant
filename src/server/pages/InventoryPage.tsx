import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Pencil, Plus, Stethoscope, Trash2, Wrench } from "lucide-react";
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
import { Input } from "@/components/ui/input";
import type { DiagReport } from "@/types/generated/DiagReport";
import { DiagResult } from "../DiagResult";
import { FixDialog } from "../FixDialog";
import type { Host } from "@/types/generated/Host";
import { api } from "../api";
import { ErrorText, Field, Pre, Section, Select } from "../ui";

const EMPTY: Host = {
  name: "",
  aliases: [],
  address: "",
  os: "linux",
  access: "ssh",
  user: "",
  port: 0,
  winrm_configuration: "",
  owner: "",
  criticality: "normal",
  notes: "",
  fix: false,
  fix_services: [],
};

const ACCESS_LABEL: Record<Host["access"], string> = {
  ssh: "SSH",
  winrm: "WinRM (JEA)",
  none: "без доступа",
};

function HostDialog({
  host,
  onSave,
  onClose,
  saving,
  error,
}: {
  host: Host;
  onSave: (h: Host) => void;
  onClose: () => void;
  saving: boolean;
  error: unknown;
}) {
  const [h, setH] = useState<Host>(host);
  const set = <K extends keyof Host>(k: K, v: Host[K]) => setH({ ...h, [k]: v });
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="max-h-[90vh] overflow-y-auto sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>{host.name ? `Хост ${host.name}` : "Новый хост"}</DialogTitle>
          <DialogDescription>
            Имя — как агент называется в Wazuh. Остальное нужно для проверок и разбора.
          </DialogDescription>
        </DialogHeader>
        <form
          className="grid gap-3"
          onSubmit={(e) => {
            e.preventDefault();
            onSave(h);
          }}
        >
          <Field label="Имя в Wazuh (agent.name)">
            <Input value={h.name} onChange={(e) => set("name", e.target.value)} required />
          </Field>
          <div className="grid grid-cols-2 gap-3">
            <Field label="ОС">
              <Select
                value={h.os}
                onChange={(v) => {
                  const os = v as Host["os"];
                  setH({
                    ...h,
                    os,
                    access: h.access === "none" ? "none" : os === "windows" ? "winrm" : "ssh",
                  });
                }}
                options={[
                  { value: "linux", label: "Linux" },
                  { value: "windows", label: "Windows" },
                ]}
              />
            </Field>
            <Field label="Доступ для проверок">
              <Select
                value={h.access}
                onChange={(v) => set("access", v as Host["access"])}
                options={[
                  {
                    value: h.os === "windows" ? "winrm" : "ssh",
                    label: h.os === "windows" ? "WinRM (JEA)" : "SSH",
                  },
                  { value: "none", label: "без доступа" },
                ]}
              />
            </Field>
          </div>
          <Field
            label="Адрес"
            hint={
              h.os === "windows"
                ? "Для WinRM — DNS-имя (Kerberos), не IP. Пусто — имя хоста."
                : "DNS-имя или IP. Пусто — имя хоста."
            }
          >
            <Input value={h.address} onChange={(e) => set("address", e.target.value)} />
          </Field>
          <Field label="Другие имена" hint="Через запятую: FQDN, старые имена.">
            <Input
              value={h.aliases.join(", ")}
              onChange={(e) =>
                set(
                  "aliases",
                  e.target.value
                    .split(",")
                    .map((s) => s.trim())
                    .filter(Boolean),
                )
              }
            />
          </Field>
          {h.os === "linux" ? (
            <div className="grid grid-cols-2 gap-3">
              <Field label="Пользователь SSH">
                <Input
                  value={h.user}
                  placeholder="hermes-diag"
                  onChange={(e) => set("user", e.target.value)}
                />
              </Field>
              <Field label="Порт">
                <Input
                  inputMode="numeric"
                  value={h.port || ""}
                  placeholder="22"
                  onChange={(e) => set("port", Number(e.target.value.replace(/\D/g, "")) || 0)}
                />
              </Field>
            </div>
          ) : (
            <Field label="Точка входа JEA">
              <Input
                value={h.winrm_configuration}
                placeholder="HermesDiag"
                onChange={(e) => set("winrm_configuration", e.target.value)}
              />
            </Field>
          )}
          <div className="grid grid-cols-2 gap-3">
            <Field label="Ответственный">
              <Input value={h.owner} onChange={(e) => set("owner", e.target.value)} />
            </Field>
            <Field label="Критичность">
              <Select
                value={h.criticality || "normal"}
                onChange={(v) => set("criticality", v)}
                options={[
                  { value: "low", label: "низкая" },
                  { value: "normal", label: "обычная" },
                  { value: "high", label: "высокая" },
                ]}
              />
            </Field>
          </div>
          <Field label="Заметки" hint="Попадают в разбор: что здесь крутится, особенности.">
            <Input value={h.notes} onChange={(e) => set("notes", e.target.value)} />
          </Field>
          <label className="flex items-center gap-2 text-sm">
            <input type="checkbox" checked={h.fix} onChange={(e) => set("fix", e.target.checked)} />
            Разрешить починку из панели
          </label>
          {h.fix ? (
            <Field
              label="Службы, которые можно перезапускать"
              hint={
                h.os === "windows"
                  ? "Имена служб через запятую. На машине: windows-setup.ps1 -FixServices с тем же списком."
                  : "Юниты systemd через запятую. На машине: linux-setup.sh … --fix с тем же списком."
              }
            >
              <Input
                value={h.fix_services.join(", ")}
                onChange={(e) =>
                  set(
                    "fix_services",
                    e.target.value
                      .split(",")
                      .map((s) => s.trim())
                      .filter(Boolean),
                  )
                }
              />
            </Field>
          ) : null}
          <ErrorText error={error} />
          <DialogFooter>
            <Button type="button" variant="ghost" onClick={onClose}>
              Отмена
            </Button>
            <Button type="submit" disabled={saving || !h.name.trim()}>
              Сохранить
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

export function InventoryPage() {
  const qc = useQueryClient();
  const inventory = useQuery({ queryKey: ["inventory"], queryFn: api.inventory });
  const [editing, setEditing] = useState<{ host: Host; index: number } | null>(null);
  const [report, setReport] = useState<DiagReport | null>(null);
  const [key, setKey] = useState<string | null>(null);
  const [fixing, setFixing] = useState<Host | null>(null);
  const hosts = inventory.data?.hosts ?? [];

  const save = useMutation({
    mutationFn: (next: Host[]) => api.inventorySave({ hosts: next }),
    onSuccess: (inv) => {
      qc.setQueryData(["inventory"], inv);
      setEditing(null);
    },
  });
  const diag = useMutation({
    mutationFn: (name: string) => api.diagRun(name),
    onSuccess: setReport,
  });
  const sshKey = useMutation({ mutationFn: api.sshKey, onSuccess: (r) => setKey(r.public_key) });

  const upsert = (h: Host, index: number) => {
    const next = [...hosts];
    if (index < 0) next.push(h);
    else next[index] = h;
    save.mutate(next);
  };
  const remove = (index: number) => {
    if (!window.confirm(`Удалить ${hosts[index]?.name ?? "хост"} из инвентаря?`)) return;
    save.mutate(hosts.filter((_, i) => i !== index));
  };

  return (
    <>
      <h1 className="text-xl font-semibold">Инвентарь</h1>
      <Section
        title="Хосты"
        description="Какая машина стоит за агентом Wazuh, как на неё зайти для проверок только на чтение и кто за неё отвечает."
        actions={
          <Button size="sm" onClick={() => setEditing({ host: EMPTY, index: -1 })}>
            <Plus /> Добавить
          </Button>
        }
      >
        <ErrorText error={inventory.error ?? (editing ? null : save.error)} />
        {hosts.length === 0 ? (
          <p className="text-sm text-muted-foreground">
            Хостов нет. Алерты по неизвестным хостам разбираются только по тексту алерта.
          </p>
        ) : (
          <div className="overflow-x-auto">
            <table className="w-full text-sm">
              <thead className="text-left text-xs text-muted-foreground">
                <tr className="border-b border-border">
                  <th className="p-2">Имя</th>
                  <th className="p-2">Адрес</th>
                  <th className="p-2">ОС</th>
                  <th className="p-2">Доступ</th>
                  <th className="p-2">Ответственный</th>
                  <th className="p-2" />
                </tr>
              </thead>
              <tbody>
                {hosts.map((h, i) => (
                  <tr key={h.name} className="border-b border-border/60">
                    <td className="p-2 font-medium">
                      {h.name}
                      {h.criticality === "high" ? (
                        <Badge variant="destructive" className="ml-2">
                          высокая
                        </Badge>
                      ) : null}
                    </td>
                    <td className="p-2 text-muted-foreground">{h.address || h.name}</td>
                    <td className="p-2">{h.os === "windows" ? "Windows" : "Linux"}</td>
                    <td className="p-2">
                      <Badge variant={h.access === "none" ? "outline" : "secondary"}>
                        {ACCESS_LABEL[h.access]}
                      </Badge>
                    </td>
                    <td className="p-2 text-muted-foreground">{h.owner}</td>
                    <td className="p-2">
                      <div className="flex justify-end gap-1">
                        <Button
                          size="sm"
                          variant="outline"
                          disabled={h.access === "none" || diag.isPending}
                          onClick={() => diag.mutate(h.name)}
                          title="Запустить проверки только на чтение"
                        >
                          <Stethoscope /> Проверить
                        </Button>
                        {h.fix && h.access !== "none" ? (
                          <Button
                            size="sm"
                            variant="outline"
                            onClick={() => setFixing(h)}
                            title="Действие из каталога починок"
                          >
                            <Wrench /> Починить
                          </Button>
                        ) : null}
                        <Button
                          size="icon-sm"
                          variant="ghost"
                          onClick={() => setEditing({ host: h, index: i })}
                          title="Изменить"
                        >
                          <Pencil />
                        </Button>
                        <Button
                          size="icon-sm"
                          variant="ghost"
                          onClick={() => remove(i)}
                          title="Удалить"
                        >
                          <Trash2 />
                        </Button>
                      </div>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </Section>

      {diag.isPending || report || diag.error ? (
        <Section
          title="Результат проверки"
          actions={
            <Button size="sm" variant="ghost" onClick={() => setReport(null)}>
              Скрыть
            </Button>
          }
        >
          {diag.isPending ? <p className="text-sm text-muted-foreground">Проверяю…</p> : null}
          <ErrorText error={diag.error} />
          {report && !diag.isPending ? <DiagResult report={report} /> : null}
        </Section>
      ) : null}

      <Section
        title="Подготовка машин"
        description="Один раз на каждую машину: учётка только для чтения. Подробно — docs/DIAG.md."
      >
        <div className="space-y-2 text-sm">
          <p className="font-medium">Linux</p>
          <p className="text-muted-foreground">
            Ключ службы → на машине под root:{" "}
            <code>sudo bash linux-setup.sh '&lt;ключ&gt;' &lt;IP этого сервера&gt;</code>{" "}
            (tools/diag в репозитории).
          </p>
          <Button
            size="sm"
            variant="outline"
            onClick={() => sshKey.mutate()}
            disabled={sshKey.isPending}
          >
            Показать ключ службы
          </Button>
          <ErrorText error={sshKey.error} />
          {key ? <Pre>{key}</Pre> : null}
          <p className="pt-2 font-medium">Windows (домен)</p>
          <p className="text-muted-foreground">
            На машине под администратором:{" "}
            <code>.\windows-setup.ps1 -Principal 'DEKA\&lt;сервер&gt;$'</code> — точка входа JEA
            HermesDiag только с командами чтения.
          </p>
        </div>
      </Section>

      {fixing ? <FixDialog host={fixing} onClose={() => setFixing(null)} /> : null}

      {editing ? (
        <HostDialog
          host={editing.host}
          saving={save.isPending}
          error={save.error}
          onClose={() => {
            save.reset();
            setEditing(null);
          }}
          onSave={(h) => upsert(h, editing.index)}
        />
      ) : null}
    </>
  );
}
