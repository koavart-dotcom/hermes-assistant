import { describe, expect, it } from "vitest";
import { dueTime, fireKey, hhmm } from "@/lib/digest";

describe("digest schedule", () => {
  it("fires within two minutes after a configured time, once per day", () => {
    const at = (h: number, m: number) => new Date(2026, 9, 16, h, m);
    const times = ["09:00", "14:00"];
    expect(dueTime(at(8, 59), times, new Set())).toBeNull();
    expect(dueTime(at(9, 0), times, new Set())).toBe("09:00");
    expect(dueTime(at(9, 2), times, new Set())).toBe("09:00");
    expect(dueTime(at(9, 3), times, new Set())).toBeNull();
    const fired = new Set([fireKey(at(9, 1), "09:00")]);
    expect(dueTime(at(9, 1), times, fired)).toBeNull();
    expect(dueTime(at(14, 0), times, fired)).toBe("14:00");
    expect(hhmm(at(9, 5))).toBe("09:05");
    expect(fireKey(at(9, 5), "09:00")).toBe("2026-10-16 09:00");
  });
});
