import { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Search } from "lucide-react";
import { PageShell } from "@/app/PageShell";
import { Badge } from "@/components/ui/badge";
import { Input } from "@/components/ui/input";
import { api, isTauri } from "@/services/tauri";
import { groupCommands } from "@/features/commands/groups";

/** "Команды": the cheat sheet of everything Jarvis understands without the model. */
export function CommandsPage() {
  const list = useQuery({ queryKey: ["quick_commands"], queryFn: api.quickCommands, enabled: isTauri(), staleTime: 60_000 });
  const [query, setQuery] = useState("");
  const groups = useMemo(() => groupCommands(list.data ?? [], query), [list.data, query]);
  const total = list.data?.length ?? 0;

  return (
    <PageShell
      title="Команды"
      description="Всё, что Джарвис выполняет мгновенно, без модели. Скажите фразу после «Джарвис» или напишите её в чат. Фразы с «…» принимают продолжение: «найди в ютубе …»."
    >
      <div className="mb-4 flex max-w-xl items-center gap-2">
        <Search className="size-4 text-muted-foreground" aria-hidden />
        <Input value={query} onChange={(e) => setQuery(e.target.value)} placeholder="Поиск по фразам…" aria-label="Поиск команд" />
        <span className="shrink-0 text-xs text-muted-foreground" data-testid="commands-total">
          {total ? `${total} команд` : isTauri() ? "…" : "браузерный режим"}
        </span>
      </div>
      {!isTauri() ? <p className="text-sm text-muted-foreground">Список доступен только внутри приложения.</p> : null}
      <div className="grid gap-4 lg:grid-cols-2">
        {groups.map(([title, cmds]) => (
          <section key={title} className="hud-panel p-4 text-sm" aria-labelledby={`g-${title}`}>
            <h2 id={`g-${title}`} className="hud-label mb-2">
              {title} · {cmds.length}
            </h2>
            <ul className="space-y-2">
              {cmds.map((c) => (
                <li key={c.id}>
                  <p className="flex flex-wrap items-center gap-1.5 font-medium">
                    {c.name}
                    {c.confirm ? <Badge variant="outline">с подтверждением</Badge> : null}
                  </p>
                  <p className="text-xs text-muted-foreground">{c.phrases.map((p) => `«${p.replace("{query}", "…")}»`).join(", ")}</p>
                </li>
              ))}
            </ul>
          </section>
        ))}
      </div>
    </PageShell>
  );
}
