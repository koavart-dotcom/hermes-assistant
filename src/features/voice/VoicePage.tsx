import { useEffect, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Ear, Keyboard, Loader2, Mic, Volume2, VolumeX } from "lucide-react";
import { PageShell } from "@/app/PageShell";
import { JarvisHud } from "@/features/voice/JarvisHud";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { confirmPendingQuick, onPendingQuick, pttStart, pttStop } from "@/lib/voicePipeline";
import { api, isTauri, onSetupEvent } from "@/services/tauri";
import { useVoiceStore, type VoiceMode } from "@/stores/voice";
import type { QuickMatch } from "@/types/generated/QuickMatch";

function errorText(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

const MODES: Array<{ id: VoiceMode; title: string; hint: string; icon: typeof Mic }> = [
  { id: "chat", title: "Чат", hint: "Только текст, микрофон выключен.", icon: Keyboard },
  { id: "ptt", title: "Голос по кнопке", hint: "Удерживайте кнопку или пробел и говорите.", icon: Mic },
  { id: "wake", title: "Всегда слушает", hint: "Реагирует на фразу, начинающуюся с «Джарвис» или «Гермес».", icon: Ear },
];

const PHASE_TEXT: Record<string, string> = {
  off: "Голос выключен, режим «Чат».",
  idle: "Готов. Удерживайте кнопку или пробел и говорите.",
  listening: "Слушаю. Скажите «Джарвис, …» или «Гермес, …».",
  recording: "Слушаю…",
  transcribing: "Распознаю…",
  thinking: "Hermes думает…",
  speaking: "Говорю…",
};

/** Voice page: mode selector, push-to-talk, state, last exchange. Audio work lives in voicePipeline. */
export function VoicePage() {
  const qc = useQueryClient();
  const status = useQuery({ queryKey: ["voice_status"], queryFn: api.voiceStatus, enabled: isTauri(), refetchInterval: 10_000 });
  const mode = useVoiceStore((s) => s.mode);
  const phase = useVoiceStore((s) => s.phase);
  const transcript = useVoiceStore((s) => s.transcript);
  const answer = useVoiceStore((s) => s.answer);
  const error = useVoiceStore((s) => s.error);
  const speak = useVoiceStore((s) => s.speak);
  const { setMode, setSpeak } = useVoiceStore.getState();
  const [pendingQuick, setPendingQuick] = useState<QuickMatch | null>(null);
  const [setupLog, setSetupLog] = useState<string[]>([]);
  const [settingUp, setSettingUp] = useState(false);

  useEffect(() => onPendingQuick(setPendingQuick), []);

  // Setup progress (shared setup event stream) while voice deps install.
  useEffect(() => {
    if (!isTauri() || !settingUp) return;
    let unlisten: (() => void) | undefined;
    onSetupEvent((ev) => {
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

  // Space = push-to-talk while this page is open (not in chat mode).
  useEffect(() => {
    if (mode === "chat") return;
    const down = (e: KeyboardEvent) => {
      if (e.code === "Space" && !e.repeat && (e.target as HTMLElement)?.tagName !== "TEXTAREA") {
        e.preventDefault();
        void pttStart();
      }
    };
    const up = (e: KeyboardEvent) => {
      if (e.code === "Space") {
        e.preventDefault();
        void pttStop();
      }
    };
    window.addEventListener("keydown", down);
    window.addEventListener("keyup", up);
    return () => {
      window.removeEventListener("keydown", down);
      window.removeEventListener("keyup", up);
    };
  }, [mode]);

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

  if (!isTauri()) {
    return <PageShell title="Голос" description="Голосовой режим работает только внутри приложения." />;
  }

  const s = status.data;
  const ready = Boolean(s?.deps_ready && s?.server_running);
  const canTalk = ready && mode !== "chat" && (phase === "idle" || phase === "listening" || phase === "recording" || phase === "speaking");

  return (
    <PageShell title="Голос" description="Режим работы, кнопка «нажми и говори», озвучка ответов.">
      <div className="mb-4 grid max-w-2xl gap-2 sm:grid-cols-3">
        {MODES.map(({ id, title, hint, icon: Icon }) => (
          <button
            key={id}
            type="button"
            onClick={() => setMode(id)}
            aria-pressed={mode === id}
            className={cn(
              "rounded-lg border p-3 text-left text-sm transition-colors",
              mode === id ? "border-primary bg-primary/10" : "hover:bg-accent/60",
            )}
          >
            <span className="flex items-center gap-2 font-medium">
              <Icon className="size-4" /> {title}
            </span>
            <span className="mt-1 block text-xs text-muted-foreground">{hint}</span>
          </button>
        ))}
      </div>

      <div className="mb-4 flex flex-wrap items-center gap-2 text-sm">
        <Badge variant={ready ? "default" : "secondary"}>
          {!s ? "…" : !s.hermes_installed ? "Hermes не установлен" : !s.deps_ready ? "голос не настроен" : s.server_running ? "готов" : "сервер голоса не запущен"}
        </Badge>
        {s ? (
          <span className="text-muted-foreground">
            распознавание: Whisper ({s.stt_provider}, ru) · озвучка: {s.tts_provider} ({s.tts_voice})
          </span>
        ) : null}
        <Button size="sm" variant="outline" onClick={() => setSpeak(!speak)} aria-pressed={speak}>
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
            "relative flex size-60 items-center justify-center rounded-full select-none outline-none focus-visible:ring-2 focus-visible:ring-ring",
            !canTalk && "opacity-40",
          )}
          disabled={!canTalk}
          onMouseDown={() => void pttStart()}
          onMouseUp={() => void pttStop()}
          onMouseLeave={() => phase === "recording" && void pttStop()}
          onTouchStart={() => void pttStart()}
          onTouchEnd={() => void pttStop()}
          aria-label="Нажмите и говорите"
        >
          <JarvisHud size={240} />
          {phase === "transcribing" || phase === "thinking" ? (
            <Loader2 className="absolute size-6 animate-spin text-foreground/70" aria-hidden />
          ) : null}
        </button>
        <Button size="sm" variant="outline" onClick={() => void api.hudWindowOpen()} disabled={!isTauri()}>
          Джарвис поверх окон
        </Button>
        <p className="text-sm text-muted-foreground" data-testid="voice-phase">
          {PHASE_TEXT[phase] ?? phase}
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
              <Button size="sm" onClick={() => void confirmPendingQuick(true)}>
                Да, выполнить
              </Button>
              <Button size="sm" variant="outline" onClick={() => void confirmPendingQuick(false)}>
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
