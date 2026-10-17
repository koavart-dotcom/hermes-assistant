/**
 * Decisions on Vosk results (always-listening mode): Vosk shows words ~0.2 s after they
 * are said, so a command is tried as soon as the partial phrase stops changing, without
 * waiting for Vosk's own end of phrase (~0.5–1 s later). Pure logic, unit-tested.
 */

import { stripWakeWord } from "@/lib/vad";
import type { VoskHeard } from "@/types/generated/VoskHeard";

export interface FastWakeOptions {
  wakeWords: () => string[];
  /** A partial phrase unchanged this long counts as said (ms). */
  stableMs?: number;
  /** Right after «Я здесь, сэр» a phrase needs no wake word. */
  following?: () => boolean;
  now?: () => number;
}

export class FastWake {
  private readonly opts: Required<FastWakeOptions>;
  private partial = "";
  private since = 0;
  /** Phrases already offered to the caller (each is tried once). */
  private tried = new Set<string>();

  constructor(opts: FastWakeOptions) {
    this.opts = { stableMs: 350, following: () => false, now: () => Date.now(), ...opts };
  }

  /** The wake word is in the phrase being spoken (for the HUD and ducking). */
  get wakeHeard(): boolean {
    return this.partial !== "" && stripWakeWord(this.partial, this.opts.wakeWords()) !== null;
  }

  /** Forget the phrase (after acting on it or after our own voice). */
  reset(): void {
    this.partial = "";
    this.since = 0;
    this.tried.clear();
  }

  /**
   * Feed one recogniser result; returns the command to try (wake word stripped, `""` for a
   * bare «Джарвис»), or null. A final result ends the phrase.
   */
  push(heard: VoskHeard): string | null {
    const now = this.opts.now();
    if (heard.text !== null) {
      const text = heard.text.trim();
      const seen = this.tried.has(text);
      this.reset();
      return text && !seen ? this.command(text) : null;
    }
    const partial = heard.partial.trim();
    if (partial !== this.partial) {
      this.partial = partial;
      this.since = now;
      return null;
    }
    if (!partial || now - this.since < this.opts.stableMs || this.tried.has(partial)) return null;
    this.tried.add(partial);
    return this.command(partial);
  }

  private command(text: string): string | null {
    const cmd = stripWakeWord(text, this.opts.wakeWords());
    if (cmd !== null) return cmd;
    return this.opts.following() ? text : null;
  }
}
