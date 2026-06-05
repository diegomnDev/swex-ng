## What & why

<!-- What does this change and why? Link any issue. -->

## Protocol / mapping changes

<!-- If you touched decode, capture rules, or mapping: how did you VERIFY the
behaviour? (reference sw-exporter file, a real capture, a test). "Couldn't verify"
is an acceptable, honest answer — say so. -->

## Checklist

- [ ] `cargo fmt --manifest-path src-tauri/Cargo.toml` clean
- [ ] `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings` clean
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --lib` passes
- [ ] `pnpm exec tsc --noEmit` passes
- [ ] No secrets committed (key, signing keys, captures, `.env`)
