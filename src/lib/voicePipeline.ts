/**
 * Voice pipeline shared by push-to-talk and always-listening modes:
 * audio → Whisper → quick command or chat turn → spoken answer.
 * Lives outside React so it keeps running on every page (see VoiceRuntime).
 */

import { api, logInfo, reportError } from "@/services/tauri";
import { useChatStore } from "@/stores/chat";
import { useVoiceStore } from "@/stores/voice";
import type { CallStatus } from "@/types/generated/CallStatus";
import type { QuickMatch } from "@/types/generated/QuickMatch";
import { Segmenter, rms, stripWakeWord } from "@/lib/vad";
import { TARGET_RATE, downsample, encodeWav, openMic, setPreferredMic, startRecording, toBase64, type Mic, type Recorder } from "@/lib/wav";

const MIN_UTTERANCE_SEC = 0.5;
/** After «Я здесь, сэр» the next phrase needs no wake word for this long. */
const FOLLOW_MS = 8000;
let followUntil = 0;

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
    if (audioCtx.state !== "running") void audioCtx.resume();
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
    playAck();
    // The persona answers «[голос]» messages briefly and without Markdown (they are spoken).
    await sendToChat(text, `[голос] ${text}`);
  } catch (e) {
    v.setError(errorText(e));
    v.setPhase(idlePhase());
  }
}

let ack: Promise<void> | null = null;

/**
 * «Вас понял, сэр» while the model thinks. The backend also wakes the Jarvis voice
 * server, which unloads itself when idle, so its loading overlaps with the thinking.
 */
