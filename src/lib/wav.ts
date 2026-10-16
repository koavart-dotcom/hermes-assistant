/**
 * Microphone capture to 16 kHz mono PCM16 WAV — the safest input for
 * Faster-Whisper (no container/codec dependencies on the Hermes side).
 */

export const TARGET_RATE = 16_000;

/** Downsample float samples from `fromRate` to `toRate` (simple averaging). */
export function downsample(input: Float32Array, fromRate: number, toRate: number): Float32Array {
  if (fromRate === toRate) return input;
  if (fromRate < toRate) throw new Error("upsampling is not supported");
  const ratio = fromRate / toRate;
  const outLength = Math.floor(input.length / ratio);
  const out = new Float32Array(outLength);
  let offset = 0;
  for (let i = 0; i < outLength; i++) {
    const next = Math.floor((i + 1) * ratio);
    let sum = 0;
    let n = 0;
    for (let j = offset; j < next && j < input.length; j++) {
      sum += input[j]!;
      n++;
    }
    out[i] = n ? sum / n : 0;
    offset = next;
  }
  return out;
}

/** Encode PCM float samples [-1, 1] as a 16-bit mono WAV file. */
export function encodeWav(samples: Float32Array, sampleRate: number): Uint8Array {
  const buffer = new ArrayBuffer(44 + samples.length * 2);
  const view = new DataView(buffer);
  const writeStr = (offset: number, s: string) => {
    for (let i = 0; i < s.length; i++) view.setUint8(offset + i, s.charCodeAt(i));
  };
  writeStr(0, "RIFF");
  view.setUint32(4, 36 + samples.length * 2, true);
  writeStr(8, "WAVE");
  writeStr(12, "fmt ");
  view.setUint32(16, 16, true);
  view.setUint16(20, 1, true); // PCM
  view.setUint16(22, 1, true); // mono
  view.setUint32(24, sampleRate, true);
  view.setUint32(28, sampleRate * 2, true);
  view.setUint16(32, 2, true);
  view.setUint16(34, 16, true);
  writeStr(36, "data");
  view.setUint32(40, samples.length * 2, true);
  let o = 44;
  for (let i = 0; i < samples.length; i++, o += 2) {
    const s = Math.max(-1, Math.min(1, samples[i]!));
    view.setInt16(o, s < 0 ? s * 0x8000 : s * 0x7fff, true);
  }
  return new Uint8Array(buffer);
}

export function toBase64(bytes: Uint8Array): string {
  let binary = "";
  const chunk = 0x8000;
  for (let i = 0; i < bytes.length; i += chunk) {
    binary += String.fromCharCode(...bytes.subarray(i, i + chunk));
  }
  return btoa(binary);
}

/** Peak level of a buffer, 0..1 (for the "listening" meter). */
export function peak(samples: Float32Array): number {
  let p = 0;
  for (let i = 0; i < samples.length; i++) {
    const a = Math.abs(samples[i]!);
    if (a > p) p = a;
  }
  return p;
}

export interface Mic {
  /** Sample rate of the frames passed to `onFrame`. */
  sampleRate: number;
  close: () => Promise<void>;
}

/** Open the default microphone and stream float frames (always-listening mode). */
export async function openMic(onFrame: (frame: Float32Array, sampleRate: number) => void): Promise<Mic> {
  const stream = await navigator.mediaDevices.getUserMedia({
    audio: { channelCount: 1, echoCancellation: true, noiseSuppression: true },
  });
  const ctx = new AudioContext();
  const source = ctx.createMediaStreamSource(stream);
  const processor = ctx.createScriptProcessor(4096, 1, 1);
  processor.onaudioprocess = (e) => onFrame(new Float32Array(e.inputBuffer.getChannelData(0)), ctx.sampleRate);
  source.connect(processor);
  processor.connect(ctx.destination);
  return {
    sampleRate: ctx.sampleRate,
    close: async () => {
      processor.disconnect();
      source.disconnect();
      stream.getTracks().forEach((t) => t.stop());
      await ctx.close();
    },
  };
}

export interface Recorder {
  stop: () => Promise<{ wav: Uint8Array; seconds: number }>;
  onLevel: (cb: (level: number) => void) => void;
}

/** Start capturing the default microphone; `stop()` returns the WAV. */
export async function startRecording(): Promise<Recorder> {
  const stream = await navigator.mediaDevices.getUserMedia({
    audio: { channelCount: 1, echoCancellation: true, noiseSuppression: true },
  });
  const ctx = new AudioContext();
  const source = ctx.createMediaStreamSource(stream);
  const processor = ctx.createScriptProcessor(4096, 1, 1);
  const chunks: Float32Array[] = [];
  let levelCb: ((level: number) => void) | null = null;
  processor.onaudioprocess = (e) => {
    const data = e.inputBuffer.getChannelData(0);
    chunks.push(new Float32Array(data));
    levelCb?.(peak(data));
  };
  source.connect(processor);
  processor.connect(ctx.destination);
  const started = performance.now();
  return {
    onLevel: (cb) => {
      levelCb = cb;
    },
    stop: async () => {
      processor.disconnect();
      source.disconnect();
      stream.getTracks().forEach((t) => t.stop());
      const rate = ctx.sampleRate;
      await ctx.close();
      const total = chunks.reduce((n, c) => n + c.length, 0);
      const all = new Float32Array(total);
      let o = 0;
      for (const c of chunks) {
        all.set(c, o);
        o += c.length;
      }
      const pcm = downsample(all, rate, TARGET_RATE);
      return { wav: encodeWav(pcm, TARGET_RATE), seconds: (performance.now() - started) / 1000 };
    },
  };
}
