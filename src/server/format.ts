/** «5 мин назад» for unix seconds. */
export function ago(unix: number): string {
  const s = Math.max(0, Math.round(Date.now() / 1000 - unix));
  if (s < 60) return `${s} с назад`;
  if (s < 3600) return `${Math.round(s / 60)} мин назад`;
  if (s < 86400) return `${Math.round(s / 3600)} ч назад`;
  return new Date(unix * 1000).toLocaleString("ru-RU");
}
