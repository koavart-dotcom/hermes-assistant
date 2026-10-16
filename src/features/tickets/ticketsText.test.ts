import { describe, expect, it } from "vitest";
import { ago, ticketsMarkdown } from "@/features/tickets/ticketsText";
import type { Ticket } from "@/types/generated/Ticket";

const t = (n: number, mark = "working"): Ticket => ({
  id: n,
  number: n,
  subject: `Тема ${n}`,
  status: "active",
  mailbox: "IT Support",
  customer: "Иван Петров",
  customer_email: "ivan@x.ua",
  assignee: "",
  tags: [],
  created_at: "2026-10-16T08:00:00Z",
  updated_at: "2026-10-16T09:00:00Z",
  last_message: "",
  url: `https://h/conversation/${n}`,
  mark,
});

describe("tickets text", () => {
  it("formats relative time", () => {
    const now = Date.parse("2026-10-16T10:00:00Z");
    expect(ago("2026-10-16T09:59:30Z", now)).toBe("только что");
    expect(ago("2026-10-16T09:40:00Z", now)).toBe("20 мин назад");
    expect(ago("2026-10-16T07:00:00Z", now)).toBe("3 ч назад");
    expect(ago("2026-10-13T10:00:00Z", now)).toBe("3 дн назад");
    expect(ago("bad", now)).toBe("");
  });

  it("renders markdown with sections and marks", () => {
    const md = ticketsMarkdown({ fetched_at: 0, my_user_id: 4, mine: [t(1, "new")], unassigned: [t(2)], summary: "На вас 1 заявка." });
    expect(md).toContain("На вас 1 заявка.");
    expect(md).toContain("**На вас (1)**\n- [#1](https://h/conversation/1) Тема 1 — Иван Петров _(новая)_");
    expect(md).toContain("**Без исполнителя (1)**\n- [#2](https://h/conversation/2) Тема 2 — Иван Петров");
  });
});
