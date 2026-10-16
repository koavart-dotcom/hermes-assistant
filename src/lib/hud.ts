/**
 * Jarvis-style HUD: a reactor core with rotating rings, drawn on a 2D canvas.
 * Pure functions here (no React) so the look is testable and reusable in the
 * floating window. State comes from the voice store: phase + level (0..1).
 */

import type { VoicePhase } from "@/stores/voice";

export interface HudStyle {
  /** Base colour as `r,g,b`. */
  rgb: string;
  /** Rotation speed multiplier (rad/s for the fastest ring). */
  speed: number;
  /** Overall brightness 0..1. */
  glow: number;
  /** Sweep scanner visible (thinking / transcribing). */
  scanner: boolean;
}

const CYAN = "79,209,255";
const AMBER = "255,176,80";
const VIOLET = "176,150,255";
const GREY = "140,150,160";

export function phaseStyle(phase: VoicePhase): HudStyle {
  switch (phase) {
    case "off":
      return { rgb: GREY, speed: 0.05, glow: 0.35, scanner: false };
    case "idle":
      return { rgb: CYAN, speed: 0.12, glow: 0.6, scanner: false };
    case "listening":
      return { rgb: CYAN, speed: 0.2, glow: 0.8, scanner: false };
    case "recording":
      return { rgb: AMBER, speed: 0.35, glow: 1, scanner: false };
    case "transcribing":
      return { rgb: VIOLET, speed: 0.7, glow: 0.9, scanner: true };
    case "thinking":
      return { rgb: VIOLET, speed: 1.1, glow: 0.9, scanner: true };
    case "speaking":
      return { rgb: CYAN, speed: 0.3, glow: 1, scanner: false };
  }
}

/** Exponential smoothing towards `target` (frame-rate independent-ish). */
export function smooth(current: number, target: number, up = 0.5, down = 0.12): number {
  const k = target > current ? up : down;
  return current + (target - current) * k;
}

export const HISTORY = 48;

export interface HudFrame {
  t: number; // seconds
  phase: VoicePhase;
  level: number; // smoothed 0..1
  history: Float32Array; // ring of recent levels, length HISTORY, newest at `head`
  head: number;
}

export function pushHistory(f: HudFrame, value: number): void {
  f.head = (f.head + 1) % HISTORY;
  f.history[f.head] = value;
}

const TAU = Math.PI * 2;

/** Draw one frame into a square canvas of `size` CSS px (ctx already scaled for DPR). */
export function drawHud(ctx: CanvasRenderingContext2D, size: number, f: HudFrame): void {
  const st = phaseStyle(f.phase);
  const c = size / 2;
  const R = size / 2;
  const lvl = Math.min(1, Math.max(0, f.level));
  const rgba = (a: number) => `rgba(${st.rgb},${Math.max(0, Math.min(1, a * st.glow))})`;

  ctx.clearRect(0, 0, size, size);
  ctx.lineCap = "round";

  // Ambient glow behind everything.
  const glow = ctx.createRadialGradient(c, c, 0, c, c, R * 0.75);
  glow.addColorStop(0, rgba(0.35 + 0.35 * lvl));
  glow.addColorStop(0.5, rgba(0.08));
  glow.addColorStop(1, rgba(0));
  ctx.fillStyle = glow;
  ctx.fillRect(0, 0, size, size);

  // Outer double ring with four brackets.
  ctx.save();
  ctx.translate(c, c);
  ctx.rotate(f.t * st.speed * 0.15);
  ctx.strokeStyle = rgba(0.5);
  ctx.lineWidth = 1;
  ctx.beginPath();
  ctx.arc(0, 0, R * 0.92, 0, TAU);
  ctx.stroke();
  ctx.lineWidth = 3;
  for (let i = 0; i < 4; i++) {
    const a0 = (i * TAU) / 4 - 0.25;
    ctx.beginPath();
    ctx.arc(0, 0, R * 0.96, a0, a0 + 0.5);
    ctx.stroke();
  }
  ctx.restore();

  // Level ring: 48 radial bars from history (equalizer around the core).
  ctx.save();
  ctx.translate(c, c);
  ctx.rotate(-f.t * st.speed * 0.1);
  ctx.lineWidth = Math.max(1.5, size / 110);
  for (let i = 0; i < HISTORY; i++) {
    const v = f.history[(f.head - i + HISTORY) % HISTORY] ?? 0;
    const a = (i * TAU) / HISTORY;
    const r0 = R * 0.7;
    const r1 = r0 + R * (0.03 + 0.14 * v);
    ctx.strokeStyle = rgba(0.25 + 0.75 * v);
    ctx.beginPath();
    ctx.moveTo(Math.cos(a) * r0, Math.sin(a) * r0);
    ctx.lineTo(Math.cos(a) * r1, Math.sin(a) * r1);
    ctx.stroke();
  }
  ctx.restore();

  // Dashed arcs ring (three 70° arcs), counter-rotating.
  ctx.save();
  ctx.translate(c, c);
  ctx.rotate(-f.t * st.speed * 0.6);
  ctx.strokeStyle = rgba(0.85);
  ctx.lineWidth = Math.max(2, size / 70);
  for (let i = 0; i < 3; i++) {
    const a0 = (i * TAU) / 3;
    ctx.beginPath();
    ctx.arc(0, 0, R * 0.56, a0, a0 + 1.22);
    ctx.stroke();
  }
  ctx.restore();

  // Tick ring (60 ticks), rotating.
  ctx.save();
  ctx.translate(c, c);
  ctx.rotate(f.t * st.speed * 0.4);
  ctx.lineWidth = 1;
  for (let i = 0; i < 60; i++) {
    const a = (i * TAU) / 60;
    const long = i % 5 === 0;
    ctx.strokeStyle = rgba(long ? 0.9 : 0.45);
    ctx.beginPath();
    ctx.moveTo(Math.cos(a) * R * 0.42, Math.sin(a) * R * 0.42);
    ctx.lineTo(Math.cos(a) * R * (long ? 0.47 : 0.45), Math.sin(a) * R * (long ? 0.47 : 0.45));
    ctx.stroke();
  }
  ctx.restore();

  // Scanner sweep while thinking: an arc with a fading tail.
  if (st.scanner) {
    ctx.save();
    ctx.translate(c, c);
    const a1 = (f.t * st.speed * 2.2) % TAU;
    for (let k = 0; k < 10; k++) {
      ctx.strokeStyle = rgba(0.9 * (1 - k / 10));
      ctx.lineWidth = Math.max(2, size / 60);
      ctx.beginPath();
      ctx.arc(0, 0, R * 0.64, a1 - (k + 1) * 0.1, a1 - k * 0.1);
      ctx.stroke();
    }
    ctx.restore();
  }

  // Core: pulsing disc with a bright centre.
  const coreR = R * (0.2 + 0.1 * lvl) * (1 + 0.02 * Math.sin(f.t * 3));
  const core = ctx.createRadialGradient(c, c, 0, c, c, coreR);
  core.addColorStop(0, `rgba(255,255,255,${0.9 * st.glow})`);
  core.addColorStop(0.35, rgba(0.9));
  core.addColorStop(1, rgba(0.15));
  ctx.fillStyle = core;
  ctx.beginPath();
  ctx.arc(c, c, coreR, 0, TAU);
  ctx.fill();
  ctx.strokeStyle = rgba(0.9);
  ctx.lineWidth = 1.5;
  ctx.beginPath();
  ctx.arc(c, c, coreR + 2, 0, TAU);
  ctx.stroke();
}
