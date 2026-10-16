import type { QuickCommandInfo } from "@/types/generated/QuickCommandInfo";

/** Group title by the id prefix of the quick-command pack. */
export const GROUPS: Record<string, string> = {
  helpdesk: "Хелпдеск",
  claude: "Клод (мост)",
  voice: "Голос и режимы",
  ui: "Окна Джарвиса",
  web: "Поиск и сайты",
  url: "Сайты",
  br: "Браузер",
  media: "Медиа",
  snd: "Звук",
  app: "Программы",
  run: "Windows",
  win: "Окна",
  clip: "Буфер и скриншоты",
  kb: "Клавиатура",
  scr: "Экран",
  pwr: "Питание",
  me: "Разговор",
};

const ORDER = Object.keys(GROUPS);

export function groupOf(id: string): string {
  const prefix = id.split(".")[0] ?? "";
  return GROUPS[prefix] ?? "Другое";
}

/** Commands grouped and filtered by a search string (matches name and phrases). */
export function groupCommands(list: QuickCommandInfo[], query: string): Array<[string, QuickCommandInfo[]]> {
  const q = query.trim().toLowerCase().replace(/ё/g, "е");
  const hit = (c: QuickCommandInfo) =>
    !q || c.name.toLowerCase().includes(q) || c.phrases.some((p) => p.toLowerCase().replace(/ё/g, "е").includes(q));
  const map = new Map<string, QuickCommandInfo[]>();
  for (const c of list) {
    if (!hit(c)) continue;
    const g = groupOf(c.id);
    map.set(g, [...(map.get(g) ?? []), c]);
  }
  return [...map.entries()].sort((a, b) => {
    const ia = ORDER.findIndex((k) => GROUPS[k] === a[0]);
    const ib = ORDER.findIndex((k) => GROUPS[k] === b[0]);
    return (ia === -1 ? 99 : ia) - (ib === -1 ? 99 : ib);
  });
}

