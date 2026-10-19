/**
 * Client of the hermesd control API for the web panel. The panel is served by
 * hermesd itself, so requests go to the same origin (`/control/v1`). The token
 * is the one in `control.token` on the server; it stays in this tab
 * (sessionStorage) unless «запомнить» puts it in localStorage.
 */

import type { DiagReport } from "@/types/generated/DiagReport";
import type { EnvKey } from "@/types/generated/EnvKey";
import type { FixAction } from "@/types/generated/FixAction";
import type { FixPlan } from "@/types/generated/FixPlan";
import type { FixReport } from "@/types/generated/FixReport";
import type { EnvPreset } from "@/types/generated/EnvPreset";
import type { Incident } from "@/types/generated/Incident";
import type { IngestOutcome } from "@/types/generated/IngestOutcome";
import type { Inventory } from "@/types/generated/Inventory";
import type { ModelOption } from "@/types/generated/ModelOption";
import type { ModelSettings } from "@/types/generated/ModelSettings";
import type { Probe } from "@/types/generated/Probe";
import type { StatusSnapshot } from "@/types/generated/StatusSnapshot";
import type { WazuhConfig } from "@/types/generated/WazuhConfig";

const BASE = "/control/v1";
const KEY = "hermes.server.token";

export class ApiError extends Error {
  constructor(
    readonly status: number,
    message: string,
  ) {
    super(message);
  }
}

function storages(): Storage[] {
  const out: Storage[] = [];
  try {
    out.push(sessionStorage, localStorage);
  } catch {
    /* storage unavailable */
  }
  return out;
}

export function getToken(): string {
  for (const s of storages()) {
    try {
      const t = s.getItem(KEY);
      if (t) return t;
    } catch {
      /* ignore */
    }
  }
  return "";
}

export function setToken(token: string, remember: boolean) {
  clearToken();
  const [session, local] = storages();
  try {
    (remember ? local : session)?.setItem(KEY, token);
  } catch {
    /* ignore */
  }
}

export function clearToken() {
  for (const s of storages()) {
    try {
      s.removeItem(KEY);
    } catch {
      /* ignore */
    }
  }
}

async function call<T>(
  method: string,
  path: string,
  body?: unknown,
  token = getToken(),
): Promise<T> {
  const res = await fetch(BASE + path, {
    method,
    headers: {
      Authorization: `Bearer ${token}`,
      "X-Hermes-Client": "web-panel",
      ...(body !== undefined ? { "Content-Type": "application/json" } : {}),
    },
    body: body !== undefined ? JSON.stringify(body) : undefined,
  });
  const text = await res.text();
  if (!res.ok) {
    let msg = text;
    try {
      msg = (JSON.parse(text) as { error?: string }).error ?? text;
    } catch {
      /* plain text */
    }
    throw new ApiError(res.status, msg || `HTTP ${res.status}`);
  }
  const type = res.headers.get("content-type") ?? "";
  return (type.includes("json") ? JSON.parse(text) : text) as T;
}

export interface ServerInfo {
  name: string;
  version: string;
  pin: { version: string; commit: string; branch: string };
  hermes_home: string;
  app_data_dir: string;
  log_dir: string;
  hermes_installed: boolean;
  setup_running: boolean;
  control_port: number;
}

export interface SetupEventStamped {
  seq: number;
  ts_unix: number;
  event: { kind: string; step?: string; line?: string; message?: string };
}

export interface WazuhIntegration {
  enabled: boolean;
  listen: string;
  hook_url: string;
  api_key: string;
  ossec_conf: string;
}

export interface WazuhConfigSaved {
  ok: boolean;
  listening: boolean;
  config: WazuhConfig;
}

export interface FixRequest {
  host: string;
  action: string;
  service?: string | null;
  incident_id?: number | null;
}

export const api = {
  info: (token?: string) => call<ServerInfo>("GET", "/info", undefined, token),
  status: () => call<StatusSnapshot>("GET", "/status"),
  setupEvents: (after = 0) =>
    call<{ events: SetupEventStamped[] }>("GET", `/setup/events?after=${after}&limit=200`).then(
      (r) => r.events,
    ),
  setupStart: () =>
    call<unknown>("POST", "/setup/start", { model: "", consent: true, autostart: false }),
  gatewayRestart: () => call<unknown>("POST", "/gateway/restart"),
  logs: (tail = 300) => call<string>("GET", `/logs?tail=${tail}`),

  wazuhConfig: () => call<WazuhConfig>("GET", "/wazuh/config"),
  wazuhConfigSave: (c: WazuhConfig) => call<WazuhConfigSaved>("PUT", "/wazuh/config", c),
  wazuhIntegration: () => call<WazuhIntegration>("GET", "/wazuh/integration"),
  wazuhIncidents: () => call<Incident[]>("GET", "/wazuh/incidents"),
  wazuhTest: (alert: unknown) => call<IngestOutcome>("POST", "/wazuh/alerts", alert),

  inventory: () => call<Inventory>("GET", "/inventory"),
  inventorySave: (inv: Inventory) => call<Inventory>("PUT", "/inventory", inv),
  probes: () => call<Probe[]>("GET", "/diag/probes"),
  sshKey: () => call<{ public_key: string }>("GET", "/diag/ssh-key"),
  diagRun: (host: string, probes: string[] = []) =>
    call<DiagReport>("POST", "/diag/run", { host, probes }),

  fixActions: () => call<FixAction[]>("GET", "/fix/actions"),
  fixPreview: (r: FixRequest) => call<FixPlan>("POST", "/fix/preview", r),
  fixRun: (r: FixRequest) => call<FixReport>("POST", "/fix/run", { ...r, confirm: true }),

  envKeys: () => call<EnvKey[]>("GET", "/hermes/env/keys"),
  envPresets: () => call<EnvPreset[]>("GET", "/hermes/env/presets"),
  envKeySet: (name: string, value: string) =>
    call<unknown>("PUT", "/hermes/env/keys", { name, value }),
  envKeyRemove: (name: string) =>
    call<unknown>("DELETE", `/hermes/env/keys/${encodeURIComponent(name)}`),
  modelOptions: () => call<ModelOption[]>("GET", "/models/options"),
  modelCurrent: () => call<ModelSettings>("GET", "/models/current"),
  modelApply: (m: ModelSettings) => call<ModelSettings>("PUT", "/models/current", m),
};

/** A test alert like the one in tools/wazuh/sample-alert.json. */
export function sampleAlert(host: string) {
  return {
    timestamp: new Date().toISOString(),
    id: `panel.${Date.now()}`,
    rule: {
      level: 12,
      id: "531",
      description: "Partition usage reached 100% (тестовый алерт из панели)",
      groups: ["ossec", "low_diskspace"],
    },
    agent: { id: "000", name: host || "test-host", ip: "" },
    location: "df -P",
    full_log: "ossec: output: 'df -P': /dev/sda1 51475068 51475068 0 100% /",
  };
}
