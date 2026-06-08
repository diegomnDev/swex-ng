//! Persisted user settings (`settings.json` in the app config dir).
//!
//! These were previously env-vars only (`SWEX_CAPTURE_ALL`, etc.). They're now a
//! UI-editable struct; the env vars still work and OVERRIDE the saved value
//! (handy for headless / power use). The output folder stays separate
//! (`out_dir.txt`) — it predates this and has its own commands.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// TCP port the local proxy listens on.
    pub port: u16,
    /// Show `debug`-level log lines (per-command + ignored-decrypt noise).
    pub verbose: bool,
    /// Dump every decrypted command to `captures/`.
    pub capture_all: bool,
    /// Comma/space-separated unit ids to hunt for in every payload.
    pub hunt_ids: String,
    /// Collect per-monster community rune/artifact stats to `runestats/`.
    pub runestats: bool,
    /// Also save the decrypted gateway request next to each capture.
    pub save_request: bool,
    /// Keep a timestamped copy of each profile under `profile saves/`.
    pub timestamped_copy: bool,
    /// Pretty-print written JSON (off = compact, smaller files).
    pub pretty_json: bool,
    /// Merge World Guild Battle defense into the profile when that screen opens.
    pub merge_wgb: bool,
    /// Show a macOS notification when a profile is captured.
    pub notify_on_capture: bool,
    /// Start the proxy automatically on launch (if a key is loaded).
    pub auto_start: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            port: 8080,
            verbose: false,
            capture_all: false,
            hunt_ids: String::new(),
            runestats: false,
            save_request: false,
            timestamped_copy: false,
            pretty_json: true,
            merge_wgb: true,
            notify_on_capture: false,
            auto_start: false,
        }
    }
}

fn settings_path(config_dir: &Path) -> PathBuf {
    config_dir.join("settings.json")
}

/// Load settings, falling back to defaults if the file is missing or invalid.
pub fn load(config_dir: &Path) -> Settings {
    std::fs::read(settings_path(config_dir))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

/// Persist settings as pretty JSON.
pub fn save(config_dir: &Path, settings: &Settings) -> Result<(), String> {
    std::fs::create_dir_all(config_dir).map_err(|e| e.to_string())?;
    let buf = serde_json::to_vec_pretty(settings).map_err(|e| e.to_string())?;
    std::fs::write(settings_path(config_dir), buf).map_err(|e| e.to_string())
}

/// Parse `hunt_ids` (comma/space/newline separated) into i64s, skipping junk.
pub fn parse_hunt_ids(raw: &str) -> Vec<i64> {
    raw.split([',', ' ', '\t', '\n', '\r'])
        .filter_map(|t| t.trim().parse::<i64>().ok())
        .collect()
}

/// True when `s` is a truthy env value (`1`/`true`/`yes`, case-insensitive).
fn env_truthy(s: &str) -> bool {
    let s = s.trim().to_ascii_lowercase();
    s == "1" || s == "true" || s == "yes"
}

/// Resolve the effective proxy config: saved settings, with env vars overriding
/// when present (so the documented `SWEX_*` flags still win for headless runs).
pub fn resolve(settings: &Settings) -> crate::proxy::HandlerConfig {
    let bool_env = |name: &str, fallback: bool| {
        std::env::var(name)
            .map(|v| env_truthy(&v))
            .unwrap_or(fallback)
    };
    let hunt_ids = match std::env::var("SWEX_HUNT_IDS") {
        Ok(v) if !v.trim().is_empty() => parse_hunt_ids(&v),
        _ => parse_hunt_ids(&settings.hunt_ids),
    };
    crate::proxy::HandlerConfig {
        verbose: settings.verbose,
        capture_all: bool_env("SWEX_CAPTURE_ALL", settings.capture_all),
        hunt_ids,
        runestats: bool_env("SWEX_RUNESTATS", settings.runestats),
        save_request: settings.save_request,
        timestamped_copy: settings.timestamped_copy,
        pretty_json: settings.pretty_json,
        merge_wgb: settings.merge_wgb,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_safe() {
        let s = Settings::default();
        assert_eq!(s.port, 8080);
        assert!(s.pretty_json && s.merge_wgb);
        assert!(!s.capture_all && !s.runestats && !s.verbose);
    }

    #[test]
    fn parse_hunt_ids_skips_junk() {
        assert_eq!(parse_hunt_ids("123, 456 789\nx"), vec![123, 456, 789]);
        assert!(parse_hunt_ids("").is_empty());
    }
}
