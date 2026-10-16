import { describe, expect, it } from "vitest";
import { downsample, encodeWav, peak, toBase64 } from "@/lib/wav";

describe("wav helpers", () => {
  it("downsamples by averaging", () => {
    const input = new Float32Array([0, 1, 0, 1, 0, 1, 0, 1]);
    const out = downsample(input, 48_000, 16_000);
    expect(out.length).toBe(2);
    expect(out[0]).toBeCloseTo(1 / 3, 5);
    expect(() => downsample(input, 8_000, 16_000)).toThrow();
    expect(downsample(input, 16_000, 16_000)).toBe(input);
  });

  it("writes a valid 16-bit mono RIFF header", () => {
    const wav = encodeWav(new Float32Array([0, 0.5, -0.5, 1, -1]), 16_000);
    const view = new DataView(wav.buffer);
    const tag = (o: number) => String.fromCharCode(...wav.subarray(o, o + 4));
    expect(tag(0)).toBe("RIFF");
    expect(tag(8)).toBe("WAVE");
    expect(tag(12)).toBe("fmt ");
    expect(view.getUint16(20, true)).toBe(1);
    expect(view.getUint16(22, true)).toBe(1);
    expect(view.getUint32(24, true)).toBe(16_000);
    expect(view.getUint16(34, true)).toBe(16);
    expect(tag(36)).toBe("data");
    expect(view.getUint32(40, true)).toBe(10);
    expect(view.getInt16(44, true)).toBe(0);
    expect(view.getInt16(46, true)).toBe(Math.trunc(0.5 * 0x7fff));
    expect(view.getInt16(50, true)).toBe(0x7fff);
    expect(view.getInt16(52, true)).toBe(-0x8000);
  });

  it("base64-encodes and measures peaks", () => {
    expect(toBase64(new Uint8Array([82, 73, 70, 70]))).toBe("UklGRg==");
    expect(peak(new Float32Array([0.1, -0.7, 0.3]))).toBeCloseTo(0.7);
  });
});
