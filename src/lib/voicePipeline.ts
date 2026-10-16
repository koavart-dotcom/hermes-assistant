/**
 * Voice pipeline shared by push-to-talk and always-listening modes:
 * audio → Whisper → quick command or chat turn → spoken answer.
 * Lives outside React so it keeps running on every page (see VoiceRuntime).
 */

import { api, reportError } from "@/services/tauri";
import { useChatStore } from "@/stores/chat";
import { useVoiceStore } from "@/stores/voice";
import type { QuickMatch } from "@/types/generated/QuickMatch";
import { Segmenter, rms, stripWakeWord } from "@/lib/vad";
import { TARGET_RATE, downsample, encodeWav, openMic, startRecording, toBase64, type Mic, type Recorder } from "@/lib/wav";

const MIN_UTTERANCE_SEC = 0.5;

let recorder: Recorder | null = null;
let mic: Mic | null = null;
let audio: HTMLAudioElement | null = null;
let pendingQuick: QuickMatch | null = null;
let pendingListeners: Array<(q: QuickMatch | null) => void> = [];

function errorText(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

let audioCtx: AudioContext | null = null;
let levelTimer: ReturnType<typeof setInterval> | null = null;

/** Feed the spoken answer's loudness into `level` so the HUD moves with the voice. */
function watchSpeechLevel(a: HTMLAudioElement): void {
  stopSpeechLevel();
  try {
    audioCtx ??= new AudioContext();
    const src = audioCtx.createMediaElementSource(a);
    const analyser = audioCtx.createAnalyser();
    analyser.fftSize = 512;
    src.connect(analyser);
    analyser.connect(audioCtx.destination);
    const buf = new Float32Array(analyser.fftSize);
    levelTimer = setInterval(() => {
      analyser.getFloatTimeDomainData(buf);
      let sum = 0;
      for (const x of buf) sum += x * x;
      useVoiceStore.getState().setLevel(Math.min(1, Math.sqrt(sum / buf.length) * 4));
    }, 40);
  } catch {
    /* no Web Audio: the HUD just pulses without amplitude */
  }
}

function stopSpeechLevel(): void {
  if (levelTimer) clearInterval(levelTimer);
  levelTimer = null;
  useVoiceStore.getState().setLevel(0);
}

export function stopSpeaking(): void {
  audio?.pause();
  audio = null;
  stopSpeechLevel();
  const s = useVoiceStore.getState();
  if (s.phase === "speaking") s.setPhase(s.mode === "wake" ? "listening" : "idle");
}

export function onPendingQuick(cb: (q: QuickMatch | null) => void): () => void {
  pendingListeners.push(cb);
  cb(pendingQuick);
  return () => {
    pendingListeners = pendingListeners.filter((x) => x !== cb);
  };
}

function setPendingQuick(q: QuickMatch | null): void {
  pendingQuick = q;
  for (const cb of pendingListeners) cb(q);
}

function idlePhase(): "idle" | "listening" {
  return useVoiceStore.getState().mode === "wake" ? "listening" : "idle";
}

/** Run a recognized utterance: quick command first, else a chat turn. */
export async function handleUtterance(text: string): Promise<void> {
  const v = useVoiceStore.getState();
  v.setTranscript(text);
  v.setAnswer("");
  if (!text.trim()) {
    v.setPhase(idlePhase());
    return;
  }
  try {
    const quick = await api.quickMatch(text);
    if (quick) {
      if (quick.stops_speech) stopSpeaking();
      if (quick.needs_confirm) {
        setPendingQuick(quick);
        v.setPhase(idlePhase());
        return;
      }
      const r = await api.quickRun(quick.id, false, quick.query);
      v.setAnswer(r.ok ? `⚡ ${r.name}` : `Команда «${r.name}» не выполнена: ${r.error ?? ""}`);
      v.setPhase(idlePhase());
      if (r.ui) await showUi(r.ui);
      if (r.ok && r.spoken) await speakAnswer(r.spoken);
      if (r.ok && r.handoff) await sendToChat(text, r.handoff);
      return;
    }
    await sendToChat(text, text);
  } catch (e) {
    v.setError(errorText(e));
    v.setPhase(idlePhase());
  }
}

/** Send a turn to the voice chat session; `shown` is what the user said, `message` what the model gets. */
async function sendToChat(shown: string, message: string): Promise<void> {
  const v = useVoiceStore.getState();
  const chat = useChatStore.getState();
  let sessionId = chat.activeId;
  if (!sessionId) {
    const s = await api.chatCreateSession("Голос");
    chat.upsertSession(s);
    chat.setActive(s.id);
    sessionId = s.id;
  }
  v.setWaitingSession(sessionId);
  chat.addUserTurn(sessionId, shown);
  v.setPhase("thinking");
  await api.chatSend(sessionId, message);
}

export async function confirmPendingQuick(yes: boolean): Promise<void> {
  const q = pendingQuick;
  setPendingQuick(null);
  if (!q || !yes) return;
  const v = useVoiceStore.getState();
  try {
    const r = await api.quickRun(q.id, true, q.query);
    v.setAnswer(r.ok ? `⚡ ${r.name}` : `Команда «${r.name}» не выполнена: ${r.error ?? ""}`);
    if (r.ui) await showUi(r.ui);
    if (r.ok && r.spoken) await speakAnswer(r.spoken);
  } catch (e) {
    v.setError(errorText(e));
  }
}

/** UI targets of quick commands (`ui:tickets`): open the window, speak the summary. */
export async function showUi(target: string): Promise<void> {
  const v = useVoiceStore.getState();
  try {
    if (target === "hud") await api.hudWindowOpen();
    if (target === "hud_off") await api.hudWindowClose();
    if (target !== "tickets") return;
    // The window is opened in parallel: the spoken summary must not wait for it.
    api.ticketsWindowOpen().catch((e) => {
      reportError("tickets window", errorText(e));
      v.setError(`Окно тикетов: ${errorText(e)}`);
    });
    const snap = await api.ticketsFetch();
    await speakAnswer(snap.summary);
  } catch (e) {
    reportError("showUi", errorText(e));
    v.setError(errorText(e));
    v.setPhase(idlePhase());
  }
}

/** Called by VoiceRuntime for every `completed` chat event of the voice session. */
export async function speakAnswer(content: string): Promise<void> {
  const v = useVoiceStore.getState();
  v.setAnswer(content);
  if (!v.speak || !content.trim()) {
    v.setPhase(idlePhase());
    return;
  }
  v.setPhase("speaking");
  try {
    const url = await api.voiceSpeak(content);
    stopSpeaking();
    const a = new Audio(url);
    audio = a;
    watchSpeechLevel(a);
    const done = () => {
      if (audio === a) {
        audio = null;
        stopSpeechLevel();
        useVoiceStore.getState().setPhase(idlePhase());
      }
    };
    a.onended = done;
    a.onerror = done;
    await a.play();
  } catch (e) {
    v.setError(`Озвучка: ${errorText(e)}`);
    v.setPhase(idlePhase());
  }
}

async function transcribeSamples(samples: Float32Array, sampleRate: number): Promise<string> {
  const pcm = downsample(samples, sampleRate, TARGET_RATE);
  return api.voiceTranscribe(toBase64(encodeWav(pcm, TARGET_RATE)), "audio/wav");
}

// ------------------------------------------------------------ push-to-talk --

export async function pttStart(): Promise<void> {
  const v = useVoiceStore.getState();
  if (recorder || (v.phase !== "idle" && v.phase !== "listening" && v.phase !== "speaking")) return;
  stopSpeaking();
  v.setError(null);
  try {
    await stopWakeListening(); // the mic cannot be shared with the always-on stream
    recorder = await startRecording();
    recorder.onLevel(v.setLevel);
    v.setPhase("recording");
  } catch (e) {
    v.setError(`Микрофон: ${errorText(e)}`);
  }
}

export async function pttStop(): Promise<void> {
  const r = recorder;
  if (!r) return;
  recorder = null;
  const v = useVoiceStore.getState();
  v.setPhase("transcribing");
  try {
    const { wav, seconds } = await r.stop();
    v.setLevel(0);
    if (seconds < MIN_UTTERANCE_SEC) {
      v.setPhase(idlePhase());
    } else {
      const text = await api.voiceTranscribe(toBase64(wav), "audio/wav");
      await handleUtterance(text);
    }
  } catch (e) {
    v.setError(errorText(e));
    v.setPhase(idlePhase());
  } finally {
    if (useVoiceStore.getState().mode === "wake") void startWakeListening();
  }
}

// --------------------------------------------------------- always listening --

let busy = false;

export async function startWakeListening(): Promise<void> {
  if (mic) return;
  const v = useVoiceStore.getState();
  try {
    let segmenter: Segmenter | null = null;
    mic = await openMic((frame, sampleRate) => {
      segmenter ??= new Segmenter(sampleRate);
      const st = useVoiceStore.getState();
      if (st.phase === "listening" || st.phase === "recording") st.setLevel(rms(frame) * 4);
      if (busy) return; // ignore audio while we transcribe/think/speak (avoids hearing ourselves)
      const utterance = segmenter.push(frame);
      if (st.phase === "listening" && segmenter.isOpen) st.setPhase("recording");
      if (!utterance) return;
      if (utterance.length / sampleRate < MIN_UTTERANCE_SEC) {
        st.setPhase("listening");
        return;
      }
      busy = true;
      st.setPhase("transcribing");
      transcribeSamples(utterance, sampleRate)
        .then(async (text) => {
          const cmd = stripWakeWord(text);
          if (cmd === null) {
            useVoiceStore.getState().setPhase("listening");
            return;
          }
          await handleUtterance(cmd || "привет");
        })
        .catch((e) => {
          useVoiceStore.getState().setError(errorText(e));
          useVoiceStore.getState().setPhase("listening");
        })
        .finally(() => {
          busy = false;
        });
    });
    v.setPhase("listening");
    v.setError(null);
  } catch (e) {
    v.setError(`Микрофон: ${errorText(e)}`);
    v.setPhase("idle");
  }
}

export async function stopWakeListening(): Promise<void> {
  const m = mic;
  mic = null;
  if (m) await m.close();
  const v = useVoiceStore.getState();
  if (v.phase === "listening" || v.phase === "recording") v.setPhase("idle");
}

/** Keep the listener in sync with the selected mode. */
export async function applyMode(): Promise<void> {
  const v = useVoiceStore.getState();
  if (v.mode === "wake") {
    await startWakeListening();
  } else {
    await stopWakeListening();
    v.setPhase(v.mode === "chat" ? "off" : "idle");
  }
}
