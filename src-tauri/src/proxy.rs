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
use std::net::SocketAddr;
use std::path::PathBuf;
use tauri::{AppHandle, Emitter};

use crate::cert::CaFiles;
use crate::decode::decrypt_response;

#[derive(Clone)]
pub struct SwHandler {
    pub key: [u8; 16],
    pub out_dir: PathBuf,
    pub app: AppHandle,
    is_gateway: bool,
}

impl SwHandler {
    pub fn new(key: [u8; 16], out_dir: PathBuf, app: AppHandle) -> Self {
        Self {
            key,
            out_dir,
            app,
            is_gateway: false,
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
                    let _ = self.app.emit("profile-captured", serde_json::json!({
                        "wizard_id": wid,
                        "wizard_name": wname,
                        "path": path.to_string_lossy(),
                        "monster_count": json.get("unit_list").and_then(|u| u.as_array()).map(|a| a.len()).unwrap_or(0),
                        "rune_count": json.get("runes").and_then(|r| r.as_array()).map(|a| a.len()).unwrap_or(0),
                    }));
                }
                Err(e) => self.log("error", format!("serialize error: {e}")),
            }
        } else {
            self.log("debug", format!("command {command}"));
        }
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
