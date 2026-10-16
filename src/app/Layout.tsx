import { NavLink, Outlet } from "react-router";
import { Moon, Sun } from "lucide-react";
import { cn } from "@/lib/utils";
import { routes } from "@/app/routes";
import { UpdateBanner } from "@/app/UpdateBanner";
import { ChatEventsBridge } from "@/app/ChatEventsBridge";
import { VoiceRuntime } from "@/app/VoiceRuntime";
import { useUiStore } from "@/stores/ui";

export function Layout() {
  const theme = useUiStore((s) => s.theme);
  const setTheme = useUiStore((s) => s.setTheme);
  const dark = document.documentElement.classList.contains("dark");

  return (
    <div className="flex h-full">
      <aside className="flex w-56 shrink-0 flex-col border-r bg-sidebar">
        <div className="flex h-14 items-center gap-2 px-4 text-base font-semibold">
          <span className="inline-block size-2.5 rounded-full bg-primary" aria-hidden />
          Hermes
        </div>
        <nav className="flex flex-1 flex-col gap-1 px-2" aria-label="Разделы">
          {routes.map(({ path, title, icon: Icon }) => (
            <NavLink
              key={path}
              to={path}
              className={({ isActive }) =>
                cn(
                  "flex items-center gap-2 rounded-md px-3 py-2 text-sm transition-colors",
                  isActive
                    ? "bg-accent text-accent-foreground font-medium"
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
