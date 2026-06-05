# Security Policy

## Reporting a vulnerability

Please **do not** open a public issue for security problems. Instead, use GitHub's
[private vulnerability reporting](https://github.com/diegomnDev/swex-ng/security/advisories/new)
for this repository. You'll get a response as soon as possible.

## What this app touches

SWEX-NG is a local MITM proxy for a single purpose: decrypting **your own** Summoners
War profile response. Its security posture:

- **The decryption key never ships in this repo.** It belongs to the game; you extract
  it once from your own SWEX install. It is stored locally (`key.hex` in the app data
  dir, git-ignored) or read from the `SWEX_KEY` environment variable.
- **Interception is scoped to `*.qpyou.cn`.** Every other host is tunnelled untouched —
  the proxy never decrypts or inspects unrelated traffic (`should_intercept` in
  `src-tauri/src/proxy.rs`).
- **Your data stays local.** Captured profiles are written to a folder you choose; the
  app makes no network calls of its own except the updater check against GitHub Releases.
- **The CA is trusted only in your user (login) trust domain**, not the System keychain,
  and is removed-able via Keychain Access at any time. The proxy resets the system HTTPS
  proxy on Stop and on app exit.

## Updater integrity

Auto-updates are **signature-verified** with a minisign key pair (Tauri updater). The
public key is embedded in the app; the private key exists only as a CI secret. An update
that doesn't verify is rejected.

## Releases are not yet Apple-notarized

Until the project has an Apple Developer ID, release binaries are unsigned and you must
bypass Gatekeeper on first launch (see the README). This does not affect updater
signature verification.
