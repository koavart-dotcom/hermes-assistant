import type { ReactNode } from "react";

interface PageShellProps {
  title: string;
  description?: string;
  children?: ReactNode;
}

/** Common page header + content area. Keeps the empty screens of stage 1 consistent. */
export function PageShell({ title, description, children }: PageShellProps) {
  return (
    <div className="flex h-full flex-col">
      <header className="relative flex h-14 shrink-0 items-center gap-3 px-6">
        <span className="hud-label">Hermes</span>
        <span className="h-3 w-px bg-hud-line" aria-hidden />
        <h1 className="text-base font-semibold tracking-tight">{title}</h1>
        <span className="hud-hairline absolute inset-x-0 bottom-0" aria-hidden />
      </header>
      <div className="flex-1 p-6">
        {description ? <p className="mb-4 max-w-2xl text-sm text-muted-foreground">{description}</p> : null}
        {children}
      </div>
    </div>
  );
}
