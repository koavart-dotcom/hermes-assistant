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
}

export const DEFAULT_SEGMENTER: SegmenterOptions = {
  threshold: 0.02,
  minSpeechMs: 200,
  silenceMs: 800,
  maxMs: 12_000,
  preRollMs: 300,
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

  constructor(sampleRate: number, opts: Partial<SegmenterOptions> = {}) {
    this.sampleRate = sampleRate;
    this.opts = { ...DEFAULT_SEGMENTER, ...opts };
  }

  get isOpen(): boolean {
    return this.open;
  }

  /** Returns a finished utterance or null. */
  push(frame: Float32Array): Float32Array | null {
    const ms = (frame.length / this.sampleRate) * 1000;
    const loud = rms(frame) >= this.opts.threshold;
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
 * If the utterance starts with a wake word (tolerating one or two
 * misheard letters), return the command that follows (may be empty);
 * otherwise null.
 */
export function stripWakeWord(text: string, wakeWords: string[] = WAKE_WORDS): string | null {
  const words = text
    .toLowerCase()
    .replace(/ё/g, "е")
    .replace(/[^\p{L}\p{N}\s]/gu, " ")
    .split(/\s+/)
    .filter(Boolean);
  if (words.length === 0) return null;
  const first = words[0]!;
  const hit = wakeWords.some((w) => first === w || (first.length >= 5 && levenshtein(first, w) <= 2));
  if (!hit) return null;
  return words.slice(1).join(" ");
}
