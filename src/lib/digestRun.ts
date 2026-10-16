import { speakAnswer } from "@/lib/voicePipeline";
import { api, reportError } from "@/services/tauri";
import { useChatStore } from "@/stores/chat";
import { useVoiceStore } from "@/stores/voice";

/** Build and deliver the helpdesk digest: chat turn, voice, Telegram (per prefs). */
export async function runDigest(manual = false): Promise<void> {
  const v = useVoiceStore.getState();
  const d = v.prefs.digest;
  try {
    const r = await api.digestRun(d.telegram);
    const chat = useChatStore.getState();
    let sessionId = chat.activeId;
    if (!sessionId) {
      const s = await api.chatCreateSession("Сводка хелпдеска");
      chat.upsertSession(s);
      chat.setActive(s.id);
      sessionId = s.id;
    }
    chat.addAssistantTurn(sessionId, r.text, r.telegram_error ?? undefined);
    if ((d.speak || manual) && !v.prefs.silent) await speakAnswer(r.spoken);
  } catch (e) {
    reportError("digest", e instanceof Error ? e.message : String(e));
  }
}
