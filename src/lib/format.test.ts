import { formatBytes, percent } from "@/lib/format";

describe("format", () => {
  it("formats bytes in Russian units", () => {
    expect(formatBytes(6_600_000_000n)).toBe("6.6 ГБ");
    expect(formatBytes(1_471_226_485)).toBe("1.5 ГБ");
    expect(formatBytes(250_000_000)).toBe("250 МБ");
    expect(formatBytes(null)).toBe("—");
  });

  it("computes bounded percent", () => {
    expect(percent(50n, 200n)).toBe(25);
    expect(percent(10, null)).toBeNull();
    expect(percent(300, 200)).toBe(100);
  });
});
