import { useEffect, useRef } from "react";
import { HISTORY, drawHud, pushHistory, smooth, type HudFrame } from "@/lib/hud";
import { useVoiceStore, type VoicePhase } from "@/stores/voice";

/**
 * Animated reactor HUD. Reads phase/level straight from the voice store inside
 * a requestAnimationFrame loop (no React re-renders per frame). `source`
 * overrides the store (used by the floating window, which mirrors the main
 * window's state through events).
 */
export function JarvisHud({
  size = 220,
  source,
  className,
}: {
  size?: number;
  source?: () => { phase: VoicePhase; level: number };
  className?: string;
}) {
  const ref = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvas = ref.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    const dpr = Math.min(2, window.devicePixelRatio || 1);
    canvas.width = size * dpr;
    canvas.height = size * dpr;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);

    const frame: HudFrame = { t: 0, phase: "idle", level: 0, history: new Float32Array(HISTORY), head: 0 };
    const start = performance.now();
    let last = start;
    let raf = 0;
    const read = source ?? (() => {
      const s = useVoiceStore.getState();
      return { phase: s.phase, level: s.level };
    });
    const tick = (now: number) => {
      const { phase, level } = read();
      frame.t = (now - start) / 1000;
      frame.phase = phase;
      frame.level = smooth(frame.level, Math.min(1, level));
      if (now - last >= 40) {
        pushHistory(frame, frame.level);
        last = now;
      }
      drawHud(ctx, size, frame);
      raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, [size, source]);

  return <canvas ref={ref} style={{ width: size, height: size }} className={className} aria-hidden />;
}
