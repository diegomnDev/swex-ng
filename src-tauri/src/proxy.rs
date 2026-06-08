//! MITM proxy core — Rust port of SWEX's `app/proxy/SWProxy.js`.
//!
//! Faithful to the original behaviour:
//!   * Only the Summoners War hosts (`*.qpyou.cn`) are intercepted; every other
//!     connection is tunnelled untouched (privacy + avoids breaking other apps,
//!     exactly like SWEX's onConnect logic).
//!   * Requests to `/api/gateway_c2.php` have their response captured, HTTP
//!     content-encoding removed, then decrypted and JSON-parsed.
//!   * The profile is the response to `HubUserLogin` / `GuestLogin`, written as
//!     `{wizard_name}-{wizard_id}.json` (same as profile-export.js), and a
//!     `profile-captured` event is emitted to the UI.

use http_body_util::{BodyExt, Full};
use hudsucker::{
    certificate_authority::RcgenAuthority,
    hyper::{Request, Response},
    rcgen::{Issuer, KeyPair},
    rustls::crypto::aws_lc_rs,
    Body, HttpContext, HttpHandler, Proxy, RequestOrResponse,
};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter};

use crate::cert::CaFiles;
use crate::decode::{decrypt_request, decrypt_response};

#[derive(Clone)]
pub struct SwHandler {
    pub key: [u8; 16],
    pub out_dir: PathBuf,
    pub app: AppHandle,
    is_gateway: bool,
    /// Diagnostic: when set (`SWEX_CAPTURE_ALL=1`), dump the decrypted JSON of
    /// EVERY command — not just login — to `out_dir/captures/`. Off by default
    /// so normal use never fills the disk. See README-diagnostics.md.
    capture_all: bool,
    /// Diagnostic: unit_ids to hunt for (`SWEX_HUNT_IDS="123,456"`). Every
    /// decrypted payload is searched recursively; matches are logged with the
    /// command name + JSON path so we can identify a command we don't know the
    /// name of yet (e.g. the WGB-defense set, which sw-exporter never captures).
    hunt_ids: Vec<i64>,
    /// `wizard_id -> profile file path`, recorded when a login profile is
    /// written. Lets a later command (e.g. the WGB-defense set, which arrives
    /// only when that screen is opened) re-open the same file and merge into it,
    /// mirroring sw-exporter's `mergeStorage` flow. Shared across the per-
    /// connection clones of this handler, hence `Arc<Mutex<_>>`.
    profiles: Arc<Mutex<HashMap<i64, PathBuf>>>,
    /// Decrypted gateway request paired with the next response on this same
    /// connection (only populated under capture-all). Mirrors sw-exporter, which
    /// buffers request+response and decrypts both in onResponseEnd; lets a capture
    /// record the request that produced a response — e.g. which unit_master_id a
    /// getUnitStats* answer is for (the monster id is in the request, never the
    /// response).
    pending_request: Option<serde_json::Value>,
}

impl SwHandler {
    pub fn new(key: [u8; 16], out_dir: PathBuf, app: AppHandle) -> Self {
        Self {
            key,
            out_dir,
            app,
            is_gateway: false,
            capture_all: env_capture_all(),
            hunt_ids: env_hunt_ids(),
            profiles: Arc::new(Mutex::new(HashMap::new())),
            pending_request: None,
        }
    }

    fn log(&self, level: &str, msg: impl Into<String>) {
        let _ = self.app.emit(
            "proxy-log",
            serde_json::json!({
                "level": level, "message": msg.into(),
            }),
        );
    }
}

impl HttpHandler for SwHandler {
    async fn should_intercept(&mut self, _ctx: &HttpContext, req: &Request<Body>) -> bool {
        // CONNECT authority is host:port. Only MITM the SW servers.
        req.uri()
            .authority()
            .map(|a| a.host().contains("qpyou.cn"))
            .unwrap_or(false)
    }

    async fn handle_request(
        &mut self,
        _ctx: &HttpContext,
        req: Request<Body>,
    ) -> RequestOrResponse {
        self.is_gateway = req.uri().path().contains("/api/gateway_c2.php");
        self.pending_request = None;
        // Under capture-all, buffer + decrypt the request so the upcoming response
        // capture can be paired with it (the monster a getUnitStats* answer is for
        // lives in the request, not the response). The ORIGINAL bytes are forwarded
        // unchanged — same length, so Content-Length stays valid.
        if self.is_gateway && self.capture_all {
            let (parts, body) = req.into_parts();
            let bytes = match body.collect().await {
                Ok(c) => c.to_bytes(),
                Err(_) => return Request::from_parts(parts, Body::empty()).into(),
            };
            let text = String::from_utf8_lossy(&bytes);
            self.pending_request = decrypt_request(&self.key, &text).ok();
            return Request::from_parts(parts, Body::from(Full::new(bytes))).into();
        }
        req.into()
    }

