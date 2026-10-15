/** Human-readable size for byte counts coming from Rust (bigint or number). */
export function formatBytes(value: bigint | number | null | undefined): string {
  if (value === null || value === undefined) return "—";
  const n = typeof value === "bigint" ? Number(value) : value;
  if (n < 1_000_000) return `${Math.round(n / 1000)} КБ`;
  if (n < 1_000_000_000) return `${(n / 1_000_000).toFixed(0)} МБ`;
  return `${(n / 1_000_000_000).toFixed(1)} ГБ`;
}

export function percent(received: bigint | number, total: bigint | number | null | undefined): number | null {
  if (total === null || total === undefined) return null;
  const t = Number(total);
  if (t <= 0) return null;
  return Math.min(100, Math.round((Number(received) / t) * 100));
}
