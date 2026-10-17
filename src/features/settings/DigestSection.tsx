import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { CalendarClock, Loader2, Send } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import { runDigest } from "@/lib/digestRun";
import { api, isTauri } from "@/services/tauri";
import { useVoiceStore } from "@/stores/voice";
import type { Prefs } from "@/types/generated/Prefs";

function errorText(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

/** "Сводка хелпдеска": schedule, voice, Telegram; "send now" for a test. */
export function DigestSection() {
  const qc = useQueryClient();
  const enabled = isTauri();
  const prefs = useQuery({ queryKey: ["prefs"], queryFn: api.prefsGet, enabled });
  const [draft, setDraft] = useState<Prefs | null>(null);
  const [timesTyped, setTimesTyped] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [running, setRunning] = useState(false);

  const save = useMutation({
    mutationFn: (p: Prefs) => api.prefsSet(p),
    onSuccess: (p) => {
      setError(null);
      setDraft(null);
      setTimesTyped(null);
      useVoiceStore.getState().setPrefs(p);
      void qc.invalidateQueries({ queryKey: ["prefs"] });
    },
    onError: (e) => setError(errorText(e)),
  });

  const d = draft ?? prefs.data ?? null;
  const times = timesTyped ?? d?.digest.times.join(", ") ?? "";
  const changed = d && prefs.data && (JSON.stringify(d) !== JSON.stringify(prefs.data) || times !== prefs.data.digest.times.join(", "));
  const submit = () => {
    if (!d) return;
    save.mutate({ ...d, digest: { ...d.digest, times: times.split(/[,;\s]+/).map((t) => t.trim()).filter(Boolean) } });
  };
  const sendNow = async () => {
    setRunning(true);
    setError(null);
    try {
      await runDigest(true);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setRunning(false);
    }
  };

  return (
    <section className="hud-panel mt-6 max-w-xl space-y-3 p-4 text-sm" aria-labelledby="digest-title">
      <h2 id="digest-title" className="flex items-center gap-2 text-sm font-semibold tracking-tight">
        <CalendarClock className="size-4" /> Утренняя сводка
      </h2>
      <p className="text-muted-foreground">
        В указанное время Джарвис собирает встречи Google Calendar, непрочитанную почту Gmail, задачи Jira и заявки FreeScout:
        озвучивает итог, кладёт список в чат и шлёт в Telegram. Источники подключаются ключами в «Интеграции и ключи» —
        ненастроенные просто пропускаются. Для Telegram нужны TELEGRAM_BOT_TOKEN и TELEGRAM_CHAT_ID. Приложение должно быть запущено.
      </p>
      {!enabled ? <p className="text-muted-foreground">Доступно только внутри приложения.</p> : null}
      {d ? (
        <>
          <label className="flex items-center gap-2">
            <Switch checked={d.digest.enabled} onCheckedChange={(v) => setDraft({ ...d, digest: { ...d.digest, enabled: v } })} aria-label="Сводка по расписанию" />
            <span>Сводка по расписанию</span>
          </label>
          <label className="flex flex-col gap-1">
            <span className="text-xs text-muted-foreground">Время, через запятую (местное)</span>
            <Input value={times} onChange={(e) => setTimesTyped(e.target.value)} aria-label="Время сводки" placeholder="09:00, 14:00, 17:00" className="font-mono" />
          </label>
          <label className="flex items-center gap-2">
            <Switch checked={d.digest.speak} onCheckedChange={(v) => setDraft({ ...d, digest: { ...d.digest, speak: v } })} aria-label="Озвучивать сводку" />
            <span>Озвучивать итог голосом</span>
          </label>
          <label className="flex items-center gap-2">
            <Switch checked={d.digest.telegram} onCheckedChange={(v) => setDraft({ ...d, digest: { ...d.digest, telegram: v } })} aria-label="Слать в Telegram" />
            <span>Слать в Telegram</span>
          </label>
          <div className="flex flex-wrap gap-2">
            <Button onClick={submit} disabled={!changed || save.isPending}>
              {save.isPending ? <Loader2 className="mr-1 size-4 animate-spin" /> : null}
              Сохранить
            </Button>
            <Button variant="outline" onClick={() => void sendNow()} disabled={running || Boolean(changed)}>
              {running ? <Loader2 className="mr-1 size-4 animate-spin" /> : <Send className="mr-1 size-4" />}
              Сводка сейчас
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
