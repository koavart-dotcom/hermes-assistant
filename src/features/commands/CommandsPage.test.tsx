import { describe, expect, it } from "vitest";
import { groupCommands, groupOf } from "@/features/commands/groups";

const cmds = [
  { id: "win.desktop", name: "Свернуть все окна", phrases: ["сверни все окна"], confirm: false },
  { id: "web.youtube_search", name: "Поиск на YouTube", phrases: ["найди в ютубе {query}"], confirm: false },
  { id: "pwr.shutdown", name: "Выключить компьютер", phrases: ["выключи компьютер"], confirm: true },
];

describe("commands page", () => {
  it("groups by id prefix in a fixed order", () => {
    expect(groupOf("br.newtab")).toBe("Браузер");
    expect(groupOf("zzz.x")).toBe("Другое");
    const g = groupCommands(cmds, "");
    expect(g.map(([t]) => t)).toEqual(["Поиск и сайты", "Окна", "Питание"]);
  });

  it("filters by name and phrases, ignoring ё", () => {
    expect(groupCommands(cmds, "ютуб").flatMap(([, c]) => c.map((x) => x.id))).toEqual(["web.youtube_search"]);
    expect(groupCommands(cmds, "компьютер").flatMap(([, c]) => c.map((x) => x.id))).toEqual(["pwr.shutdown"]);
    expect(groupCommands(cmds, "нет такого")).toEqual([]);
  });
});
