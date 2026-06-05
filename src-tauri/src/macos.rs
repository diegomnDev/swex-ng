//! macOS system integration that SWEX never did (its instructions made you do
//! steps 9-10 by hand + trust the cert in Keychain manually). Here we automate:
//!   * trusting the CA in the user's login keychain (`security add-trusted-cert`)
//!   * setting / clearing the HTTPS system proxy   (`networksetup`)
//!
//! Both run silently in the user's session: user-domain cert trust and
//! `networksetup` need no admin on current macOS, so there is no prompt at all.

use std::path::Path;
use std::process::Command;

#[derive(Debug, thiserror::Error)]
pub enum MacError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("command failed: {0}")]
    Cmd(String),
}

/// Returns the name of the network service bound to the default route
/// (e.g. "Wi-Fi"). Falls back to "Wi-Fi" if detection fails.
pub fn primary_service() -> String {
    // device of default route, e.g. "en0"
    let dev = Command::new("sh")
        .arg("-c")
        .arg("route -n get default 2>/dev/null | awk '/interface:/{print $2}'")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();

    if !dev.is_empty() {
        // map hardware port -> service name
        if let Ok(o) = Command::new("networksetup")
            .arg("-listallhardwareports")
            .output()
        {
            let text = String::from_utf8_lossy(&o.stdout);
            let mut current = String::new();
            for line in text.lines() {
                if let Some(name) = line.strip_prefix("Hardware Port: ") {
                    current = name.trim().to_string();
                } else if line.trim() == format!("Device: {dev}") && !current.is_empty() {
                    return current;
                }
            }
        }
    }
    "Wi-Fi".to_string()
}

/// True if the CA already evaluates as a trusted root, so a re-run of the proxy
/// skips re-trusting and just logs "CA already trusted".
pub fn is_ca_trusted(ca_pem_path: &Path) -> bool {
    // `-p basic`: a bare root CA (no leaf, no hostname) only passes the SSL
    // policy with a server name, so the default policy gives a false negative
    // even once it's trusted. The basic policy checks trust without that.
    Command::new("security")
        .arg("verify-cert")
        .arg("-p")
        .arg("basic")
        .arg("-c")
        .arg(ca_pem_path)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Fallback command for the rare case in-app trust fails (e.g. locked login
/// keychain). User trust domain, no sudo — same thing the app does.
pub fn manual_trust_command(ca_pem_path: &Path) -> String {
    format!(
        "security add-trusted-cert -r trustRoot -k ~/Library/Keychains/login.keychain-db '{}'",
        ca_pem_path.to_string_lossy()
    )
}

/// Trust the CA as a root in the user's login keychain (user trust domain).
///
/// This is the whole automation and it is silent: writing trust settings to the
/// user's *own*, already-unlocked login keychain needs no admin and shows no
/// dialog, yet SSL trust evaluation for apps the user runs (the game included)
/// consults the user trust domain. Verified: `add-trusted-cert` returns 0 with
/// no prompt and `verify-cert -p basic` then reports the root as trusted.
///
/// We deliberately do NOT touch the *System* (admin) trust domain: macOS refuses
/// to set it from an osascript-spawned process detached from the GUI session
/// (`SecTrustSettingsSetTrustSettings: no user interaction possible`). The user
/// domain is also what mitmproxy/Proxyman install into, and Apple exempts
/// user-installed roots from Certificate Transparency, so TLS is not blocked.
pub fn trust_certificate(ca_pem_path: &Path) -> Result<(), MacError> {
    let home = std::env::var("HOME").map_err(|_| MacError::Cmd("HOME not set".into()))?;
    let login_kc = format!("{home}/Library/Keychains/login.keychain-db");
    let out = Command::new("security")
        .arg("add-trusted-cert")
        .args(["-r", "trustRoot", "-k"])
        .arg(&login_kc)
        .arg(ca_pem_path)
        .output()?;
    if out.status.success() {
        Ok(())
    } else {
        Err(MacError::Cmd(
            String::from_utf8_lossy(&out.stderr).trim().to_string(),
        ))
    }
}

/// Enable the HTTPS system proxy on `service` pointing at host:port.
pub fn set_proxy(service: &str, host: &str, port: u16) -> Result<(), MacError> {
    let status = Command::new("networksetup")
        .args(["-setsecurewebproxy", service, host, &port.to_string()])
        .status()?;
    if !status.success() {
        return Err(MacError::Cmd("setsecurewebproxy failed".into()));
    }
    let status = Command::new("networksetup")
        .args(["-setsecurewebproxystate", service, "on"])
        .status()?;
    if !status.success() {
        return Err(MacError::Cmd("setsecurewebproxystate on failed".into()));
    }
    Ok(())
}

/// Disable the HTTPS system proxy on `service`.
pub fn unset_proxy(service: &str) -> Result<(), MacError> {
    let _ = Command::new("networksetup")
        .args(["-setsecurewebproxystate", service, "off"])
        .status()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Exercises the REAL app trust path against a real CA. Ignored by default
    // because it mutates the login keychain. Run explicitly:
    //   SWEX_CA=<path/to/ca.pem> cargo test --lib trust_certificate_really_trusts -- --ignored
    // Verify with `security dump-trust-settings` BEFORE (clean) and AFTER (the
    // SWEX root appears) — that store is authoritative; `verify-cert` caches.
    #[test]
    #[ignore = "mutates the login keychain; run explicitly with SWEX_CA set"]
    fn trust_certificate_really_trusts() {
        let ca = std::env::var("SWEX_CA").expect("set SWEX_CA to the ca.pem path");
        trust_certificate(std::path::Path::new(&ca))
            .expect("trust_certificate() should succeed silently (no admin, no prompt)");
    }
}
