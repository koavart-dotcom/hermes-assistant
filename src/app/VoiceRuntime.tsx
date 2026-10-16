import { useEffect } from "react";
import { emitVoiceState, isTauri, onChatEvent, onHudPtt } from "@/services/tauri";
import { applyMode, loadPrefs, pttStart, pttStop, speakAnswer } from "@/lib/voicePipeline";
import { useVoiceStore } from "@/stores/voice";

/**
 * App-level voice runtime: starts/stops the always-listening microphone when
 * the mode changes and speaks the answers of voice-initiated chat turns, on
 * whatever page is open.
 */
export function VoiceRuntime() {
  const mode = useVoiceStore((s) => s.mode);

  useEffect(() => {
    if (!isTauri()) return;
    void loadPrefs().then(applyMode);
  }, [mode]);

  useEffect(() => {
    if (!isTauri()) return;
    let unlisten: (() => void) | undefined;
    onChatEvent((env) => {
      const v = useVoiceStore.getState();
      if (env.session_id !== v.waitingSession) return;
      if (env.event.kind === "completed") void speakAnswer(env.event.content);
      if (env.event.kind === "failed") {
        v.setError(env.event.error);
        v.setPhase(v.mode === "wake" ? "listening" : "idle");
      }
      if (env.event.kind === "ended") {
        v.setWaitingSession(null);
        if (v.phase === "thinking") v.setPhase(v.mode === "wake" ? "listening" : "idle");
      }
    }).then((u) => (unlisten = u));
    return () => unlisten?.();
  }, []);

  // Mirror phase/level to the floating HUD window (≤ 25 fps) and take its presses.
  useEffect(() => {
    if (!isTauri()) return;
    let timer: ReturnType<typeof setTimeout> | null = null;
    let pending = false;
    const flush = () => {
      timer = null;
      pending = false;
      const s = useVoiceStore.getState();
      void emitVoiceState({ phase: s.phase, level: s.level });
    };
    const unsub = useVoiceStore.subscribe((s, prev) => {
      if (s.phase === prev.phase && s.level === prev.level) return;
      if (s.phase !== prev.phase) {
        if (timer) clearTimeout(timer);
        flush();
        return;
      }
      if (!pending) {
        pending = true;
        timer = setTimeout(flush, 40);
      }
    });
    let unlisten: (() => void) | undefined;
    onHudPtt((down) => void (down ? pttStart() : pttStop())).then((u) => (unlisten = u));
    return () => {
      unsub();
      if (timer) clearTimeout(timer);
      unlisten?.();
    };
  }, []);

  return null;
}
