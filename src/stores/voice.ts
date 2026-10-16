import { create } from "zustand";
import type { Prefs } from "@/types/generated/Prefs";

export const DEFAULT_PREFS: Prefs = {
  voice_volume: 0.8,
  silent: false,
  wake_words: ["джарвис", "гермес", "jarvis", "hermes"],
  wake_required: true,
  voice_pack: "Jarvis",
  digest: { enabled: false, times: ["09:00", "14:00", "17:00"], speak: true, telegram: true },
  claude_project: "hermes-assistant",
};

/** How the assistant takes input: text only, push-to-talk, or always listening for a wake word. */
export type VoiceMode = "chat" | "ptt" | "wake";
export type VoicePhase = "off" | "idle" | "listening" | "recording" | "transcribing" | "thinking" | "speaking";

interface VoiceState {
  mode: VoiceMode;
  phase: VoicePhase;
  level: number;
  transcript: string;
  answer: string;
  error: string | null;
  speak: boolean;
  /** Session the last voice turn was sent to (its answer is spoken). */
  waitingSession: string | null;
  /** Shell preferences (volume, silent, wake words); loaded from the backend. */
  prefs: Prefs;
  setPrefs: (p: Prefs) => void;
  setMode: (mode: VoiceMode) => void;
  setPhase: (phase: VoicePhase) => void;
  setLevel: (level: number) => void;
  setTranscript: (t: string) => void;
  setAnswer: (a: string) => void;
  setError: (e: string | null) => void;
  setSpeak: (v: boolean) => void;
  setWaitingSession: (id: string | null) => void;
}

const MODE_KEY = "hermes.voice.mode";
const SPEAK_KEY = "hermes.voice.speak";

function read<T extends string>(key: string, allowed: readonly T[], fallback: T): T {
  try {
    const v = localStorage.getItem(key);
    if (v && (allowed as readonly string[]).includes(v)) return v as T;
  } catch {
    /* storage unavailable */
  }
  return fallback;
}

export const useVoiceStore = create<VoiceState>((set) => ({
  mode: read(MODE_KEY, ["chat", "ptt", "wake"] as const, "ptt"),
  phase: "idle",
  level: 0,
  transcript: "",
  answer: "",
  error: null,
  speak: read(SPEAK_KEY, ["1", "0"] as const, "1") === "1",
  waitingSession: null,
  prefs: DEFAULT_PREFS,
  setPrefs: (prefs) => set({ prefs }),
  setMode: (mode) => {
    try {
      localStorage.setItem(MODE_KEY, mode);
    } catch {
      /* ignore */
    }
    set({ mode, error: null });
  },
  setPhase: (phase) => set({ phase }),
  setLevel: (level) => set({ level }),
  setTranscript: (transcript) => set({ transcript }),
  setAnswer: (answer) => set({ answer }),
  setError: (error) => set({ error }),
  setSpeak: (speak) => {
    try {
      localStorage.setItem(SPEAK_KEY, speak ? "1" : "0");
    } catch {
      /* ignore */
    }
    set({ speak });
  },
  setWaitingSession: (waitingSession) => set({ waitingSession }),
}));
