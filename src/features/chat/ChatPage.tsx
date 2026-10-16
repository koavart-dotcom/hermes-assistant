import { useCallback, useEffect, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Link } from "react-router";
import { Loader2, Plus, Send, Square, Trash2, Wrench } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { api, isTauri, onChatEvent, type ApprovalChoice } from "@/services/tauri";
import { useChatStore, type PendingApproval, type Turn } from "@/stores/chat";
import type { SessionSummary } from "@/types/generated/SessionSummary";

const EMPTY_TURNS: Turn[] = [];

const CHOICE_LABELS: Record<string, string> = {
  once: "Разрешить",
  session: "Разрешить на сессию",
  always: "Разрешить всегда",
  deny: "Запретить",
};

function sessionTitle(s: SessionSummary): string {
  return s.title?.trim() || s.preview?.trim() || s.id;
}

function errorText(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

export function ChatPage() {
  const sessions = useChatStore((s) => s.sessions);
  const activeId = useChatStore((s) => s.activeId);
  const turns = useChatStore((s) => (s.activeId ? (s.turns[s.activeId] ?? EMPTY_TURNS) : EMPTY_TURNS));
  const runId = useChatStore((s) => (s.activeId ? (s.runs[s.activeId] ?? null) : null));
  const approval = useChatStore((s) => (s.activeId ? (s.approvals[s.activeId] ?? null) : null));
  const error = useChatStore((s) => s.error);
  const { setSessions, upsertSession, removeSession, setActive, setHistory, addUserTurn, apply, setError } =
    useChatStore.getState();

  const info = useQuery({ queryKey: ["app_info"], queryFn: api.appInfo, enabled: isTauri(), refetchInterval: 10_000 });
  const gatewayUp = info.data?.hermes_state === "running";

  const [draft, setDraft] = useState("");
  const [busy, setBusy] = useState(false);
  const bottomRef = useRef<HTMLDivElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  // Live events from the Rust side.
  useEffect(() => {
    if (!isTauri()) return;
    let unlisten: (() => void) | undefined;
    onChatEvent((env) => apply(env.session_id, env.event)).then((u) => (unlisten = u));
    return () => unlisten?.();
  }, [apply]);

  // Session list once the gateway is up.
  useEffect(() => {
    if (!isTauri() || !gatewayUp) return;
    api
      .chatSessions()
      .then((list) => {
        setSessions(list);
        const current = useChatStore.getState().activeId;
        if (!current && list.length) setActive(list[0]!.id);
      })
      .catch((e) => setError(errorText(e)));
  }, [gatewayUp, setSessions, setActive, setError]);

  // History of the selected session (not while it streams).
  useEffect(() => {
    if (!isTauri() || !activeId || runId) return;
    api
      .chatMessages(activeId)
      .then((m) => setHistory(activeId, m))
      .catch((e) => setError(errorText(e)));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [activeId]);

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ block: "end" });
  }, [turns, approval]);

  const newSession = useCallback(async () => {
    try {
      const s = await api.chatCreateSession();
      upsertSession(s);
      setActive(s.id);
      setHistory(s.id, []);
      textareaRef.current?.focus();
    } catch (e) {
      setError(errorText(e));
    }
  }, [upsertSession, setActive, setHistory, setError]);

  const send = useCallback(async () => {
    const text = draft.trim();
    if (!text || busy) return;
    let sessionId = activeId;
    setBusy(true);
    try {
      if (!sessionId) {
        const s = await api.chatCreateSession();
        upsertSession(s);
        setActive(s.id);
        sessionId = s.id;
      }
      addUserTurn(sessionId, text);
      setDraft("");
      await api.chatSend(sessionId, text);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  }, [draft, busy, activeId, upsertSession, setActive, addUserTurn, setError]);

  const stop = useCallback(() => {
    if (activeId) api.chatStop(activeId).catch((e) => setError(errorText(e)));
  }, [activeId, setError]);

  const decide = useCallback(
    (a: PendingApproval, choice: ApprovalChoice) => {
      api.chatApprove(a.runId, choice).catch((e) => setError(errorText(e)));
    },
    [setError],
  );

  const remove = useCallback(
    async (id: string) => {
      try {
        await api.chatDeleteSession(id);
        removeSession(id);
      } catch (e) {
        setError(errorText(e));
      }
    },
    [removeSession, setError],
  );

  if (!isTauri()) {
    return (
      <div className="flex h-full flex-col">
        <header className="flex h-14 items-center border-b px-6">
          <h1 className="text-base font-semibold">Чат</h1>
        </header>
        <p className="p-6 text-sm text-muted-foreground" data-testid="chat-browser-mode">
          Чат работает только внутри приложения.
        </p>
      </div>
    );
  }

  return (
    <div className="flex h-full">
      <aside className="flex w-60 shrink-0 flex-col border-r">
        <div className="flex h-14 items-center justify-between border-b px-3">
          <span className="text-sm font-semibold">Диалоги</span>
          <Button size="sm" variant="outline" onClick={newSession} disabled={!gatewayUp} aria-label="Новый диалог">
            <Plus className="size-4" />
          </Button>
        </div>
        <div className="flex-1 overflow-auto p-2">
          {sessions.length === 0 ? (
            <p className="px-2 py-3 text-xs text-muted-foreground">Пока нет диалогов.</p>
          ) : (
            sessions.map((s) => (
              <div
                key={s.id}
                className={cn(
                  "group flex items-center gap-1 rounded-md px-2 py-1.5 text-sm",
                  s.id === activeId ? "bg-accent text-accent-foreground" : "hover:bg-accent/60",
                )}
              >
                <button type="button" className="min-w-0 flex-1 truncate text-left" onClick={() => setActive(s.id)}>
                  {sessionTitle(s)}
                </button>
                <button
                  type="button"
                  className="invisible text-muted-foreground hover:text-destructive group-hover:visible"
                  onClick={() => remove(s.id)}
                  aria-label="Удалить диалог"
                >
                  <Trash2 className="size-3.5" />
                </button>
              </div>
            ))
          )}
        </div>
      </aside>

      <section className="flex min-w-0 flex-1 flex-col">
        <header className="flex h-14 items-center justify-between border-b px-6">
          <h1 className="text-base font-semibold">Чат</h1>
          {info.data ? (
            <Badge variant={gatewayUp ? "default" : "secondary"}>
              {gatewayUp ? "Hermes работает" : info.data.hermes_state === "stopped" ? "Hermes остановлен" : "Hermes не установлен"}
            </Badge>
          ) : null}
        </header>

        {!gatewayUp && info.data ? (
          <div className="border-b bg-muted/40 px-6 py-3 text-sm">
            Помощник не запущен.{" "}
            <Link className="underline" to={info.data.hermes_state === "stopped" ? "/status" : "/setup"}>
              {info.data.hermes_state === "stopped" ? "Запустить в разделе «Состояние»" : "Перейти к установке"}
            </Link>
          </div>
        ) : null}

        <div className="flex-1 overflow-auto px-6 py-4">
          {turns.length === 0 ? (
            <p className="text-sm text-muted-foreground">Напишите сообщение — Hermes ответит здесь.</p>
          ) : (
            <div className="mx-auto flex max-w-3xl flex-col gap-4">
              {turns.map((t, i) => (
                <TurnView key={i} turn={t} />
              ))}
              {approval ? <ApprovalCard approval={approval} onDecide={decide} /> : null}
              <div ref={bottomRef} />
            </div>
          )}
        </div>

        {error ? (
          <div className="border-t bg-destructive/10 px-6 py-2 text-sm text-destructive" role="alert">
            {error}
          </div>
        ) : null}

        <footer className="border-t p-4">
          <div className="mx-auto flex max-w-3xl items-end gap-2">
            <textarea
              ref={textareaRef}
              className="min-h-[44px] max-h-40 flex-1 resize-y rounded-md border bg-background px-3 py-2 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
              placeholder={gatewayUp ? "Сообщение… (Enter — отправить, Shift+Enter — перенос строки)" : "Hermes не запущен"}
              value={draft}
              disabled={!gatewayUp || busy}
              rows={1}
              onChange={(e) => setDraft(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter" && !e.shiftKey) {
                  e.preventDefault();
                  void send();
                }
              }}
            />
            {runId ? (
              <Button variant="outline" onClick={stop} aria-label="Остановить">
                <Square className="size-4" />
              </Button>
            ) : (
              <Button onClick={() => void send()} disabled={!gatewayUp || busy || !draft.trim()} aria-label="Отправить">
                <Send className="size-4" />
              </Button>
            )}
          </div>
        </footer>
      </section>
    </div>
  );
}

