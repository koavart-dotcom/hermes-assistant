import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { AppInfo } from "@/types/generated/AppInfo";
import type { ChatEnvelope } from "@/types/generated/ChatEnvelope";
import type { ChatMessage } from "@/types/generated/ChatMessage";
import type { SessionSummary } from "@/types/generated/SessionSummary";
import type { SetupEvent } from "@/types/generated/SetupEvent";
import type { SetupOptions } from "@/types/generated/SetupOptions";
import type { SetupPlan } from "@/types/generated/SetupPlan";
import type { StatusSnapshot } from "@/types/generated/StatusSnapshot";
import type { UpdateInfo } from "@/types/generated/UpdateInfo";

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
  chatCreateSession: (title?: string) => invoke<SessionSummary>("chat_create_session", { title: title ?? null }),
  chatDeleteSession: (sessionId: string) => invoke<void>("chat_delete_session", { sessionId }),
  chatMessages: (sessionId: string) => invoke<ChatMessage[]>("chat_messages", { sessionId }),
  chatSend: (sessionId: string, message: string) => invoke<void>("chat_send", { sessionId, message }),
  chatStop: (sessionId: string) => invoke<void>("chat_stop", { sessionId }),
  chatApprove: (runId: string, choice: ApprovalChoice) => invoke<void>("chat_approve", { runId, choice }),

  updateCheck: () => invoke<UpdateInfo>("update_check"),
  updateInstall: () => invoke<void>("update_install"),
};

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