function playAck(): void {
  const v = useVoiceStore.getState();
  if (!v.speak || v.quiet) return;
  quietUntil = Date.now() + 4000; // the mic must not take the clip for a new question
  const p: Promise<void> = api
    .voiceAck()
    .catch((e: unknown) => reportError("ack", errorText(e)))
    .finally(() => {
      quietUntil = Date.now() + 700;
      if (ack === p) ack = null;
    });
  ack = p;
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
    if (target === "follow") {
      followUntil = Date.now() + FOLLOW_MS;
      v.setAnswer("Слушаю — можно без «Джарвис».");
      return;
    }
    if (target === "quiet_on" || target === "quiet_off") {
      v.setQuiet(target === "quiet_on");
      v.setAnswer(target === "quiet_on" ? "Тихий режим: не говорю и реагирую только на «слушай»." : "Снова слушаю.");
      return;
    }
    if (target === "ticket_send") {
      const { sendDraftReply } = await import("@/lib/ticketReply");
      const msg = await sendDraftReply();
      v.setAnswer(msg);
      const chat = useChatStore.getState();
      if (chat.activeId) chat.addAssistantTurn(chat.activeId, msg);
      await speakAnswer(msg);
      return;
    }
    if (target === "digest") {
      const { runDigest } = await import("@/lib/digestRun");
      await runDigest(true);
      return;
    }
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
  if (!v.speak || v.prefs.silent || v.quiet || !content.trim()) {
    v.setPhase(idlePhase());
    return;
  }
  v.setPhase("speaking");
  try {
    const url = await api.voiceSpeak(content);
    if (ack) await ack; // let the reaction clip finish first
    stopSpeaking();
    const a = new Audio(url);
    a.volume = Math.min(1, Math.max(0, v.prefs.voice_volume));
    audio = a;
    watchSpeechLevel(a);
    const done = () => {
      quietUntil = Date.now() + 700;
      if (audio === a) {
        audio = null;
        stopSpeechLevel();
        useVoiceStore.getState().setPhase(idlePhase());
      }
    };
    a.onended = done;
    a.onerror = () => {
      const err = a.error ? `${a.error.code} ${a.error.message}` : "unknown";
      reportError("speak", `audio element error: ${err} (url ${url.slice(0, 30)}…, ${url.length} chars)`);
      v.setError(`Озвучка: плеер не смог воспроизвести ответ (${err})`);
      done();
    };
    await a.play();
  } catch (e) {
    reportError("speak", errorText(e));
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
  if (recorder) {
    // A press that never got its release (window closed mid-press): finish it first.
    await pttStop();
    return;
  }
  if (v.phase !== "idle" && v.phase !== "listening" && v.phase !== "speaking") return;
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
/** Ignore the microphone until this time (ms) — set when an answer finishes playing. */
let quietUntil = 0;

export async function startWakeListening(): Promise<void> {
  if (mic || call.active) return;
  const v = useVoiceStore.getState();
  try {
    let segmenter: Segmenter | null = null;
    mic = await openMic((frame, sampleRate) => {
      segmenter ??= new Segmenter(sampleRate);
      const st = useVoiceStore.getState();
      if (st.phase === "listening" || st.phase === "recording") st.setLevel(rms(frame) * 4);
      if (busy) return; // ignore audio while we transcribe/think/speak (avoids hearing ourselves)
      // Echo guard: our own answer (and its tail in the room) must not open a new utterance.
      if (st.phase === "speaking" || Date.now() < quietUntil) {
        segmenter.reset();
        return;
      }
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
          const prefs = useVoiceStore.getState().prefs;
          let cmd = stripWakeWord(text, prefs.wake_words);
          // What «Всегда слушает» heard and why it acted or not — for debugging from the log.
          logInfo(
            "wake",
            `${(utterance.length / sampleRate).toFixed(1)} s «${text.slice(0, 120)}» → ${cmd === null ? "без обращения" : `«${cmd}»`}`,
          );
          // Right after «Я здесь, сэр» the phrase is addressed to us even without the wake word.
          if (cmd === null && Date.now() < followUntil && text.trim()) cmd = text;
          followUntil = 0;
          if (useVoiceStore.getState().quiet) {
            // Quiet mode: only the phrase that ends it is honoured.
            const q = await api.quickMatch(cmd ?? text);
            if (q?.id === "voice.quiet_off") await handleUtterance(cmd ?? text);
            else useVoiceStore.getState().setPhase("listening");
            return;
          }
          if (cmd === null) {
            // Without the wake word only quick commands may run (if allowed in settings).
            if (!prefs.wake_required && text.trim()) {
              const q = await api.quickMatch(text);
              if (q && !q.needs_confirm) {
                if (q.stops_speech) stopSpeaking();
                const r = await api.quickRun(q.id, false, q.query);
                useVoiceStore.getState().setTranscript(text);
                useVoiceStore.getState().setAnswer(r.ok ? `⚡ ${r.name}` : `Команда «${r.name}» не выполнена: ${r.error ?? ""}`);
                if (r.ui) await showUi(r.ui);
              }
            }
            useVoiceStore.getState().setPhase("listening");
            return;
          }
          // A bare «Джарвис» works like «Джарвис, ты тут?»: a reply clip and a follow-up window.
          await handleUtterance(cmd || "ты тут");
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

// The microphone chosen in Settings: used by the next recording; the always-on stream reopens on it.
useVoiceStore.subscribe((s, prev) => {
  if (s.prefs.mic_device === prev.prefs.mic_device) return;
  setPreferredMic(s.prefs.mic_device);
  if (mic) void stopWakeListening().then(startWakeListening);
});

// ------------------------------------------------------------- calls --

/** On a call right now (another app holds the mic/camera); `byUs` — we switched quiet on. */
let call: { active: boolean; byUs: boolean } = { active: false, byUs: false };

/**
 * Auto-quiet on calls: when Teams/Telegram/… take the microphone or the camera, stop
 * talking and close the always-on stream (nothing is transcribed during the call); when
 * the call ends, return to the previous state. A quiet mode switched on by hand stays.
 */
export async function applyCallStatus(status: CallStatus): Promise<void> {
  const v = useVoiceStore.getState();
  if (!v.prefs.auto_quiet_calls) status = { in_call: false, apps: [] };
  if (status.in_call && !call.active) {
    call = { active: true, byUs: !v.quiet };
    stopSpeaking();
    if (call.byUs) v.setQuiet(true);
    await stopWakeListening();
    v.setAnswer(`Созвон (${status.apps.join(", ")}) — молчу и не слушаю.`);
    return;
  }
  if (!status.in_call && call.active) {
    const byUs = call.byUs;
    call = { active: false, byUs: false };
    if (byUs) v.setQuiet(false);
    await applyMode();
    v.setAnswer(byUs ? "Созвон закончился — снова слушаю." : "Созвон закончился.");
  }
}

/** Test hook: forget the call state between tests. */
export function resetCallState(): void {
  call = { active: false, byUs: false };
}

export function inCall(): boolean {
  return call.active;
}

/** Load shell preferences into the store (called at start and after Settings saves). */
export async function loadPrefs(): Promise<void> {
  try {
    useVoiceStore.getState().setPrefs(await api.prefsGet());
  } catch (e) {
    reportError("prefs", errorText(e));
  }
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