    async fn handle_response(&mut self, _ctx: &HttpContext, res: Response<Body>) -> Response<Body> {
        if !self.is_gateway {
            return res;
        }
        self.is_gateway = false;

        // Remove HTTP content-encoding (gzip/deflate) — equiv. of Proxy.gunzip.
        let res = match hudsucker::decode_response(res) {
            Ok(r) => r,
            Err(_) => return Response::new(Body::empty()),
        };

        let (mut parts, body) = res.into_parts();
        let bytes = match body.collect().await {
            Ok(c) => c.to_bytes(),
            Err(_) => return Response::from_parts(parts, Body::empty()),
        };

        // Decrypt a copy; the original bytes are forwarded to the game intact.
        let text = String::from_utf8_lossy(&bytes);
        match decrypt_response(&self.key, &text) {
            Ok(json) => self.handle_command(json),
            Err(e) => self.log("debug", format!("decrypt failed (ignored): {e}")),
        }

        // Content-Length may now be stale; let hyper reframe the body.
        parts.headers.remove(hyper::header::CONTENT_LENGTH);
        Response::from_parts(parts, Body::from(Full::new(bytes)))
    }
}

impl SwHandler {
    fn handle_command(&self, mut json: serde_json::Value) {
        let command = json.get("command").and_then(|c| c.as_str()).unwrap_or("");

        // --- Diagnostic mode (off by default) ---------------------------------
        // Runs for EVERY command before the normal login handling below, so we
        // can discover commands we don't handle yet (the WGB-defense set is not
        // captured by any sw-exporter plugin, so its name is unverified — these
        // two probes are how we find it from a real capture).
        if self.capture_all {
            self.capture_raw(command, &json);
        }
        self.run_hunt(command, &json);
        // ----------------------------------------------------------------------

        if command == "HubUserLogin" || command == "GuestLogin" {
            // profile-export.js checks building_list presence for completeness.
            if json.get("building_list").is_none() {
                self.log(
                    "error",
                    "Login data incomplete (no building_list). Retry by re-entering.",
                );
                return;
            }
            // Own the wizard fields so the immutable borrow of `json` ends here,
            // before we take it mutably for `sort_user_data` below.
            let (wid, wname) = {
                let wizard = json.get("wizard_info");
                let wid = wizard
                    .and_then(|w| w.get("wizard_id"))
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);
                let wname = wizard
                    .and_then(|w| w.get("wizard_name"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("profile")
                    .to_string();
                (wid, wname)
            };
            // Same as sw-exporter: sanitize the whole `name-id` then add `.json`.
            let filename = format!(
                "{}.json",
                crate::profile::sanitize_filename(&format!("{wname}-{wid}"))
            );
            let path = self.out_dir.join(&filename);

            // Match sw-exporter's `sortUserData` (its ProfileExport plugin runs
            // this by default): normalize com2us's object-shaped rune lists into
            // arrays and apply the in-game ordering before writing.
            crate::profile::sort_user_data(&mut json);

            match serde_json::to_vec_pretty(&json) {
                Ok(buf) => {
                    if let Err(e) = std::fs::write(&path, buf) {
                        self.log("error", format!("Could not write {filename}: {e}"));
                        return;
                    }
                    self.log("success", format!("Saved profile to {filename}"));
                    // Remember where this wizard's profile lives so a later
                    // command (WGB defense) can merge into the same file.
                    self.profiles.lock().unwrap().insert(wid, path.clone());
                    let _ = self.app.emit("profile-captured", serde_json::json!({
                        "wizard_id": wid,
                        "wizard_name": wname,
                        "path": path.to_string_lossy(),
                        "monster_count": json.get("unit_list").and_then(|u| u.as_array()).map(|a| a.len()).unwrap_or(0),
                        "rune_count": count_runes(&json),
                    }));
                }
                Err(e) => self.log("error", format!("serialize error: {e}")),
            }
        } else if command == "GetServerGuildWarDefenseDeckList" {
            // World Guild Battle defense decks. NOT present in the login payload
            // and NOT captured by any sw-exporter plugin — command name verified
            // only against a real capture. Arrives when the WGB-defense screen is
            // opened; we merge it into the already-written profile.
            self.merge_guildwar_defense(&json);
        } else {
            self.log("debug", format!("command {command}"));
        }
    }

    /// Merge a `GetServerGuildWarDefenseDeckList` payload into the profile file
    /// written at login. The WGB data is namespaced under a new top-level
    /// `guildwar_defense` key — it must NOT be spread at top level because the
    /// login payload already owns `deck_list` (arena/general decks). This export
    /// key is our own schema (to be confirmed against the sw-builder importer),
    /// not a com2us field name.
    fn merge_guildwar_defense(&self, json: &serde_json::Value) {
        let wid = json
            .get("target_wizard_id")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        let path = match self.profiles.lock().unwrap().get(&wid).cloned() {
            Some(p) => p,
            None => {
                self.log(
                    "warning",
                    format!(
                        "WGB defense received for wizard {wid}, but no profile saved yet this \
                         session — log in first, then reopen the WGB-defense screen."
                    ),
                );
                return;
            }
        };

        let mut profile: serde_json::Value = match std::fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
        {
            Some(v) => v,
            None => {
                self.log("error", format!("WGB merge: could not read {path:?}"));
                return;
            }
        };

        let deck_count = match merge_wgb_into(&mut profile, json) {
            Ok(n) => n,
            Err(e) => {
                self.log("error", format!("WGB merge: {e}"));
                return;
            }
        };

        match serde_json::to_vec_pretty(&profile) {
            Ok(buf) => match std::fs::write(&path, buf) {
                Ok(()) => {
                    self.log(
                        "success",
                        format!("Merged World Guild Battle defense ({deck_count} decks) into the profile."),
                    );
                    let _ = self.app.emit(
                        "profile-updated",
                        serde_json::json!({
                            "wizard_id": wid,
                            "path": path.to_string_lossy(),
                            "guildwar_defense_decks": deck_count,
                        }),
                    );
                }
                Err(e) => self.log("error", format!("WGB merge: write failed: {e}")),
            },
            Err(e) => self.log("error", format!("WGB merge: serialize failed: {e}")),
        }
    }

    /// CAPTURE-ALL: dump the full decrypted JSON of `command` to
    /// `out_dir/captures/{epoch_ms}-{command}.json` (subdir created on demand).
    fn capture_raw(&self, command: &str, json: &serde_json::Value) {
        let dir = self.out_dir.join("captures");
        if let Err(e) = std::fs::create_dir_all(&dir) {
            self.log("error", format!("capture: could not create {dir:?}: {e}"));
            return;
        }
        let ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let safe = crate::profile::sanitize_filename(if command.is_empty() {
            "unknown"
        } else {
            command
        });
        let name = format!("{ms}-{safe}.json");
        match serde_json::to_vec_pretty(json) {
            Ok(buf) => match std::fs::write(dir.join(&name), buf) {
                Ok(()) => self.log("info", format!("captured '{command}' -> captures/{name}")),
                Err(e) => self.log("error", format!("capture: write {name} failed: {e}")),
            },
            Err(e) => self.log("error", format!("capture: serialize {command} failed: {e}")),
        }
        // Paired request, same `{ms}-{command}` prefix with a `.request` suffix, so
        // a response that doesn't name its monster (getUnitStats*) can be matched to
        // the request that does.
        if let Some(req) = &self.pending_request {
            let rname = format!("{ms}-{safe}.request.json");
            match serde_json::to_vec_pretty(req) {
                Ok(buf) => {
                    if let Err(e) = std::fs::write(dir.join(&rname), buf) {
                        self.log("error", format!("capture: write {rname} failed: {e}"));
                    }
                }
                Err(e) => self.log(
                    "error",
                    format!("capture: serialize request {command} failed: {e}"),
                ),
            }
        }
    }

    /// HUNT: recursively search the decrypted payload for any of `hunt_ids`. On a
    /// match, emit a loud `success` log naming the command + the JSON path(s) —
    /// this reveals WHICH command carries those unit_ids even when we don't know
    /// its name in advance.
    fn run_hunt(&self, command: &str, json: &serde_json::Value) {
        if self.hunt_ids.is_empty() {
            return;
        }
        let mut hits = Vec::new();
        find_id_paths(json, &self.hunt_ids, "", &mut hits);
        if hits.is_empty() {
            return;
        }
        let mut ids: Vec<i64> = hits.iter().map(|(id, _)| *id).collect();
        ids.sort_unstable();
        ids.dedup();
        let id_list = ids
            .iter()
            .map(i64::to_string)
            .collect::<Vec<_>>()
            .join(", ");
        let paths = hits
            .iter()
            .map(|(id, p)| format!("  {p} = {id}"))
            .collect::<Vec<_>>()
            .join("\n");
        self.log(
            "success",
            format!("HUNT match in command '{command}' — ids [{id_list}] at:\n{paths}"),
        );
    }
}

/// Insert the WGB-defense blocks into `profile` under a `guildwar_defense` key,
/// keeping `deck_list` (round assignments) + `round_unit_list` (full builds).
/// Returns the number of defense decks. Errors if `profile` isn't a JSON object.
fn merge_wgb_into(
    profile: &mut serde_json::Value,
    cmd: &serde_json::Value,
) -> Result<usize, &'static str> {
    let obj = profile
        .as_object_mut()
        .ok_or("profile root is not a JSON object")?;
    obj.insert(
        "guildwar_defense".to_string(),
        serde_json::json!({
            "deck_list": cmd.get("deck_list").cloned().unwrap_or(serde_json::Value::Null),
            "round_unit_list": cmd.get("round_unit_list").cloned().unwrap_or(serde_json::Value::Null),
        }),
    );
    Ok(cmd
        .get("deck_list")
        .and_then(|d| d.as_array())
        .map(|a| a.len())
        .unwrap_or(0))
}

