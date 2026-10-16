import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { AudioLines, Loader2, Play } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { api, isTauri } from "@/services/tauri";
import type { SpeechSettings } from "@/types/generated/SpeechSettings";

function errorText(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

const STT_MODELS = ["tiny", "base", "small", "medium", "large-v3", "large-v3-turbo"];
const STT_HINT: Record<string, string> = {
  tiny: "очень быстро, много ошибок",
  base: "быстро, простые фразы",
  small: "баланс (по умолчанию)",
  medium: "точнее, 1–2 с дольше",
  "large-v3": "самая точная, ~3 ГБ",
  "large-v3-turbo": "точная и быстрая, ~1,6 ГБ",
};
const LANGS: Array<[string, string]> = [["ru", "русский"], ["uk", "украинский"], ["en", "английский"], ["auto", "определять"]];

const selectClass = "h-8 rounded-lg border border-input bg-transparent px-2 text-sm";

/** "Распознавание и озвучка": Whisper size/language and the TTS provider + voice of Hermes. */
export function SpeechSection() {
  const qc = useQueryClient();
  const enabled = isTauri();
  const current = useQuery({ queryKey: ["speech_current"], queryFn: api.speechCurrent, enabled });
  const tts = useQuery({ queryKey: ["tts_options"], queryFn: api.ttsOptions, enabled, staleTime: Infinity });
  const keys = useQuery({ queryKey: ["env_keys"], queryFn: api.envKeysList, enabled });
  const [draft, setDraft] = useState<SpeechSettings | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [testing, setTesting] = useState(false);

  const apply = useMutation({
    mutationFn: (s: SpeechSettings) => api.speechApply(s),
    onSuccess: (s) => {
      setError(null);
      setDraft(s);
      void qc.invalidateQueries({ queryKey: ["speech_current"] });
      void qc.invalidateQueries({ queryKey: ["voice_status"] });
    },
    onError: (e) => setError(errorText(e)),
  });

  const test = async () => {
    setTesting(true);
    setError(null);
    try {
      const url = await api.voiceSpeak("Привет, Артем. Это проверка голоса.");
      await new Audio(url).play();
    } catch (e) {
      setError(errorText(e));
    } finally {
      setTesting(false);
    }
  };

  const d = draft ?? current.data ?? null;
  const provider = (tts.data ?? []).find((p) => p.id === d?.tts_provider);
  const keyMissing = provider?.needs_key && !(keys.data ?? []).some((k) => k.name === provider.needs_key && k.masked);
  const changed = d && current.data && JSON.stringify(d) !== JSON.stringify(current.data);

  return (
    <section className="hud-panel mt-6 max-w-xl space-y-3 p-4 text-sm" aria-labelledby="speech-title">
      <h2 id="speech-title" className="flex items-center gap-2 text-sm font-semibold tracking-tight">
        <AudioLines className="size-4" /> Распознавание и озвучка
      </h2>
      <p className="text-muted-foreground">Распознаёт Whisper на вашей видеокарте. Изменения перезапускают сервер голоса; новая модель или голос скачиваются при первом использовании.</p>
      {!enabled ? <p className="text-muted-foreground">Доступно только внутри приложения.</p> : null}
      {d ? (
        <>
          <div className="grid gap-2 sm:grid-cols-2">
            <label className="flex flex-col gap-1">
              <span className="text-xs text-muted-foreground">Модель распознавания</span>
              <select className={selectClass} value={d.stt_model} onChange={(e) => setDraft({ ...d, stt_model: e.target.value })} aria-label="Модель распознавания">
                {STT_MODELS.map((m) => (
                  <option key={m} value={m}>
                    {m} — {STT_HINT[m]}
                  </option>
                ))}
              </select>
            </label>
            <label className="flex flex-col gap-1">
              <span className="text-xs text-muted-foreground">Язык речи</span>
              <select className={selectClass} value={d.stt_language} onChange={(e) => setDraft({ ...d, stt_language: e.target.value })} aria-label="Язык речи">
                {LANGS.map(([v, t]) => (
                  <option key={v} value={v}>
                    {t}
                  </option>
                ))}
              </select>
            </label>
            <label className="flex flex-col gap-1">
              <span className="text-xs text-muted-foreground">Озвучка</span>
              <select
                className={selectClass}
                value={d.tts_provider}
                aria-label="Озвучка"
                onChange={(e) => {
                  const p = (tts.data ?? []).find((x) => x.id === e.target.value);
                  setDraft({ ...d, tts_provider: e.target.value, tts_voice: p?.voices[0] ?? "" });
                }}
              >
                {(tts.data ?? []).map((p) => (
                  <option key={p.id} value={p.id}>
                    {p.title}
                  </option>
                ))}
              </select>
            </label>
            <label className="flex flex-col gap-1">
              <span className="text-xs text-muted-foreground">Голос</span>
              {provider && provider.voices.length ? (
                <select className={selectClass} value={d.tts_voice} onChange={(e) => setDraft({ ...d, tts_voice: e.target.value })} aria-label="Голос">
                  {!provider.voices.includes(d.tts_voice) && d.tts_voice ? <option value={d.tts_voice}>{d.tts_voice}</option> : null}
                  {provider.voices.map((v) => (
                    <option key={v} value={v}>
                      {v}
                    </option>
                  ))}
                </select>
              ) : (
                <Input className="font-mono" aria-label="Голос" value={d.tts_voice} onChange={(e) => setDraft({ ...d, tts_voice: e.target.value })} placeholder="voice id" />
              )}
            </label>
          </div>
          {provider ? <p className="text-xs text-muted-foreground">{provider.hint}</p> : null}
          {keyMissing ? <p className="text-xs text-amber-600">Нет ключа {provider?.needs_key} — задайте его в «Интеграции и ключи».</p> : null}
          <div className="flex flex-wrap gap-2">
            <Button onClick={() => apply.mutate(d)} disabled={!changed || apply.isPending || Boolean(keyMissing)}>
              {apply.isPending ? <Loader2 className="mr-1 size-4 animate-spin" /> : null}
              {apply.isPending ? "Применяю…" : "Применить"}
            </Button>
            <Button variant="outline" onClick={() => void test()} disabled={testing || Boolean(changed)}>
              {testing ? <Loader2 className="mr-1 size-4 animate-spin" /> : <Play className="mr-1 size-4" />}
              Проверить голос
            </Button>
          </div>
        </>
      ) : null}
      {error ? (
        <p className="text-destructive" role="alert">
          {error}
        </p>
      ) : null}
    </section>
  );
}
