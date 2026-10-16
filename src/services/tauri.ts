import { invoke } from "@tauri-apps/api/core";
import { emit, listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { AppInfo } from "@/types/generated/AppInfo";
import type { EnvKey } from "@/types/generated/EnvKey";
import type { EnvPreset } from "@/types/generated/EnvPreset";
import type { TicketsSnapshot } from "@/types/generated/TicketsSnapshot";
import type { ChatEnvelope } from "@/types/generated/ChatEnvelope";
import type { ChatMessage } from "@/types/generated/ChatMessage";
import type { SessionSummary } from "@/types/generated/SessionSummary";
import type { SetupEvent } from "@/types/generated/SetupEvent";
import type { SetupOptions } from "@/types/generated/SetupOptions";
import type { SetupPlan } from "@/types/generated/SetupPlan";
import type { StatusSnapshot } from "@/types/generated/StatusSnapshot";
import type { UpdateInfo } from "@/types/generated/UpdateInfo";
import type { VoiceStatus } from "@/types/generated/VoiceStatus";
import type { QuickCommandInfo } from "@/types/generated/QuickCommandInfo";
import type { QuickMatch } from "@/types/generated/QuickMatch";
import type { QuickRunResult } from "@/types/generated/QuickRunResult";

export type ApprovalChoice = "once" | "session" | "always" | "deny";

/** Thin typed wrappers over Tauri commands. Types come from Rust via ts-rs. */
export const api = {
  appInfo: () => invoke<AppInfo>("app_info"),
  setupPlan: () => invoke<SetupPlan>("setup_plan"),
  setupStart: (options: SetupOptions) => invoke<void>("setup_start", { options }),
  setupCancel: () => invoke<void>("setup_cancel"),
  statusSnapshot: () => invoke<StatusSnapshot>("status_snapshot"),
  gatewayStart: () => invoke<void>("gateway_start"),
  gatewayStop: () => invoke<void>("gateway_stop"),

  chatSessions: () => invoke<SessionSummary[]>("chat_sessions"),
  chatCreateSession: (title?: string) =>
    invoke<SessionSummary>("chat_create_session", { title: title ?? null }),
  chatDeleteSession: (sessionId: string) => invoke<void>("chat_delete_session", { sessionId }),
  chatMessages: (sessionId: string) => invoke<ChatMessage[]>("chat_messages", { sessionId }),
  chatSend: (sessionId: string, message: string) =>
    invoke<void>("chat_send", { sessionId, message }),
  chatStop: (sessionId: string) => invoke<void>("chat_stop", { sessionId }),
  chatApprove: (runId: string, choice: ApprovalChoice) =>
    invoke<void>("chat_approve", { runId, choice }),

  updateCheck: () => invoke<UpdateInfo>("update_check"),
  updateInstall: () => invoke<void>("update_install"),

  voiceStatus: () => invoke<VoiceStatus>("voice_status"),
  voiceSetup: () => invoke<void>("voice_setup"),
  voiceTranscribe: (audioBase64: string, mimeType = "audio/wav") =>
    invoke<string>("voice_transcribe", { audioBase64, mimeType }),
  voiceSpeak: (text: string) => invoke<string>("voice_speak", { text }),

  quickCommands: () => invoke<QuickCommandInfo[]>("quick_commands"),
  quickMatch: (text: string) => invoke<QuickMatch | null>("quick_match", { text }),
  quickRun: (id: string, confirmed = false, query?: string | null) =>
    invoke<QuickRunResult>("quick_run", { id, confirmed, query: query ?? null }),
  quickImportVoices: (sourceDir: string, name?: string) =>
    invoke<number>("quick_import_voices", { sourceDir, name: name ?? null }),

  permissionsLog: (tail = 200) => invoke<PermissionRecord[]>("permissions_log", { tail }),
  permissionsPolicyGet: () => invoke<Record<string, unknown>>("permissions_policy_get"),
  permissionsPolicySet: (policy: Record<string, unknown>) =>
    invoke<void>("permissions_policy_set", { policy }),

  envKeysList: () => invoke<EnvKey[]>("env_keys_list"),
  envKeysPresets: () => invoke<EnvPreset[]>("env_keys_presets"),
  envKeysSet: (name: string, value: string) => invoke<void>("env_keys_set", { name, value }),
  envKeysRemove: (name: string) => invoke<void>("env_keys_remove", { name }),
  gatewayRestart: () => invoke<void>("gateway_restart"),

  ticketsFetch: () => invoke<TicketsSnapshot>("tickets_fetch"),
  ticketsWindowOpen: () => invoke<void>("tickets_window_open"),
  openUrl: (url: string) => invoke<void>("open_url", { url }),
  hudWindowOpen: () => invoke<void>("hud_window_open"),
  hudWindowClose: () => invoke<void>("hud_window_close"),
};

/** One decision line of the hermes_permissions plugin (hermes_permissions.log). */
export interface PermissionRecord {
  ts: number;
  tool: string;
  action: "allow" | "confirm" | "block";
  reason: string;
  preview: string;
  task_id: string;
}

export const UPDATE_EVENT = "update:progress";

export interface UpdateProgress {
  downloaded: number;
  total: number | null;
}

export function onUpdateProgress(handler: (p: UpdateProgress) => void): Promise<UnlistenFn> {
  return listen<UpdateProgress>(UPDATE_EVENT, (e) => handler(e.payload));
}

export const SETUP_EVENT = "setup:event";
export const CHAT_EVENT = "chat:event";

export function onSetupEvent(handler: (event: SetupEvent) => void): Promise<UnlistenFn> {
  return listen<SetupEvent>(SETUP_EVENT, (e) => handler(e.payload));
}

export function onChatEvent(handler: (envelope: ChatEnvelope) => void): Promise<UnlistenFn> {
  return listen<ChatEnvelope>(CHAT_EVENT, (e) => handler(e.payload));
}

/** True when running inside the Tauri webview (false in the browser / vitest). */
export function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/** Write a webview-side error into the application log (tauri-plugin-log). */
export function reportError(kind: string, message: string): void {
  if (!isTauri()) return;
  const where = typeof location !== "undefined" ? location.hash || "/" : "";
  invoke("plugin:log|log", {
    level: 5,
    message: `[webview ${where}] ${kind}: ${message}`,
    location: null,
    file: null,
    line: null,
    keyValues: null,
  }).catch(() => undefined);
}

/** Forward uncaught errors and rejected promises of this window to the log. */
export function installErrorReporting(): void {
  if (typeof window === "undefined") return;
  window.addEventListener("error", (e) => reportError("error", `${e.message} @ ${e.filename}:${e.lineno}`));
  window.addEventListener("unhandledrejection", (e) => reportError("unhandledrejection", String(e.reason)));
}

// ---- Cross-window voice state (main window → floating HUD window) ----

export interface VoiceStateEvent {
  phase: string;
  level: number;
}
export const VOICE_STATE_EVENT = "voice:state";
/** Button presses in the HUD window → main window (push-to-talk). */
export const HUD_PTT_EVENT = "hud:ptt";

export function emitVoiceState(state: VoiceStateEvent): Promise<void> {
  return emit(VOICE_STATE_EVENT, state);
}
export function onVoiceState(handler: (s: VoiceStateEvent) => void): Promise<UnlistenFn> {
  return listen<VoiceStateEvent>(VOICE_STATE_EVENT, (e) => handler(e.payload));
}
export function emitHudPtt(down: boolean): Promise<void> {
  return emit(HUD_PTT_EVENT, { down });
}
export function onHudPtt(handler: (down: boolean) => void): Promise<UnlistenFn> {
  return listen<{ down: boolean }>(HUD_PTT_EVENT, (e) => handler(e.payload.down));
}
