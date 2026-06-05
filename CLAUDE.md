# CLAUDE.md — SWEX-NG

Guidance for AI assistants (and humans) working on this repo.

## What this is

Native macOS Summoners War profile exporter. React 19 frontend (`src/`) + Rust core
(`src-tauri/`) on Tauri 2. A pure-Rust MITM proxy decrypts the game's `gateway_c2.php`
response and writes the profile JSON. It's a reimplementation of
[Xzandro/sw-exporter](https://github.com/Xzandro/sw-exporter).

## Golden rule: don't invent protocol behaviour

Anything about the wire format, the mapping data, or the decrypt pipeline **must be
verified** against `sw-exporter` (the source of truth) or a real capture. If you can't
verify it, say so in the comment/PR — never guess APIs, versions, values, or behaviour.

## Hard rules

- **Never commit secrets.** The AES key is user-supplied (`SWEX_KEY` env or `key.hex` in
  the app data dir, git-ignored). The updater private key lives only in `~/.swex-ng-keys/`
  and as the `TAURI_SIGNING_PRIVATE_KEY` GitHub secret. Neither ever enters the repo.
- **Interception stays scoped to `*.qpyou.cn`** (`proxy.rs` `should_intercept`). Never widen it.

## Build & test

Prereqs: Rust stable, Node 20 + pnpm, `cmake` (`brew install cmake`, for `aws-lc-rs`),
Xcode Command Line Tools.

```bash
pnpm install
pnpm tauri dev      # run
pnpm tauri build    # native arm64 .app / .dmg
```

CI enforces these — run before pushing:

```bash
cargo fmt   --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test  --manifest-path src-tauri/Cargo.toml --lib
pnpm exec tsc --noEmit
```

Mapping/protocol tests assert **parity with the original** (values computed from
`mapping.js`), not your own arithmetic.

## Gotchas (learned the hard way)

- `cbc 0.2` / `aes 0.9` use the new `cipher` API: trait `BlockModeDecrypt` and
  `decrypt_padded_vec::<Pkcs7>` (NOT `BlockDecryptMut` / `decrypt_padded_vec_mut`).
- `mapping.json` `max` tables are JSON **objects** keyed `"1".."6"` — index by string key.
  `serde_json` `usize` indexing on an object returns null (this silently broke
  `rune_efficiency` once).
- macOS CA trust uses the **user login keychain** (silent, no admin), NOT the System
  keychain — osascript-spawned trust can't show its UI ("no user interaction possible").
- `tauri::generate_context!` embeds the icons at **compile time**; if `icons/*` are missing
  the build fails. The frontend is embedded compressed, so it won't appear in `strings`.
- Releases are **unsigned** (no Apple Developer ID yet); the updater payload IS signed
  with the Tauri key.

## Release

Tag `v*` (`git tag v0.1.0 && git push --tags`) → `.github/workflows/release.yml` builds the
universal (arm64 + Intel) DMG, signs the updater artifacts, and publishes the GitHub
Release + `latest.json`. Keep the version in sync across `package.json`,
`src-tauri/Cargo.toml`, and `src-tauri/tauri.conf.json`. After a local rebuild, replace
`/Applications/SWEX-NG.app` to test the fresh build.
