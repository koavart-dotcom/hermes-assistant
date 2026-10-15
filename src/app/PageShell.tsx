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
      <header className="flex h-14 items-center border-b px-6">
        <h1 className="text-base font-semibold">{title}</h1>
      </header>
      <div className="flex-1 p-6">
        {description ? <p className="mb-4 text-sm text-muted-foreground">{description}</p> : null}
        {children}
      </div>
    </div>
  );
}
