import { useQuery } from "@tanstack/react-query";
import { Link } from "react-router";
import { PageShell } from "@/app/PageShell";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { api, isTauri, type ApprovalChoice, type PermissionRecord } from "@/services/tauri";
import { useChatStore } from "@/stores/chat";

const ACTION_LABEL: Record<PermissionRecord["action"], string> = {
  allow: "разрешено",
  confirm: "подтверждение",
  block: "запрещено",
};

function when(ts: number): string {
  return new Date(ts * 1000).toLocaleTimeString("ru-RU", { hour: "2-digit", minute: "2-digit", second: "2-digit" });
}

/** Pending approvals (from the chat stream) and the decision journal of the permission plugin. */
export function ApprovalsPage() {
  const approvals = useChatStore((s) => s.approvals);
  const log = useQuery({
    queryKey: ["permissions_log"],
    queryFn: () => api.permissionsLog(200),
    enabled: isTauri(),
    refetchInterval: 5000,
  });

  const pending = Object.entries(approvals).filter(([, a]) => a);
  const clearApproval = useChatStore((s) => s.clearApproval);
  const setError = useChatStore((s) => s.setError);
  const error = useChatStore((s) => s.error);
  const decide = (sessionId: string, runId: string, choice: ApprovalChoice) =>
    api.chatApprove(runId, choice).catch((e: unknown) => {
      const msg = e instanceof Error ? e.message : String(e);
      // Run already finished or answered elsewhere: the card is stale.
      clearApproval(sessionId);
      setError(
        msg.includes("approval_not_pending") || msg.includes("409")
          ? "Этот запрос уже закрыт, подтверждение больше не требуется."
          : msg,
      );
    });

  if (!isTauri()) {
    return <PageShell title="Подтверждения" description="Доступно только внутри приложения." />;
  }

  return (
    <PageShell
      title="Подтверждения"
      description="Опасные действия помощника ждут вашего решения здесь и в чате. Ниже — журнал решений плагина разрешений."
    >
      <section className="mb-6">
        <h2 className="mb-2 text-sm font-semibold">Ожидают решения</h2>
        {error ? (
          <p className="mb-2 text-sm text-destructive" role="alert">
            {error}
          </p>
        ) : null}
        {pending.length === 0 ? (
          <p className="text-sm text-muted-foreground">Сейчас ничего не ждёт подтверждения.</p>
        ) : (
          <ul className="space-y-3">
            {pending.map(([sessionId, a]) => (
              <li key={sessionId} className="rounded-lg border border-amber-500/50 bg-amber-500/10 p-4 text-sm">
                <p className="font-medium">{a!.description ?? "Hermes просит разрешение"}</p>
                {a!.command ? <pre className="mt-2 overflow-auto rounded bg-background p-2 text-xs">{a!.command}</pre> : null}
                <div className="mt-3 flex flex-wrap gap-2">
                  {a!.choices.map((c) => (
                    <Button
                      key={c}
                      size="sm"
                      variant={c === "deny" ? "destructive" : c === "once" ? "default" : "outline"}
                      onClick={() => decide(sessionId, a!.runId, c as ApprovalChoice)}
                    >
                      {c === "once" ? "Разрешить" : c === "session" ? "На сессию" : c === "always" ? "Всегда" : "Запретить"}
                    </Button>
                  ))}
                </div>
                <p className="mt-2 text-xs text-muted-foreground">
                  Диалог: <Link className="underline" to="/chat">{sessionId}</Link>
                </p>
              </li>
            ))}
          </ul>
        )}
      </section>

      <section>
        <h2 className="mb-2 text-sm font-semibold">Журнал решений</h2>
        {log.data && log.data.length > 0 ? (
          <div className="max-h-[60vh] overflow-auto rounded-md border">
            <table className="w-full text-xs">
              <thead className="sticky top-0 bg-muted">
                <tr>
                  <th className="px-2 py-1 text-left">Время</th>
                  <th className="px-2 py-1 text-left">Инструмент</th>
                  <th className="px-2 py-1 text-left">Решение</th>
                  <th className="px-2 py-1 text-left">Что</th>
                </tr>
              </thead>
              <tbody>
                {[...log.data].reverse().map((r, i) => (
                  <tr key={i} className="border-t">
                    <td className="whitespace-nowrap px-2 py-1 text-muted-foreground">{when(r.ts)}</td>
                    <td className="px-2 py-1">{r.tool}</td>
                    <td className="px-2 py-1">
                      <Badge variant={r.action === "block" ? "destructive" : r.action === "confirm" ? "secondary" : "outline"}>
                        {ACTION_LABEL[r.action] ?? r.action}
                      </Badge>
                    </td>
                    <td className="max-w-[40ch] truncate px-2 py-1 font-mono" title={`${r.reason}\n${r.preview}`}>
                      {r.preview || r.reason}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        ) : (
          <p className="text-sm text-muted-foreground">
            Журнал пуст: плагин ещё не принимал решений (появится после первого вызова инструмента).
          </p>
        )}
      </section>
    </PageShell>
  );
}
