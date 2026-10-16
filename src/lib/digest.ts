/**
 * Helpdesk digest schedule: fires once per configured local time per day.
 * The check runs every 30 s from VoiceRuntime (the main window is always
 * open: the app autostarts), so no backend clock/timezone logic is needed.
 */

export const DIGEST_FIRED_KEY = "hermes.digest.fired";

/** `HH:MM` of a Date in local time. */
export function hhmm(d: Date): string {
  return `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
}

/** `YYYY-MM-DD HH:MM` key identifying one firing. */
export function fireKey(d: Date, time: string): string {
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")} ${time}`;
}

/**
 * Which configured time is due now (within a 2-minute window after it) and
 * has not fired yet today. `fired` holds keys from `fireKey`.
 */
export function dueTime(now: Date, times: string[], fired: Set<string>): string | null {
  const minutesNow = now.getHours() * 60 + now.getMinutes();
  for (const t of times) {
    const [h, m] = t.split(":").map(Number);
    if (h === undefined || m === undefined || !Number.isFinite(h) || !Number.isFinite(m)) continue;
    const minutes = h * 60 + m;
    if (minutesNow >= minutes && minutesNow - minutes <= 2 && !fired.has(fireKey(now, t))) return t;
  }
  return null;
}

export function readFired(): Set<string> {
  try {
    const raw = localStorage.getItem(DIGEST_FIRED_KEY);
    const arr: unknown = raw ? JSON.parse(raw) : [];
    return new Set(Array.isArray(arr) ? arr.filter((x): x is string => typeof x === "string") : []);
  } catch {
    return new Set();
  }
}

export function writeFired(fired: Set<string>): void {
  try {
    // keep the last 50 keys only
    localStorage.setItem(DIGEST_FIRED_KEY, JSON.stringify([...fired].slice(-50)));
  } catch {
    /* storage unavailable */
  }
}
