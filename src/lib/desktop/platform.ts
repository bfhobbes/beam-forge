import { invoke, isTauri } from "@tauri-apps/api/core";
import type { AppInfo } from "#lib/bindings/AppInfo.js";

/** True inside the Tauri window; false in the browser preview (`npm run dev`). */
export const desktop = isTauri();

// The browser preview has no Rust side: it says so instead of pretending.
const previewInfo: AppInfo = {
  appVersion: "preview",
  platform: "Browser preview",
  arch: "",
  core: { version: "unavailable", firmware: [] },
};

export async function appInfo(): Promise<AppInfo> {
  return desktop ? invoke<AppInfo>("app_info") : previewInfo;
}
