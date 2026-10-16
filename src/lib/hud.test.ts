import { describe, expect, it } from "vitest";
import { HISTORY, phaseStyle, pushHistory, smooth, type HudFrame } from "@/lib/hud";

describe("hud", () => {
  it("maps phases to distinct speeds and colours", () => {
    expect(phaseStyle("thinking").speed).toBeGreaterThan(phaseStyle("idle").speed);
    expect(phaseStyle("recording").rgb).not.toBe(phaseStyle("idle").rgb);
    expect(phaseStyle("thinking").scanner).toBe(true);
    expect(phaseStyle("speaking").scanner).toBe(false);
    expect(phaseStyle("off").glow).toBeLessThan(phaseStyle("listening").glow);
  });

  it("smooths quickly up and slowly down, keeps a ring of history", () => {
    expect(smooth(0, 1)).toBeCloseTo(0.5);
    expect(smooth(1, 0)).toBeCloseTo(0.88);
    const f: HudFrame = { t: 0, phase: "idle", level: 0, history: new Float32Array(HISTORY), head: 0 };
    for (let i = 1; i <= HISTORY + 2; i++) pushHistory(f, i);
    expect(f.head).toBe(2);
    expect(f.history[2]).toBe(HISTORY + 2);
    expect(f.history[3]).toBe(3); // oldest surviving sample
  });
});
