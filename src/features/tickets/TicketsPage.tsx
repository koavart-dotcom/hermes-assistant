import { useQuery } from "@tanstack/react-query";
import { ExternalLink, Loader2, RefreshCw } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { MARK_TEXT, STATUS_TEXT, ago } from "@/features/tickets/ticketsText";
import { cn } from "@/lib/utils";
import { api, isTauri } from "@/services/tauri";
import type { Ticket } from "@/types/generated/Ticket";

function errorText(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

/**
 * "Мои тикеты": content of the separate always-on-top window (route /tickets,
 * no sidebar). Read-only list from FreeScout; refreshed on demand and every
 * 5 minutes while the window is open.
 */
export function TicketsPage() {
  const q = useQuery({
    queryKey: ["tickets"],
    queryFn: api.ticketsFetch,
    enabled: isTauri(),
    refetchInterval: 5 * 60_000,
    refetchOnWindowFocus: true,
  });
  const s = q.data;

  return (
    <div className="flex h-full flex-col bg-background text-foreground">
      <header className="flex items-center gap-2 border-b px-3 py-2">
        <h1 className="text-sm font-semibold">Мои тикеты</h1>
        {s ? <span className="text-xs text-muted-foreground">обновлено {ago(new Date(s.fetched_at * 1000).toISOString())}</span> : null}
        <span className="flex-1" />
        <Button size="sm" variant="ghost" onClick={() => void q.refetch()} disabled={q.isFetching} aria-label="Обновить">
          {q.isFetching ? <Loader2 className="size-4 animate-spin" /> : <RefreshCw className="size-4" />}
        </Button>
      </header>
      <div className="min-h-0 flex-1 overflow-auto px-3 py-2 text-sm">
        {!isTauri() ? <p className="text-muted-foreground">Доступно только внутри приложения.</p> : null}
        {q.isPending && isTauri() ? <p className="text-muted-foreground">Загружаю…</p> : null}
        {q.error ? (
          <p className="text-destructive" role="alert">
            {errorText(q.error)}
          </p>
        ) : null}
        {s ? (
          <>
            <p className="mb-3 text-muted-foreground" data-testid="tickets-summary">
              {s.summary}
            </p>
            <Section title="На вас" items={s.mine} />
            <Section title="Без исполнителя" items={s.unassigned} />
          </>
        ) : null}
      </div>
    </div>
  );
}

function Section({ title, items }: { title: string; items: Ticket[] }) {
  if (!items.length) return null;
  return (
    <section className="mb-4">
      <h2 className="mb-1 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
        {title} ({items.length})
      </h2>
      <ul className="space-y-2">
        {items.map((t) => (
          <TicketCard key={t.id} t={t} />
        ))}
      </ul>
    </section>
  );
}

function TicketCard({ t }: { t: Ticket }) {
  const fresh = t.mark !== "working";
  return (
    <li className={cn("rounded-lg border p-2", fresh && "border-primary/50 bg-primary/5")}>
      <div className="flex items-start gap-2">
        <div className="min-w-0 flex-1">
          <p className="truncate font-medium" title={t.subject}>
            <span className="mr-1 text-muted-foreground">#{t.number}</span>
            {t.subject}
          </p>
          <p className="truncate text-xs text-muted-foreground">
            {t.customer || t.customer_email}
            {t.mailbox ? ` · ${t.mailbox}` : ""}
            {t.updated_at ? ` · ${ago(t.updated_at)}` : ""}
          </p>
        </div>
        <Button size="icon" variant="ghost" onClick={() => void api.openUrl(t.url)} aria-label={`Открыть #${t.number} в браузере`}>
          <ExternalLink className="size-4" />
        </Button>
      </div>
      {t.last_message ? <p className="mt-1 line-clamp-3 whitespace-pre-line text-xs">{t.last_message}</p> : null}
      <div className="mt-1 flex flex-wrap gap-1">
        {fresh ? <Badge>{MARK_TEXT[t.mark] ?? t.mark}</Badge> : null}
        <Badge variant="secondary">{STATUS_TEXT[t.status] ?? t.status}</Badge>
        {t.tags.map((tag) => (
          <Badge key={tag} variant="outline">
            {tag}
          </Badge>
        ))}
      </div>
    </li>
  );
}
