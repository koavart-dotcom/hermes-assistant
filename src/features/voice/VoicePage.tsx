import { useCallback, useEffect, useRef, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Loader2, Mic, Volume2, VolumeX } from "lucide-react";
import { PageShell } from "@/app/PageShell";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { startRecording, toBase64, type Recorder } from "@/lib/wav";
import { api, isTauri, onChatEvent, onSetupEvent } from "@/services/tauri";
import { useChatStore } from "@/stores/chat";
import type { QuickMatch } from "@/types/generated/QuickMatch";

type Phase = "idle" | "recording" | "transcribing" | "thinking" | "speaking";

function errorText(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

const SPEAK_KEY = "hermes.voice.speak";

/** Push-to-talk: hold the button (or Space) → Whisper → chat → Edge TTS. */
export function VoicePage() {
  const qc = useQueryClient();
  const status = useQuery({ queryKey: ["voice_status"], queryFn: api.voiceStatus, enabled: isTauri(), refetchInterval: 10_000 });
  const [phase, setPhase] = useState<Phase>("idle");
  const [level, setLevel] = useState(0);
  const [transcript, setTranscript] = useState("");
  const [answer, setAnswer] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [setupLog, setSetupLog] = useState<string[]>([]);
  const [settingUp, setSettingUp] = useState(false);
  const [pendingQuick, setPendingQuick] = useState<QuickMatch | null>(null);
  const [speak, setSpeak] = useState<boolean>(() => {
    try {
      return localStorage.getItem(SPEAK_KEY) !== "0";
    } catch {
      return true;
    }
  });
  const recorder = useRef<Recorder | null>(null);
  const waitingSession = useRef<string | null>(null);
  const audioRef = useRef<HTMLAudioElement | null>(null);
  const { addUserTurn, upsertSession, setActive } = useChatStore.getState();

  // Setup progress (shared setup event stream) while voice deps install.
  useEffect(() => {
    if (!isTauri()) return;
    let unlisten: (() => void) | undefined;
    onSetupEvent((ev) => {
      if (!settingUp) return;
      if (ev.kind === "log") setSetupLog((l) => [...l.slice(-200), ev.line]);
      if (ev.kind === "failed") {
        setSetupLog((l) => [...l, `Ошибка: ${ev.message}`]);
        setSettingUp(false);
      }
      if (ev.kind === "log" && ev.line === "voice: ready") {
        setSettingUp(false);
        qc.invalidateQueries({ queryKey: ["voice_status"] });
      }
    }).then((u) => (unlisten = u));
    return () => unlisten?.();
  }, [settingUp, qc]);

  // Answer of the turn we started by voice → speak it.
  useEffect(() => {
    if (!isTauri()) return;
    let unlisten: (() => void) | undefined;
    onChatEvent((env) => {
      if (env.session_id !== waitingSession.current) return;
      if (env.event.kind === "completed") {
        setAnswer(env.event.content);
        if (speak && env.event.content.trim()) {
          setPhase("speaking");
          api
            .voiceSpeak(env.event.content)
            .then((url) => {
              const a = new Audio(url);
              audioRef.current = a;
              a.onended = () => setPhase("idle");
              a.onerror = () => setPhase("idle");
              return a.play();
            })
            .catch((e) => {
              setError(`Озвучка: ${errorText(e)}`);
              setPhase("idle");
            });
        } else {
          setPhase("idle");
        }
      }
      if (env.event.kind === "failed") {
        setError(env.event.error);
        setPhase("idle");
      }
      if (env.event.kind === "ended") {
        waitingSession.current = null;
        setPhase((p) => (p === "thinking" ? "idle" : p));
      }
    }).then((u) => (unlisten = u));
    return () => unlisten?.();
  }, [speak]);

  const begin = useCallback(async () => {
    if (phase !== "idle" || !status.data?.server_running) return;
    setError(null);
    setTranscript("");
    setAnswer("");
    audioRef.current?.pause();
    try {
      const r = await startRecording();
      r.onLevel(setLevel);
      recorder.current = r;
      setPhase("recording");
    } catch (e) {
      setError(`Микрофон: ${errorText(e)}`);
    }
  }, [phase, status.data?.server_running]);

  const finish = useCallback(async () => {
    const r = recorder.current;
    if (!r) return;
    recorder.current = null;
    setPhase("transcribing");
    try {
      const { wav, seconds } = await r.stop();
      setLevel(0);
      if (seconds < 0.4) {
        setPhase("idle");
        return;
      }
      const text = await api.voiceTranscribe(toBase64(wav), "audio/wav");
      setTranscript(text);
      if (!text) {
        setPhase("idle");
        return;
      }
      // Quick command first: executed locally, no model involved.
      const quick = await api.quickMatch(text);
      if (quick) {
        if (quick.stops_speech) audioRef.current?.pause();
        if (quick.needs_confirm) {
          setPendingQuick(quick);
          setPhase("idle");
          return;
        }
        const r = await api.quickRun(quick.id, false);
        setAnswer(r.ok ? `Команда: ${r.name}` : `Команда «${r.name}» не выполнена: ${r.error ?? ""}`);
        setPhase("idle");
        return;
      }
      let sessionId = useChatStore.getState().activeId;
      if (!sessionId) {
        const s = await api.chatCreateSession("Голос");
        upsertSession(s);
        setActive(s.id);
        sessionId = s.id;
      }
      waitingSession.current = sessionId;
      addUserTurn(sessionId, text);
      setPhase("thinking");
      await api.chatSend(sessionId, text);
    } catch (e) {
      setError(errorText(e));
      setPhase("idle");
    }
  }, [addUserTurn, upsertSession, setActive]);

  // Space = push-to-talk while this page is open.
  useEffect(() => {
    const down = (e: KeyboardEvent) => {
      if (e.code === "Space" && !e.repeat && (e.target as HTMLElement)?.tagName !== "TEXTAREA") {
        e.preventDefault();
        void begin();
      }
    };
    const up = (e: KeyboardEvent) => {
      if (e.code === "Space") {
        e.preventDefault();
        void finish();
      }
    };
    window.addEventListener("keydown", down);
    window.addEventListener("keyup", up);
    return () => {
      window.removeEventListener("keydown", down);
      window.removeEventListener("keyup", up);
    };
  }, [begin, finish]);

  const runSetup = async () => {
    setSetupLog([]);
    setSettingUp(true);
    try {
      await api.voiceSetup();
    } catch (e) {
      setSetupLog([`Ошибка: ${errorText(e)}`]);
      setSettingUp(false);
    }
  };

  const toggleSpeak = () => {
    const v = !speak;
    setSpeak(v);
    try {
      localStorage.setItem(SPEAK_KEY, v ? "1" : "0");
    } catch {
      /* ignore */
    }
  };

  if (!isTauri()) {
    return <PageShell title="Голос" description="Голосовой режим работает только внутри приложения." />;
  }

  const s = status.data;
  const ready = Boolean(s?.deps_ready && s?.server_running);

  return (
    <PageShell title="Голос" description="Удерживайте кнопку (или пробел), говорите, отпустите — Hermes ответит текстом и голосом.">
      <div className="mb-4 flex flex-wrap items-center gap-2 text-sm">
        <Badge variant={ready ? "default" : "secondary"}>
          {!s ? "…" : !s.hermes_installed ? "Hermes не установлен" : !s.deps_ready ? "голос не настроен" : s.server_running ? "готов" : "сервер голоса не запущен"}
        </Badge>
        {s ? (
          <span className="text-muted-foreground">
            распознавание: {s.stt_provider} (Whisper, ru) · озвучка: {s.tts_provider} ({s.tts_voice})
          </span>
        ) : null}
        <Button size="sm" variant="outline" onClick={toggleSpeak} aria-pressed={speak}>
          {speak ? <Volume2 className="mr-1 size-4" /> : <VolumeX className="mr-1 size-4" />}
          {speak ? "Озвучивать ответы" : "Без озвучки"}
        </Button>
      </div>

      {s && s.hermes_installed && !ready ? (
        <div className="mb-6 max-w-xl rounded-lg border p-4 text-sm">
          <p>
            Для голоса нужно один раз установить распознавание речи (Faster-Whisper, около 1 ГБ с моделью) и
            озвучку (Piper, русский голос, работает без интернета).
          </p>
          <Button className="mt-3" onClick={runSetup} disabled={settingUp}>
            {settingUp ? <Loader2 className="mr-1 size-4 animate-spin" /> : null}
            {settingUp ? "Настройка…" : s.deps_ready ? "Запустить сервер голоса" : "Настроить голос"}
          </Button>
          {setupLog.length ? (
            <pre className="mt-3 max-h-48 overflow-auto rounded bg-muted p-2 text-xs">{setupLog.slice(-30).join("\n")}</pre>
          ) : null}
        </div>
      ) : null}

      <div className="flex max-w-xl flex-col items-center gap-4">
        <button
          type="button"
          className={cn(
            "flex size-32 items-center justify-center rounded-full border-4 text-primary-foreground transition-colors select-none",
            phase === "recording" ? "border-red-400 bg-red-500" : "border-primary/40 bg-primary",
            !ready && "opacity-40",
          )}
          disabled={!ready || (phase !== "idle" && phase !== "recording")}
          onMouseDown={() => void begin()}
          onMouseUp={() => void finish()}
          onMouseLeave={() => phase === "recording" && void finish()}
          onTouchStart={() => void begin()}
          onTouchEnd={() => void finish()}
          aria-label="Нажмите и говорите"
          style={{ boxShadow: phase === "recording" ? `0 0 0 ${Math.round(level * 24)}px rgba(239,68,68,0.35)` : undefined }}
        >
          {phase === "transcribing" || phase === "thinking" ? <Loader2 className="size-10 animate-spin" /> : <Mic className="size-10" />}
        </button>
        <p className="text-sm text-muted-foreground" data-testid="voice-phase">
          {phase === "idle" && "Готов. Удерживайте кнопку или пробел и говорите."}
          {phase === "recording" && "Слушаю…"}
          {phase === "transcribing" && "Распознаю…"}
          {phase === "thinking" && "Hermes думает…"}
          {phase === "speaking" && "Говорю…"}
        </p>
        {transcript ? (
          <p className="w-full rounded-md bg-muted px-3 py-2 text-sm">
            <span className="text-muted-foreground">Вы: </span>
            {transcript}
          </p>
        ) : null}
        {pendingQuick ? (
          <div className="w-full rounded-lg border border-amber-500/50 bg-amber-500/10 p-4 text-sm" role="dialog" aria-label="Подтверждение команды">
            <p className="font-medium">Выполнить «{pendingQuick.name}»?</p>
            <div className="mt-3 flex gap-2">
              <Button
                size="sm"
                onClick={() => {
                  const q = pendingQuick;
                  setPendingQuick(null);
                  api
                    .quickRun(q.id, true)
                    .then((r) => setAnswer(r.ok ? `Команда: ${r.name}` : `Команда «${r.name}» не выполнена: ${r.error ?? ""}`))
                    .catch((e) => setError(errorText(e)));
                }}
              >
                Да, выполнить
              </Button>
              <Button size="sm" variant="outline" onClick={() => setPendingQuick(null)}>
                Отмена
              </Button>
            </div>
          </div>
        ) : null}
        {answer ? (
          <p className="w-full whitespace-pre-wrap rounded-md border px-3 py-2 text-sm">
            <span className="text-muted-foreground">Hermes: </span>
            {answer}
          </p>
        ) : null}
        {error ? (
          <p className="text-sm text-destructive" role="alert">
            {error}
          </p>
        ) : null}
      </div>
    </PageShell>
  );
}
