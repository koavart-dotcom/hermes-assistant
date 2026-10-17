import { describe, expect, it } from "vitest";

import { FastWake } from "@/lib/fastWake";

function setup(following = false) {
  let t = 0;
  const fw = new FastWake({ wakeWords: () => ["джарвис", "гермес"], now: () => t, following: () => following });
  const partial = (p: string, at: number) => {
    t = at;
    return fw.push({ partial: p, text: null });
  };
  const final = (text: string, at: number) => {
    t = at;
    return fw.push({ partial: "", text });
  };
  return { fw, partial, final };
}

describe("FastWake", () => {
  it("offers the command once the partial phrase stops changing", () => {
    const { fw, partial } = setup();
    expect(partial("джарвис", 0)).toBeNull();
    expect(fw.wakeHeard).toBe(true);
    expect(partial("джарвис поставь", 200)).toBeNull();
    expect(partial("джарвис поставь на паузу", 400)).toBeNull();
    expect(partial("джарвис поставь на паузу", 600)).toBeNull(); // 200 ms — not yet
    expect(partial("джарвис поставь на паузу", 800)).toBe("поставь на паузу");
    expect(partial("джарвис поставь на паузу", 1000)).toBeNull(); // tried once
  });

  it("skips the final result of a phrase already tried", () => {
    const { partial, final } = setup();
    partial("джарвис пауза", 0);
    expect(partial("джарвис пауза", 400)).toBe("пауза");
    expect(final("джарвис пауза", 700)).toBeNull();
  });

  it("tries a final result that differs from the partial", () => {
    const { partial, final } = setup();
    partial("джарвис звук на", 0);
    expect(partial("джарвис звук на", 400)).toBe("звук на");
    expect(final("джарвис звук на сто", 600)).toBe("звук на сто");
  });

  it("ignores speech without the wake word (a video in the room)", () => {
    const { fw, partial, final } = setup();
    partial("у нас все висели", 0);
    expect(fw.wakeHeard).toBe(false);
    expect(partial("у нас все висели", 500)).toBeNull();
    expect(final("у нас все висели включи музыку", 900)).toBeNull();
  });

  it("finds the wake word in the middle of background speech", () => {
    const { final } = setup();
    expect(final("заказ выполнен джарвис включи музыку", 0)).toBe("включи музыку");
  });

  it("a bare wake word is a call", () => {
    const { final } = setup();
    expect(final("джарвис", 0)).toBe("");
  });

  it("right after «Я здесь, сэр» no wake word is needed", () => {
    const { final } = setup(true);
    expect(final("поставь на паузу", 0)).toBe("поставь на паузу");
  });
});
