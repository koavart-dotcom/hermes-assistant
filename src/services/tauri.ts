import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { AppInfo } from "@/types/generated/AppInfo";
import type { SetupEvent } from "@/types/generated/SetupEvent";
import type { SetupOptions } from "@/types/generated/SetupOptions";
import type { SetupPlan } from "@/types/generated/SetupPlan";
import type { StatusSnapshot } from "@/types/generated/StatusSnapshot";

/** Thin typed wrappers over Tauri commands. Types come from Rust via ts-rs. */
export const api = {
  appInfo: () => invoke<AppInfo>("app_info"),
  setupPlan: () => invoke<SetupPlan>("setup_plan"),
  setupStart: (options: SetupOptions) => invoke<void>("setup_start", { options }),
  setupCancel: () => invoke<void>("setup_cancel"),
  statusSnapshot: () => invoke<StatusSnapshot>("status_snapshot"),
  gatewayStart: () => invoke<void>("gateway_start"),
  gatewayStop: () => invoke<void>("gateway_stop"),
};

export const SETUP_EVENT = "setup:event";

export function onSetupEvent(handler: (event: SetupEvent) => void): Promise<UnlistenFn> {
  return listen<SetupEvent>(SETUP_EVENT, (e) => handler(e.payload));
}

/** True when running inside the Tauri webview (false in the browser / vitest). */
export function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}