/// Total runes in a profile = free (top-level `runes`) + equipped
/// (`unit_list[*].runes`). The UI showed only the free count before, which looked
/// wrong (most runes are equipped). Runes under `guildwar_defense.round_unit_list`
/// are intentionally NOT counted: those units are already in `unit_list`, so adding
/// them would double up. Call after `sort_user_data` so rune lists are arrays.
fn count_runes(json: &serde_json::Value) -> usize {
    let arr_len =
        |v: Option<&serde_json::Value>| v.and_then(|x| x.as_array()).map_or(0, |a| a.len());
    let free = arr_len(json.get("runes"));
    let equipped: usize = json
        .get("unit_list")
        .and_then(|u| u.as_array())
        .map(|units| units.iter().map(|u| arr_len(u.get("runes"))).sum())
        .unwrap_or(0);
    free + equipped
}

/// `SWEX_CAPTURE_ALL` is on for `1`/`true`/`yes` (case-insensitive), else off.
fn env_capture_all() -> bool {
    std::env::var("SWEX_CAPTURE_ALL")
        .map(|v| {
            let v = v.trim().to_ascii_lowercase();
            v == "1" || v == "true" || v == "yes"
        })
        .unwrap_or(false)
}

/// Parse `SWEX_HUNT_IDS` — comma/space-separated i64s; non-numeric tokens skipped.
fn env_hunt_ids() -> Vec<i64> {
    std::env::var("SWEX_HUNT_IDS")
        .map(|raw| {
            raw.split([',', ' ', '\t', '\n'])
                .filter_map(|t| t.trim().parse::<i64>().ok())
                .collect()
        })
        .unwrap_or_default()
}

