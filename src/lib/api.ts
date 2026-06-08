import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getVersion } from "@tauri-apps/api/app";
import { open, message } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import {
  isPermissionGranted, requestPermission, sendNotification,
} from "@tauri-apps/plugin-notification";

export const REPO_URL = "https://github.com/diegomnDev/swex-ng";
export const getAppVersion = () => getVersion();
export const openExternal = (url: string) => openUrl(url);

export interface Status {
  running: boolean;
  key_loaded: boolean;
  out_dir: string;
}

export interface LogEntry {
  level: "info" | "success" | "warning" | "error" | "debug";
  message: string;
}

export interface ProfileCaptured {
  wizard_id: number;
  wizard_name: string;
  path: string;
  monster_count: number;
  rune_count: number;
}

/// Persisted settings — mirrors the Rust `Settings` struct (snake_case fields).
export interface Settings {
  port: number;
  verbose: boolean;
  capture_all: boolean;
  hunt_ids: string;
  runestats: boolean;
  save_request: boolean;
  timestamped_copy: boolean;
  pretty_json: boolean;
  merge_wgb: boolean;
  notify_on_capture: boolean;
  auto_start: boolean;
}

export const getSettings = () => invoke<Settings>("get_settings");
export const setSettings = (settings: Settings) => invoke<void>("set_settings", { settings });

/// Fire a native macOS notification, requesting permission on first use.
export const notifyCapture = async (title: string, body: string): Promise<void> => {
  let granted = await isPermissionGranted();
  if (!granted) granted = (await requestPermission()) === "granted";
  if (granted) sendNotification({ title, body });
};

export const getStatus = () => invoke<Status>("get_status");
export const setKey = (hex: string) => invoke<void>("set_key", { hex });
export const startProxy = (port: number) => invoke<void>("start_proxy", { port });
export const stopProxy = () => invoke<void>("stop_proxy");
export const setOutDir = (path: string) => invoke<void>("set_out_dir", { path });
export const resetOutDir = () => invoke<void>("reset_out_dir");

/// Open a native folder picker; returns the chosen path or null if cancelled.
export const pickFolder = async (defaultPath?: string): Promise<string | null> => {
  const sel = await open({ directory: true, multiple: false, defaultPath });
  return typeof sel === "string" ? sel : null;
};

export const onLog = (cb: (e: LogEntry) => void): Promise<UnlistenFn> =>
  listen<LogEntry>("proxy-log", (ev) => cb(ev.payload));

export const onProfile = (cb: (p: ProfileCaptured) => void): Promise<UnlistenFn> =>
  listen<ProfileCaptured>("profile-captured", (ev) => cb(ev.payload));

/// Fired when the user picks "Check for Updates…" from the native macOS menu.
export const onMenuCheckUpdate = (cb: () => void): Promise<UnlistenFn> =>
  listen("menu-check-update", () => cb());

/// Native macOS message dialog (used for the "up to date" reply).
export const notify = (text: string) => message(text, { title: "SWEX-NG", kind: "info" });

// --- Auto-update (Tauri updater) ---

/// Check GitHub Releases for a newer signed build. Returns the Update or null
/// (also null in dev / offline — errors are swallowed so the UI never breaks).
export const checkForUpdate = async (): Promise<Update | null> => {
  try {
    return await check();
  } catch {
    return null;
  }
};

/// Download + install the update (signature-verified by Tauri), then relaunch.
export const installUpdate = async (
  update: Update,
  onProgress?: (downloaded: number, total: number | null) => void,
): Promise<void> => {
  let downloaded = 0;
  let total: number | null = null;
  await update.downloadAndInstall((e) => {
    if (e.event === "Started") {
      total = e.data.contentLength ?? null;
    } else if (e.event === "Progress") {
      downloaded += e.data.chunkLength;
      onProgress?.(downloaded, total);
    }
  });
  await relaunch();
};

export type { Update };
