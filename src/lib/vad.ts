/**
 * Energy-based voice activity segmenter and wake-word check for the
 * always-listening mode. Pure functions: no audio APIs, fully unit-testable.
 */

export interface SegmenterOptions {
  /** RMS above this counts as speech (0..1). */
  threshold: number;
  /** Speech must last this long before a segment opens (ms). */
  minSpeechMs: number;
  /** Silence this long closes the segment (ms). */
  silenceMs: number;
  /** Hard cap on a segment (ms). */
  maxMs: number;
  /** Audio kept from before speech started (ms). */
  preRollMs: number;
  /**
   * Speech must be this many times louder than the background (a video or music
   * playing in the room): the noise floor follows steady sound within seconds, so
   * a pause in Artem's speech still closes the segment.
   */
  floorRatio: number;
}

export const DEFAULT_SEGMENTER: SegmenterOptions = {
  threshold: 0.02,
  minSpeechMs: 200,
  silenceMs: 800,
  maxMs: 12_000,
  preRollMs: 300,
  floorRatio: 2.5,
};

export function rms(frame: Float32Array): number {
  if (frame.length === 0) return 0;
  let sum = 0;
  for (let i = 0; i < frame.length; i++) sum += frame[i]! * frame[i]!;
  return Math.sqrt(sum / frame.length);
}

/** Feed microphone frames; emits a finished utterance (mono float samples) when one closes. */
export class Segmenter {
  private readonly opts: SegmenterOptions;
  private readonly sampleRate: number;
  private preRoll: Float32Array[] = [];
  private preRollSamples = 0;
  private current: Float32Array[] = [];
  private currentSamples = 0;
  private speechMs = 0;
  private silenceMs = 0;
  private open = false;
  /** Background loudness estimate (RMS): falls fast to quiet, rises slowly under steady sound. */
  private floor = 0;

  constructor(sampleRate: number, opts: Partial<SegmenterOptions> = {}) {
    this.sampleRate = sampleRate;
    this.opts = { ...DEFAULT_SEGMENTER, ...opts };
  }

  get isOpen(): boolean {
    return this.open;
  }

  /** Returns a finished utterance or null. */
  /** Drop everything buffered (used while our own voice is playing). */
  reset(): void {
    this.preRoll = [];
    this.preRollSamples = 0;
    this.current = [];
    this.currentSamples = 0;
    this.speechMs = 0;
    this.silenceMs = 0;
    this.open = false;
  }

  /** Current speech threshold: the fixed minimum or `floorRatio` × background, whichever is higher. */
  get threshold(): number {
    return Math.max(this.opts.threshold, this.floor * this.opts.floorRatio);
  }

  push(frame: Float32Array): Float32Array | null {
    const ms = (frame.length / this.sampleRate) * 1000;
    const level = rms(frame);
    const loud = level >= this.threshold;
    // Quiet frames pull the floor down within ~0.5 s; steady sound lifts it over ~4 s.
    // During the first 5 s of a phrase it only falls — a command is shorter, and Artem's
    // own sentence must not become «background»; a video that keeps a segment open longer
    // starts lifting it, so the next pause closes the segment.
    const openMs = (this.currentSamples / this.sampleRate) * 1000;
    if (level < this.floor) this.floor += (level - this.floor) * Math.min(1, ms / 500);
    else if (!this.open || openMs > 5000) this.floor += (level - this.floor) * Math.min(1, ms / 4000);
    if (!this.open) {
      this.preRoll.push(frame);
      this.preRollSamples += frame.length;
      const maxPre = (this.opts.preRollMs / 1000) * this.sampleRate;
      while (this.preRollSamples > maxPre && this.preRoll.length > 1) {
        this.preRollSamples -= this.preRoll.shift()!.length;
      }
      this.speechMs = loud ? this.speechMs + ms : 0;
      if (this.speechMs >= this.opts.minSpeechMs) {
        this.open = true;
        this.current = this.preRoll.slice();
        this.currentSamples = this.preRollSamples;
        this.preRoll = [];
        this.preRollSamples = 0;
        this.silenceMs = 0;
      }
      return null;
    }
    this.current.push(frame);
    this.currentSamples += frame.length;
    this.silenceMs = loud ? 0 : this.silenceMs + ms;
    const durationMs = (this.currentSamples / this.sampleRate) * 1000;
    if (this.silenceMs >= this.opts.silenceMs || durationMs >= this.opts.maxMs) {
      return this.finish();
    }
    return null;
  }

  private finish(): Float32Array {
    const out = new Float32Array(this.currentSamples);
    let o = 0;
    for (const c of this.current) {
      out.set(c, o);
      o += c.length;
    }
    this.current = [];
    this.currentSamples = 0;
    this.open = false;
    this.speechMs = 0;
    this.silenceMs = 0;
    return out;
  }
}

export const WAKE_WORDS = ["джарвис", "гермес", "hermes", "jarvis"];

function levenshtein(a: string, b: string): number {
  const prev = Array.from({ length: b.length + 1 }, (_, i) => i);
  for (let i = 1; i <= a.length; i++) {
    let left = i;
    for (let j = 1; j <= b.length; j++) {
      const cost = a[i - 1] === b[j - 1] ? 0 : 1;
      const cur = Math.min(prev[j]! + 1, left + 1, prev[j - 1]! + cost);
      prev[j - 1] = left;
      left = cur;
    }
    prev[b.length] = left;
  }
  return prev[b.length]!;
}

/**
 * Find the wake word and return the command after it (may be empty); null without it.
 * First word: one or two misheard letters are tolerated. Later in the phrase (a video or
 * a conversation was recorded first: «…джарвис, поставь на паузу») — the last wake word
 * counts, with at most one misheard letter and only when a command follows it, so a
 * sentence that merely ends with «джарвис» does not trigger.
 */
export function stripWakeWord(text: string, wakeWords: string[] = WAKE_WORDS): string | null {
  const words = text
    .toLowerCase()
    .replace(/ё/g, "е")
    .replace(/[^\p{L}\p{N}\s]/gu, " ")
    .split(/\s+/)
    .filter(Boolean);
  if (words.length === 0) return null;
  const isWake = (word: string, maxDistance: number) =>
    wakeWords.some((w) => word === w || (word.length >= 5 && levenshtein(word, w) <= maxDistance));
  if (isWake(words[0]!, 2)) return words.slice(1).join(" ");
  for (let i = words.length - 2; i >= 1; i--) {
    if (isWake(words[i]!, 1)) return words.slice(i + 1).join(" ");
  }
  return null;
}
