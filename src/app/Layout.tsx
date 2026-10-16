import { NavLink, Outlet } from "react-router";
import { Moon, Sun } from "lucide-react";
import { cn } from "@/lib/utils";
import { routes } from "@/app/routes";
import { UpdateBanner } from "@/app/UpdateBanner";
import { ChatEventsBridge } from "@/app/ChatEventsBridge";
import { VoiceRuntime } from "@/app/VoiceRuntime";
import { useUiStore } from "@/stores/ui";
import { useVoiceStore } from "@/stores/voice";
import { JarvisHud } from "@/features/voice/JarvisHud";
import { useQuery } from "@tanstack/react-query";
import { api, isTauri } from "@/services/tauri";

export function Layout() {
  const theme = useUiStore((s) => s.theme);
  const setTheme = useUiStore((s) => s.setTheme);
  const dark = document.documentElement.classList.contains("dark");
  const phase = useVoiceStore((s) => s.phase);
  const info = useQuery({ queryKey: ["app_info"], queryFn: api.appInfo, enabled: isTauri(), staleTime: Infinity });
  const PHASE_LABEL: Record<string, string> = {
    off: "офлайн",
    idle: "готов",
    listening: "слушает",
    recording: "запись",
    transcribing: "распознаёт",
    thinking: "думает",
    speaking: "говорит",
  };

  return (
    <div className="flex h-full">
      <aside className="relative flex w-56 shrink-0 flex-col border-r bg-sidebar">
        <div className="flex h-14 items-center gap-3 px-4">
          <span className="relative inline-flex size-8 items-center justify-center" aria-hidden>
            <JarvisHud size={32} />
          </span>
          <span className="flex flex-col leading-tight">
            <span className="text-sm font-semibold tracking-[0.2em] uppercase">Hermes</span>
            <span className="text-[11px] text-muted-foreground" data-testid="sidebar-phase">
              {PHASE_LABEL[phase] ?? phase}
            </span>
          </span>
        </div>
        <span className="hud-hairline mx-3" aria-hidden />
        <nav className="flex flex-1 flex-col gap-0.5 px-2 pt-2" aria-label="Разделы">
          {routes.map(({ path, title, icon: Icon }) => (
            <NavLink
              key={path}
              to={path}
              className={({ isActive }) =>
                cn(
                  "relative flex items-center gap-2.5 rounded-md px-3 py-2 text-sm transition-colors duration-(--duration-base)",
                  isActive
                    ? "bg-hud-soft font-medium text-foreground before:absolute before:inset-y-1.5 before:left-0 before:w-0.5 before:rounded-full before:bg-hud before:shadow-[0_0_8px_var(--hud)]"
                    : "text-muted-foreground hover:bg-accent/60 hover:text-foreground",
                )
              }
            >
              <Icon className="size-4" aria-hidden />
              {title}
            </NavLink>
          ))}
        </nav>
        <div className="border-t p-2">
          <button
            type="button"
            onClick={() => setTheme(dark ? "light" : "dark")}
            className="flex w-full items-center gap-2 rounded-md px-3 py-2 text-sm text-muted-foreground hover:bg-accent/60 hover:text-foreground"
            aria-label="Переключить тему"
            title={`Тема: ${theme}`}
          >
            {dark ? <Sun className="size-4" /> : <Moon className="size-4" />}
            {dark ? "Светлая тема" : "Тёмная тема"}
          </button>
          {info.data ? <p className="px-3 pt-1 text-[11px] text-muted-foreground">v{info.data.version}</p> : null}
        </div>
      </aside>
      <main className="flex min-w-0 flex-1 flex-col overflow-hidden">
        <ChatEventsBridge />
        <VoiceRuntime />
        <UpdateBanner />
        <div className="min-h-0 flex-1 overflow-auto">
          <Outlet />
        </div>
      </main>
    </div>
  );
}