function TurnView({ turn }: { turn: Turn }) {
  const user = turn.role === "user";
  return (
    <div className={cn("flex", user ? "justify-end" : "justify-start")}>
      <div
        className={cn(
          "max-w-[85%] rounded-lg px-4 py-2 text-sm",
          user ? "bg-primary text-primary-foreground" : "bg-muted",
        )}
      >
        {turn.tools?.length ? (
          <div className="mb-2 flex flex-wrap gap-1">
            {turn.tools.map((t, i) => (
              <Badge key={i} variant={t.status === "error" ? "destructive" : t.status === "ok" ? "secondary" : "outline"}>
                {t.status === "running" ? <Loader2 className="mr-1 size-3 animate-spin" /> : <Wrench className="mr-1 size-3" />}
                {t.tool}
                {t.preview ? `: ${t.preview.slice(0, 60)}` : ""}
              </Badge>
            ))}
          </div>
        ) : null}
        {turn.commentary?.length ? (
          <p className="mb-2 whitespace-pre-wrap text-xs text-muted-foreground">{turn.commentary.join("\n")}</p>
        ) : null}
        {turn.content ? (
          <p className="whitespace-pre-wrap break-words">{turn.content}</p>
        ) : turn.streaming ? (
          <span className="inline-flex items-center gap-2 text-muted-foreground">
            <Loader2 className="size-3.5 animate-spin" />
            {turn.thinking ? "думает…" : "…"}
          </span>
        ) : null}
        {turn.error ? <p className="mt-1 text-xs text-destructive">{turn.error}</p> : null}
      </div>
    </div>
  );
}

function ApprovalCard({
  approval,
  onDecide,
}: {
  approval: PendingApproval;
  onDecide: (a: PendingApproval, choice: ApprovalChoice) => void;
}) {
  return (
    <div className="rounded-lg border border-amber-500/50 bg-amber-500/10 p-4 text-sm" role="dialog" aria-label="Подтверждение действия">
      <p className="font-medium">Hermes просит разрешение на действие</p>
      {approval.description ? <p className="mt-1 text-muted-foreground">{approval.description}</p> : null}
      {approval.command ? (
        <pre className="mt-2 overflow-auto rounded bg-background p-2 text-xs">{approval.command}</pre>
      ) : null}
      <div className="mt-3 flex flex-wrap gap-2">
        {approval.choices.map((c) => (
          <Button
            key={c}
            size="sm"
            variant={c === "deny" ? "destructive" : c === "once" ? "default" : "outline"}
            onClick={() => onDecide(approval, c as ApprovalChoice)}
          >
            {CHOICE_LABELS[c] ?? c}
          </Button>
        ))}
      </div>
    </div>
  );
}
