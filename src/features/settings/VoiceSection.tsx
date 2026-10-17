import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Loader2, Volume2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import { api, isTauri } from "@/services/tauri";
import { listMics } from "@/lib/wav";
import { useVoiceStore } from "@/stores/voice";
import type { Prefs } from "@/types/generated/Prefs";

function errorText(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

/** "Голос Джарвиса": reaction voice pack, volume, silent mode, wake words. */
export function VoiceSection() {
  const qc = useQueryClient();
  const enabled = isTauri();
  const prefs = useQuery({ queryKey: ["prefs"], queryFn: api.prefsGet, enabled });
  const packs = useQuery({ queryKey: ["voice_packs"], queryFn: api.voicePacks, enabled });
  const mics = useQuery({ queryKey: ["mics"], queryFn: listMics, enabled: typeof navigator !== "undefined" && !!navigator.mediaDevices });
  const [draft, setDraft] = useState<Prefs | null>(null);
  const [wakeTyped, setWakeText] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const save = useMutation({
    mutationFn: (p: Prefs) => api.prefsSet(p),
    onSuccess: (p) => {
      setError(null);
      setDraft(null);
      setWakeText(null);
      useVoiceStore.getState().setPrefs(p);
      void qc.invalidateQueries({ queryKey: ["prefs"] });
      void qc.invalidateQueries({ queryKey: ["voice_packs"] });
    },
    onError: (e) => setError(errorText(e)),
  });
  const importPack = useMutation({
    mutationFn: (name: string) => api.voicePackImport(name),
    onSuccess: () => {
      setError(null);
      void qc.invalidateQueries({ queryKey: ["voice_packs"] });
    },
    onError: (e) => setError(errorText(e)),
  });

  const d = draft ?? prefs.data ?? null;
  const wakeText = wakeTyped ?? d?.wake_words.join(", ") ?? "";
  const submit = () => {
    if (!d) return;
    save.mutate({
      ...d,
      wake_words: wakeText
        .split(/[,;]/)
        .map((w) => w.trim())
        .filter(Boolean),
    });
  };
  const changed = d && prefs.data && (JSON.stringify(d) !== JSON.stringify(prefs.data) || wakeText !== prefs.data.wake_words.join(", "));

  return (
    <section className="hud-panel mt-6 max-w-xl space-y-3 p-4 text-sm" aria-labelledby="voice-title">
      <h2 id="voice-title" className="flex items-center gap-2 text-sm font-semibold tracking-tight">
        <Volume2 className="size-4" /> Голос Джарвиса
      </h2>
      {!enabled ? <p className="text-muted-foreground">Доступно только внутри приложения.</p> : null}
      {d ? (
        <>
          <label className="flex flex-col gap-1">
            <span className="text-xs text-muted-foreground">Набор реакций (короткие фразы на команды)</span>
            <div className="flex flex-wrap items-center gap-2">
              <select className="h-8 rounded-lg border border-input bg-transparent px-2 text-sm" value={d.voice_pack} onChange={(e) => setDraft({ ...d, voice_pack: e.target.value })} aria-label="Набор реакций">
                {(packs.data?.installed ?? []).map((p) => (
                  <option key={p.name} value={p.name}>
                    {p.name} ({p.clips} клипов)
                  </option>
                ))}
                {!packs.data?.installed.some((p) => p.name === d.voice_pack) ? <option value={d.voice_pack}>{d.voice_pack}</option> : null}
              </select>
              {(packs.data?.importable ?? []).map((name) => (
                <Button key={name} size="sm" variant="outline" onClick={() => importPack.mutate(name)} disabled={importPack.isPending} title="Импортировать из Luxify Assistant">
                  {importPack.isPending ? <Loader2 className="mr-1 size-4 animate-spin" /> : null}+ {name}
                </Button>
              ))}
            </div>
            {packs.data?.importable.length ? <span className="text-xs text-muted-foreground">Кнопки «+» копируют голоса из Luxify Assistant с этого ПК.</span> : null}
          </label>
          <label className="flex flex-col gap-1">
            <span className="text-xs text-muted-foreground">Микрофон (уши Джарвиса)</span>
            <select className="h-8 rounded-lg border border-input bg-transparent px-2 text-sm" value={d.mic_device} onChange={(e) => setDraft({ ...d, mic_device: e.target.value })} aria-label="Микрофон">
              <option value="">Системный по умолчанию</option>
              {(mics.data ?? []).map((m) => (
                <option key={m} value={m}>
                  {m}
                </option>
              ))}
              {d.mic_device && !(mics.data ?? []).includes(d.mic_device) ? <option value={d.mic_device}>{d.mic_device} (не подключён)</option> : null}
            </select>
            <span className="text-xs text-muted-foreground">Для «Всегда слушает» удобен микрофон веб-камеры, для разговора — гарнитура. Отключённый микрофон заменяется системным.</span>
          </label>
          <label className="flex flex-col gap-1">
            <span className="text-xs text-muted-foreground">Громкость озвучки: {Math.round(d.voice_volume * 100)} %</span>
            <input type="range" min={0} max={100} value={Math.round(d.voice_volume * 100)} onChange={(e) => setDraft({ ...d, voice_volume: Number(e.target.value) / 100 })} aria-label="Громкость озвучки" />
          </label>
          <label className="flex items-center gap-2">
            <Switch checked={d.silent} onCheckedChange={(v) => setDraft({ ...d, silent: v })} aria-label="Тихий режим" />
            <span>Тихий режим: без реакций и озвучки ответов</span>
          </label>
          <label className="flex flex-col gap-1">
            <span className="text-xs text-muted-foreground">Горячая клавиша «нажми и говори» (из любого окна; пусто — выключена)</span>
            <Input value={d.hotkey} onChange={(e) => setDraft({ ...d, hotkey: e.target.value })} aria-label="Горячая клавиша" placeholder="CommandOrControl+Shift+Space" className="font-mono" />
            <span className="text-xs text-muted-foreground">Примеры: Ctrl+Alt+J, CommandOrControl+Shift+Space, F9. Удерживайте, говорите, отпустите.</span>
          </label>
          <label className="flex flex-col gap-1">
            <span className="text-xs text-muted-foreground">Слова-обращения, через запятую</span>
            <Input value={wakeText} onChange={(e) => setWakeText(e.target.value)} aria-label="Слова-обращения" placeholder="джарвис, гермес" />
          </label>
          <label className="flex items-center gap-2">
            <Switch checked={d.wake_required} onCheckedChange={(v) => setDraft({ ...d, wake_required: v })} aria-label="Требовать слово-обращение" />
            <span>В режиме «Всегда слушает» команды только после слова-обращения</span>
          </label>
          <p className="text-xs text-muted-foreground">Если выключить, быстрые команды («сверни окна», «громче») выполняются и без обращения; вопросы к модели по-прежнему требуют его.</p>
          <Button onClick={submit} disabled={!changed || save.isPending}>
            {save.isPending ? <Loader2 className="mr-1 size-4 animate-spin" /> : null}
            Сохранить
          </Button>
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
