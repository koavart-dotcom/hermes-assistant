import { useCallback, useEffect, useState } from "react";
import { Download, Loader2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { formatBytes } from "@/lib/format";
import { api, isTauri, onUpdateProgress, type UpdateProgress } from "@/services/tauri";
import type { UpdateInfo } from "@/types/generated/UpdateInfo";

/** Checks GitHub Releases at start-up (and every 6 h) and offers the update. */
export function UpdateBanner() {
  const [info, setInfo] = useState<UpdateInfo | null>(null);
  const [progress, setProgress] = useState<UpdateProgress | null>(null);
  const [installing, setInstalling] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const check = useCallback(() => {
    api
      .updateCheck()
      .then(setInfo)
      .catch((e) => console.warn("update check failed", e));
  }, []);

  useEffect(() => {
    if (!isTauri()) return;
    check();
    const timer = setInterval(check, 6 * 60 * 60 * 1000);
    let unlisten: (() => void) | undefined;
    onUpdateProgress(setProgress).then((u) => (unlisten = u));
    return () => {
      clearInterval(timer);
      unlisten?.();
    };
  }, [check]);

  if (!info?.available) return null;

  const install = () => {
    setInstalling(true);
    setError(null);
    api.updateInstall().catch((e) => {
      setError(e instanceof Error ? e.message : String(e));
      setInstalling(false);
    });
  };

  return (
    <div className="flex items-center gap-3 border-b bg-primary/10 px-4 py-2 text-sm" role="status" data-testid="update-banner">
      <Download className="size-4 shrink-0" aria-hidden />
      <span className="min-w-0 flex-1 truncate">
        Доступна версия {info.available} (у вас {info.current}).
        {installing && progress ? (
          <span className="text-muted-foreground">
            {" "}
            Скачивание: {formatBytes(progress.downloaded)}
            {progress.total ? ` из ${formatBytes(progress.total)}` : ""}
          </span>
        ) : null}
        {error ? <span className="text-destructive"> Ошибка: {error}</span> : null}
      </span>
      <Button size="sm" onClick={install} disabled={installing}>
        {installing ? <Loader2 className="mr-1 size-3.5 animate-spin" /> : null}
        {installing ? "Обновление…" : "Обновить"}
      </Button>
    </div>
  );
}
