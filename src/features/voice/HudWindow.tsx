import { useEffect, useRef, useState } from "react";
import { JarvisHud } from "@/features/voice/JarvisHud";
import { api, emitHudPtt, isTauri, onVoiceState } from "@/services/tauri";
import type { VoicePhase } from "@/stores/voice";

const PHASES: VoicePhase[] = ["off", "idle", "listening", "recording", "transcribing", "thinking", "speaking"];

/**
 * Content of the frameless always-on-top "hud" window: just the reactor.
 * State is mirrored from the main window (voice:state events); pressing it
 * is push-to-talk (hud:ptt events handled by VoiceRuntime in the main window).
 * Drag anywhere to move; double-click closes.
 */
export function HudWindow() {
  const state = useRef<{ phase: VoicePhase; level: number }>({ phase: "idle", level: 0 });
  const [phase, setPhase] = useState<VoicePhase>("idle");

  useEffect(() => {
    document.documentElement.classList.add("hud-window");
    if (!isTauri()) return;
    let unlisten: (() => void) | undefined;
    onVoiceState((s) => {
      const p = PHASES.includes(s.phase as VoicePhase) ? (s.phase as VoicePhase) : "idle";
      state.current = { phase: p, level: s.level };
      setPhase(p);
    }).then((u) => (unlisten = u));
    return () => {
      unlisten?.();
      document.documentElement.classList.remove("hud-window");
    };
  }, []);

  return (
    <div className="group relative flex h-full w-full items-center justify-center select-none">
      {/* Drag handle: a strip at the top, visible on hover (left press on the reactor is push-to-talk). */}
      <div
        data-tauri-drag-region
        className="absolute inset-x-8 top-0 h-5 cursor-grab rounded-b-md bg-foreground/10 opacity-0 transition-opacity group-hover:opacity-100 active:cursor-grabbing"
        title="Перетащить"
      />
      <button
        type="button"
        className="rounded-full outline-none"
        onMouseDown={(e) => e.button === 0 && void emitHudPtt(true)}
        onMouseUp={(e) => e.button === 0 && void emitHudPtt(false)}
        onMouseLeave={() => void emitHudPtt(false)}
        onDoubleClick={() => void api.hudWindowClose()}
        title={`Джарвис: ${phase}. Удерживайте — говорите; двойной клик — закрыть.`}
        aria-label="Нажмите и говорите"
      >
        <JarvisHud size={200} source={() => state.current} className="pointer-events-none" />
      </button>
    </div>
  );
}
