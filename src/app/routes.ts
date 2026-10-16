import type { LucideIcon } from "lucide-react";
import { Activity, ListChecks, MessageSquare, Mic, Settings, ShieldCheck, Wand2 } from "lucide-react";

export interface RouteDef {
  path: string;
  title: string;
  icon: LucideIcon;
}

/** Sidebar navigation. Titles are Russian (UI language), paths English. */
export const routes: RouteDef[] = [
  { path: "/chat", title: "Чат", icon: MessageSquare },
  { path: "/approvals", title: "Подтверждения", icon: ShieldCheck },
  { path: "/voice", title: "Голос", icon: Mic },
  { path: "/commands", title: "Команды", icon: ListChecks },
  { path: "/status", title: "Состояние", icon: Activity },
  { path: "/settings", title: "Настройки", icon: Settings },
  { path: "/setup", title: "Установка", icon: Wand2 },
];
