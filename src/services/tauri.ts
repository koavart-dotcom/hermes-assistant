import { invoke } from "@tauri-apps/api/core";
import type { AppInfo } from "@/types/generated/AppInfo";

/** Thin typed wrappers over Tauri commands. Types come from Rust via ts-rs. */
export const api = {
  appInfo: () => invoke<AppInfo>("app_info"),
};

/** True when running inside the Tauri webview (false in the browser / vitest). */
export function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}