/// Recursively walk `val`, recording the JSON path of every scalar equal to one
/// of `targets`. Numbers match directly; strings match if they parse to the same
/// i64 (com2us sometimes ships ids as strings). Paths look like
/// `deck_list[0].unit_id`, so the matching command's structure is self-evident.
fn find_id_paths(
    val: &serde_json::Value,
    targets: &[i64],
    path: &str,
    hits: &mut Vec<(i64, String)>,
) {
    use serde_json::Value;
    match val {
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                if targets.contains(&i) {
                    hits.push((i, path.to_string()));
                }
            }
        }
        Value::String(s) => {
            if let Ok(i) = s.parse::<i64>() {
                if targets.contains(&i) {
                    hits.push((i, path.to_string()));
                }
            }
        }
        Value::Array(arr) => {
            for (idx, item) in arr.iter().enumerate() {
                find_id_paths(item, targets, &format!("{path}[{idx}]"), hits);
            }
        }
        Value::Object(map) => {
            for (k, v) in map {
                let child = if path.is_empty() {
                    k.clone()
                } else {
                    format!("{path}.{k}")
                };
                find_id_paths(v, targets, &child, hits);
            }
        }
        _ => {}
    }
}

/// Build and start the proxy. Resolves when `shutdown` fires.
pub async fn run_proxy(
    ca: &CaFiles,
    addr: SocketAddr,
    handler: SwHandler,
    shutdown: tokio::sync::oneshot::Receiver<()>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let key_pair = KeyPair::from_pem(&ca.key_pem)?;
    let issuer = Issuer::from_ca_cert_pem(&ca.cert_pem, key_pair)?;
    let authority = RcgenAuthority::new(issuer, 1_000, aws_lc_rs::default_provider());

    let proxy = Proxy::builder()
        .with_addr(addr)
        .with_ca(authority)
        .with_rustls_connector(aws_lc_rs::default_provider())
        .with_http_handler(handler)
        .with_graceful_shutdown(async move {
            let _ = shutdown.await;
        })
        .build()?;

    proxy.start().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::find_id_paths;
    use serde_json::json;

    #[test]
    fn finds_ids_in_nested_arrays_and_objects() {
        // Shape mimics an unknown command we're hunting for.
        let payload = json!({
            "command": "SomeUnknownCmd",
            "deck_list": [
                { "unit_id": 27391078482_i64, "pos": 1 },
                { "unit_id": 6928412455_i64,  "pos": 2 }
            ],
            "leader": { "unit_id": 8469990197_i64 },
            "noise": 12345
        });
        let targets = [27391078482_i64, 6928412455, 8469990197];
        let mut hits = Vec::new();
        find_id_paths(&payload, &targets, "", &mut hits);

        let paths: Vec<&str> = hits.iter().map(|(_, p)| p.as_str()).collect();
        assert!(paths.contains(&"deck_list[0].unit_id"));
        assert!(paths.contains(&"deck_list[1].unit_id"));
        assert!(paths.contains(&"leader.unit_id"));
        assert_eq!(hits.len(), 3, "should not match unrelated 'noise'");
    }

    #[test]
    fn matches_ids_shipped_as_strings() {
        let payload = json!({ "wrap": { "id": "5954832488" } });
        let mut hits = Vec::new();
        find_id_paths(&payload, &[5954832488_i64], "", &mut hits);
        assert_eq!(hits, vec![(5954832488_i64, "wrap.id".to_string())]);
    }

    #[test]
    fn counts_free_plus_equipped_runes_not_guildwar() {
        use super::count_runes;
        let profile = json!({
            "runes": [{"id": 1}, {"id": 2}, {"id": 3}],          // 3 free
            "unit_list": [
                {"unit_id": 10, "runes": [{"id": 4}, {"id": 5}]}, // 2 equipped
                {"unit_id": 11, "runes": [{"id": 6}]},            // 1 equipped
                {"unit_id": 12, "runes": []},                     // 0
            ],
            // Must NOT be counted (its unit is already in unit_list).
            "guildwar_defense": {
                "round_unit_list": [[{"unit_info": {"unit_id": 10, "runes": [{"id": 4}, {"id": 5}]}}]]
            }
        });
        assert_eq!(count_runes(&profile), 6); // 3 free + 3 equipped, guildwar ignored
    }

    #[test]
    fn wgb_merge_namespaces_without_clobbering_login_deck_list() {
        use super::merge_wgb_into;
        // Login profile already owns a top-level `deck_list` (arena decks).
        let mut profile = json!({
            "command": "HubUserLogin",
            "deck_list": ["arena-deck-A", "arena-deck-B"],
            "unit_list": []
        });
        let cmd = json!({
            "command": "GetServerGuildWarDefenseDeckList",
            "target_wizard_id": 6062946,
            "deck_list": [{ "round": 1, "unit_id_list": [1, 2, 3] }],
            "round_unit_list": [[{ "pos_id": 1, "unit_info": { "unit_id": 1 } }]]
        });

        let n = merge_wgb_into(&mut profile, &cmd).unwrap();
        assert_eq!(n, 1, "one defense deck");
        // Login's own deck_list is untouched.
        assert_eq!(profile["deck_list"][0], "arena-deck-A");
        // WGB data lives under its own namespace.
        assert_eq!(profile["guildwar_defense"]["deck_list"][0]["round"], 1);
        assert_eq!(
            profile["guildwar_defense"]["round_unit_list"][0][0]["unit_info"]["unit_id"],
            1
        );
    }
}
