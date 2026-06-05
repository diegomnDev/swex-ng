# Contributing to SWEX-NG

Thanks for your interest! This is a small, focused project — contributions that keep it
**native, minimal, and honest about the protocol** are very welcome.

## Ground rules

- **Don't invent protocol behaviour.** Anything about the game's wire format must be
  verifiable against [Xzandro/sw-exporter](https://github.com/Xzandro/sw-exporter) or a
  real capture. If you can't verify it, say so in the PR.
- **Never commit secrets.** The decryption key, signing keys, captures, and `.env` files
  stay out of git (see `.gitignore`).
- **Keep interception surgical.** Only `*.qpyou.cn` is ever decrypted.

## Dev setup

```bash
pnpm install
pnpm tauri dev
```

Prereqs: Rust stable, Node 20 + pnpm, `cmake` (`brew install cmake`), Xcode CLT.

## Before opening a PR

CI runs these — run them locally first:

```bash
cargo fmt   --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test  --manifest-path src-tauri/Cargo.toml --lib
pnpm exec tsc --noEmit
```

- Add/adjust tests for protocol or mapping changes. Mapping helpers are expected to
  match the original `mapping.js` — assert against values computed from it, not your own
  arithmetic (see `mapping.rs` tests).
- Keep commits small with clear messages.

## Good first contributions

- A Linux or Windows cert-trust + system-proxy module (mirror `macos.rs`).
- Porting `getArtifactEffect` sub-effects (documented TODO in `mapping.rs`).
- UI polish.
