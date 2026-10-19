import type { DiagReport } from "@/types/generated/DiagReport";
import { Pre } from "./ui";

export function DiagResult({ report }: { report: DiagReport }) {
  return (
    <div className="space-y-2">
      <p className="text-sm">
        {report.error ? (
          <span className="text-destructive">Не подключился: {report.error}</span>
        ) : (
          <>
            {report.host}: {report.results.length} проверок за {report.duration_ms} мс,{" "}
            {report.results.filter((r) => !r.ok).length} с ошибкой
          </>
        )}
      </p>
      {report.results.map((r) => (
        <details key={r.id} className="rounded-lg border border-border">
          <summary className="cursor-pointer p-2 text-sm">
            {r.ok ? "✓" : "✗"} {r.title} <span className="text-muted-foreground">({r.id})</span>
          </summary>
          <Pre className="m-2 mt-0">{r.output || "(пусто)"}</Pre>
        </details>
      ))}
    </div>
  );
}
