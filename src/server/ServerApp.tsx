import { useEffect, useState, type ComponentType } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Activity,
  Bell,
  KeyRound,
  Lock,
  LogOut,
  Server,
  ScrollText,
  ShieldAlert,
} from "lucide-react";
import { cn } from "cn";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { ApiError, api, clearToken, getToken, setToken } from "./api";
import { OverviewPage } from "./pages/OverviewPage";
import { IncidentsPage } from "./pages/IncidentsPage";
import { InventoryPage } from "./pages/InventoryPage";
import { WazuhPage } from "./pages/WazuhPage";
import { KeysPage } from "./pages/KeysPage";
import { LogsPage } from "./pages/LogsPage";
import { AccessPage } from "./pages/AccessPage";
import { Badge } from "@/components/ui/badge";
import { ErrorText } from "./ui";

interface Page {
  id: string;
  title: string;
  icon: ComponentType<{ className?: string }>;
  Component: ComponentType;
  admin?: boolean;
}

const PAGES: Page[] = [
  { id: "overview", title: "Обзор", icon: Activity, Component: OverviewPage },
  { id: "incidents", title: "Инциденты", icon: Bell, Component: IncidentsPage },
  { id: "inventory", title: "Инвентарь", icon: Server, Component: InventoryPage },
  { id: "wazuh", title: "Wazuh", icon: ShieldAlert, Component: WazuhPage },
  { id: "keys", title: "Модель и ключи", icon: KeyRound, Component: KeysPage },
  { id: "logs", title: "Журнал", icon: ScrollText, Component: LogsPage },
  { id: "access", title: "Доступ", icon: Lock, Component: AccessPage, admin: true },
];

function currentPage(): string {
  const id = window.location.hash.replace(/^#\/?/, "");
  return PAGES.some((p) => p.id === id) ? id : "overview";
}

function Login({ onDone }: { onDone: () => void }) {
  const [token, setValue] = useState("");
  const [remember, setRemember] = useState(false);
  const [error, setError] = useState<unknown>(null);
  const [busy, setBusy] = useState(false);
  const submit = async () => {
    setBusy(true);
    setError(null);
    try {
      await api.info(token.trim());
      setToken(token.trim(), remember);
      onDone();
    } catch (e) {
      setError(e instanceof ApiError && e.status === 401 ? new Error("Ключ не подошёл.") : e);
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="grid min-h-screen place-items-center bg-background p-4 text-foreground">
      <form
        className="hud-panel w-full max-w-md space-y-4 p-6"
        onSubmit={(e) => {
          e.preventDefault();
          void submit();
        }}
      >
        <div>
          <h1 className="text-lg font-semibold">Hermes Server</h1>
          <p className="mt-1 text-sm text-muted-foreground">
            Ключ доступа от администратора. На самом сервере подходит и мастер-ключ: файл{" "}
            <code className="text-xs">control.token</code> в
            <code className="text-xs">
              {" "}
              C:\ProgramData\Hermes Assistant Server\ua.koavart.hermes\
            </code>{" "}
            на сервере.
          </p>
        </div>
        <Input
          type="password"
          autoFocus
          autoComplete="off"
          placeholder="ключ"
          value={token}
          onChange={(e) => setValue(e.target.value)}
        />
        <label className="flex items-center gap-2 text-sm text-muted-foreground">
          <input
            type="checkbox"
            checked={remember}
            onChange={(e) => setRemember(e.target.checked)}
          />
          Запомнить в этом браузере
        </label>
        <ErrorText error={error} />
        <Button type="submit" className="w-full" disabled={busy || !token.trim()}>
          Войти
        </Button>
      </form>
    </div>
  );
}

export function ServerApp() {
  const [authed, setAuthed] = useState(() => getToken() !== "");
  const [page, setPage] = useState(currentPage);
  const qc = useQueryClient();
  const info = useQuery({ queryKey: ["info"], queryFn: () => api.info(), enabled: authed });
  const me = useQuery({ queryKey: ["whoami"], queryFn: api.whoami, enabled: authed });
  const readOnly = me.data?.role === "read";
  const pages = PAGES.filter((p) => !p.admin || !readOnly);

  useEffect(() => {
    const onHash = () => setPage(currentPage());
    window.addEventListener("hashchange", onHash);
    return () => window.removeEventListener("hashchange", onHash);
  }, []);

  // A token that stopped working (service reinstalled) sends us back to the login.
  const expired = info.error instanceof ApiError && info.error.status === 401;
  if (!authed || expired) {
    return (
      <Login
        onDone={() => {
          setAuthed(true);
          void qc.resetQueries();
        }}
      />
    );
  }

  const Current = pages.find((p) => p.id === page)?.Component ?? OverviewPage;
  const logout = () => {
    clearToken();
    qc.clear();
    setAuthed(false);
  };

  return (
    <div className="flex min-h-screen flex-col bg-background text-foreground md:flex-row">
      <aside className="flex shrink-0 flex-col gap-1 border-b border-sidebar-border bg-sidebar p-3 md:w-56 md:border-r md:border-b-0">
        <div className="px-2 pb-2">
          <div className="text-sm font-semibold">Hermes Server</div>
          <div className="text-xs text-muted-foreground">
            {info.data ? `v${info.data.version}` : "…"}
          </div>
          {me.data ? (
            <div className="mt-1 flex flex-wrap items-center gap-1 text-xs text-muted-foreground">
              {me.data.name}
              {readOnly ? <Badge variant="secondary">только чтение</Badge> : null}
            </div>
          ) : null}
        </div>
        <nav className="flex flex-wrap gap-1 md:flex-col">
          {pages.map((p) => (
            <a
              key={p.id}
              href={`#/${p.id}`}
              className={cn(
                "flex items-center gap-2 rounded-lg px-2 py-1.5 text-sm text-sidebar-foreground hover:bg-sidebar-accent",
                p.id === page && "bg-sidebar-accent font-medium",
              )}
            >
              <p.icon className="size-4" />
              {p.title}
            </a>
          ))}
        </nav>
        <div className="mt-auto hidden pt-4 md:block">
          <Button variant="ghost" size="sm" className="w-full justify-start" onClick={logout}>
            <LogOut /> Выйти
          </Button>
        </div>
      </aside>
      <main className="min-w-0 flex-1 space-y-4 p-4 md:p-6">
        {readOnly ? (
          <p className="rounded-lg border border-border bg-muted/40 px-3 py-2 text-sm text-muted-foreground">
            Ключ только для чтения: изменения и починка недоступны — сервер их отклонит.
          </p>
        ) : null}
        <Current />
      </main>
    </div>
  );
}
