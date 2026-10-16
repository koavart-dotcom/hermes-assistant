import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Loader2, RefreshCw } from "lucide-react";
import { PageShell } from "@/app/PageShell";
import { Button } from "@/components/ui/button";
import { EnvKeysSection } from "@/features/settings/EnvKeysSection";
import { ModelSection } from "@/features/settings/ModelSection";
import { SpeechSection } from "@/features/settings/SpeechSection";
import { VoiceSection } from "@/features/settings/VoiceSection";
import { api, isTauri } from "@/services/tauri";
import type { UpdateInfo } from "@/types/generated/UpdateInfo";

function errorText(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

export function SettingsPage() {
  const info = useQuery({ queryKey: ["app_info"], queryFn: api.appInfo, enabled: isTauri() });
  const [checking, setChecking] = useState(false);
  const [installing, setInstalling] = useState(false);
  const [result, setResult] = useState<UpdateInfo | null>(null);
  const [error, setError] = useState<string | null>(null);

  const check = async () => {
    setChecking(true);
    setError(null);
    try {
      setResult(await api.updateCheck());
    } catch (e) {
      setError(errorText(e));
    } finally {
      setChecking(false);
    }
  };

  const install = async () => {
    setInstalling(true);
    setError(null);
    try {
      await api.updateInstall();
    } catch (e) {
      setError(errorText(e));
      setInstalling(false);
    }
  };

  return (
    <PageShell
      title="Настройки"
      description="Модель, распознавание и озвучка, голос Джарвиса, ключи сервисов, обновления."
    >
      <section className="hud-panel max-w-xl space-y-3 p-4 text-sm">
        <h2 className="hud-label">Обновления</h2>
        <p>
          Версия приложения:{" "}
          <span data-testid="settings-version">
            {info.data ? info.data.version : isTauri() ? "…" : "браузерный режим"}
          </span>
        </p>
        <p className="text-muted-foreground">
          Приложение проверяет новые версии при запуске и раз в 6 часов. Обновление скачивается с
          GitHub, проверяется по подписи, устанавливается и перезапускает приложение.
        </p>
        <div className="flex flex-wrap items-center gap-2">
          <Button variant="outline" onClick={check} disabled={!isTauri() || checking || installing}>
            {checking ? (
              <Loader2 className="mr-1 size-4 animate-spin" />
            ) : (
              <RefreshCw className="mr-1 size-4" />
            )}
            Проверить обновления
          </Button>
          {result?.available ? (
            <Button onClick={install} disabled={installing}>
              {installing ? <Loader2 className="mr-1 size-4 animate-spin" /> : null}
              {installing ? "Обновление…" : `Обновить до ${result.available}`}
            </Button>
          ) : null}
        </div>
        {result && !result.available ? (
          <p className="text-muted-foreground">У вас последняя версия ({result.current}).</p>
        ) : null}
        {result?.available && result.notes ? (
          <pre className="whitespace-pre-wrap rounded-md bg-muted p-3 text-xs">{result.notes}</pre>
        ) : null}
        {error ? (
          <p className="text-destructive" role="alert">
            {error}
          </p>
        ) : null}
      </section>

      <section className="hud-panel mt-6 max-w-xl space-y-2 p-4 text-sm">
        <h2 className="hud-label">Автозапуск</h2>
        <p className="text-muted-foreground">
          Приложение регистрирует себя в автозагрузке Windows при каждом запуске (ключ реестра Run
          текущего пользователя), поэтому после перезагрузки Ollama и помощник поднимаются сами.
          Отключение появится позже.
        </p>
      </section>

      <ModelSection />
      <SpeechSection />
      <VoiceSection />
      <EnvKeysSection />
    </PageShell>
  );
}
