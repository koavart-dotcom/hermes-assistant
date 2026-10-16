import type { Ticket } from "@/types/generated/Ticket";
import type { TicketsSnapshot } from "@/types/generated/TicketsSnapshot";

export const MARK_TEXT: Record<string, string> = { new: "новая", updated: "обновлена сегодня", working: "в работе" };
export const STATUS_TEXT: Record<string, string> = { active: "активна", pending: "ожидает", closed: "закрыта", spam: "спам" };

/** "5 мин назад" / "3 ч назад" / "2 дн назад" for ISO-8601 UTC timestamps. */
export function ago(iso: string, now = Date.now()): string {
  const t = Date.parse(iso);
  if (Number.isNaN(t)) return "";
  const s = Math.max(0, Math.round((now - t) / 1000));
  if (s < 90) return "только что";
  const m = Math.round(s / 60);
  if (m < 60) return `${m} мин назад`;
  const h = Math.round(m / 60);
  if (h < 36) return `${h} ч назад`;
  return `${Math.round(h / 24)} дн назад`;
}

function line(t: Ticket): string {
  const who = t.customer ? ` — ${t.customer}` : "";
  const mark = t.mark !== "working" ? ` _(${MARK_TEXT[t.mark] ?? t.mark})_` : "";
  return `- [#${t.number}](${t.url}) ${t.subject}${who}${mark}`;
}

/** Markdown for the chat: the same content the window shows. */
export function ticketsMarkdown(s: TicketsSnapshot): string {
  const parts: string[] = [s.summary];
  if (s.mine.length) parts.push(`**На вас (${s.mine.length})**\n${s.mine.map(line).join("\n")}`);
  if (s.unassigned.length) parts.push(`**Без исполнителя (${s.unassigned.length})**\n${s.unassigned.map(line).join("\n")}`);
  return parts.join("\n\n");
}
