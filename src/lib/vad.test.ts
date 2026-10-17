import { describe, expect, it } from "vitest";
import { Segmenter, rms, stripWakeWord } from "@/lib/vad";

function frame(n: number, amp: number): Float32Array {
  const f = new Float32Array(n);
  for (let i = 0; i < n; i++) f[i] = amp * (i % 2 === 0 ? 1 : -1);
  return f;
}

describe("segmenter", () => {
  it("opens on sustained speech and closes after silence", () => {
    const sr = 16_000;
    const seg = new Segmenter(sr, { threshold: 0.02, minSpeechMs: 100, silenceMs: 300, preRollMs: 100 });
    const step = 1600; // 100 ms
    expect(seg.push(frame(step, 0.001))).toBeNull(); // silence
    expect(seg.push(frame(step, 0.2))).toBeNull(); // speech 100 ms → opens
    expect(seg.isOpen).toBe(true);
    expect(seg.push(frame(step, 0.2))).toBeNull();
    expect(seg.push(frame(step, 0.001))).toBeNull(); // 100 ms silence
    expect(seg.push(frame(step, 0.001))).toBeNull(); // 200
    const out = seg.push(frame(step, 0.001)); // 300 → closes
    expect(out).not.toBeNull();
    // the opening speech frame (kept as pre-roll, 100 ms) + speech (100 ms) + silence (300 ms) = 500 ms
    expect(out!.length).toBe(5 * step);
    expect(seg.isOpen).toBe(false);
  });

  it("reset drops an open segment", () => {
    const seg = new Segmenter(16_000, { threshold: 0.02, minSpeechMs: 100, silenceMs: 300 });
    seg.push(frame(1600, 0.2));
    expect(seg.isOpen).toBe(true);
    seg.reset();
    expect(seg.isOpen).toBe(false);
    expect(seg.push(frame(1600, 0.001))).toBeNull();
  });

  it("ignores short blips and caps long segments", () => {
    const sr = 16_000;
    const seg = new Segmenter(sr, { threshold: 0.02, minSpeechMs: 200, silenceMs: 500, maxMs: 1000 });
    expect(seg.push(frame(1600, 0.2))).toBeNull(); // 100 ms, below minSpeech
    expect(seg.push(frame(1600, 0.001))).toBeNull();
    expect(seg.isOpen).toBe(false);
    for (let i = 0; i < 2; i++) seg.push(frame(1600, 0.2)); // 200 ms → open
    let out: Float32Array | null = null;
    for (let i = 0; i < 20 && !out; i++) out = seg.push(frame(1600, 0.2)); // continuous speech → maxMs
    expect(out).not.toBeNull();
    expect(rms(frame(4, 0.5))).toBeCloseTo(0.5);
  });
});

describe("wake word", () => {
  it("strips the wake word and tolerates misheard letters", () => {
    expect(stripWakeWord("Джарвис, открой браузер")).toBe("открой браузер");
    expect(stripWakeWord("гермес сверни все окна")).toBe("сверни все окна");
    expect(stripWakeWord("Джарвиз открой телеграм")).toBe("открой телеграм");
    expect(stripWakeWord("Hermes, how are you?")).toBe("how are you");
    expect(stripWakeWord("джарвис")).toBe("");
  });

  it("returns null without a wake word", () => {
    expect(stripWakeWord("открой браузер")).toBeNull();
    expect(stripWakeWord("")).toBeNull();
    expect(stripWakeWord("это был не вызов джарвис")).toBeNull();
  });
});
