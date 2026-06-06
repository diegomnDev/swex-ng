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

Tagging `v*` (`git tag vX.Y.Z && git push origin vX.Y.Z`) triggers
`.github/workflows/release.yml`, which builds the universal (arm64 + Intel) DMG, signs the
updater artifacts, and publishes the GitHub Release + `latest.json` (the in-app updater reads
this). Releases are **unsigned** by Apple; the updater payload IS signed with the Tauri key.

Do the steps below **in order** — the tag must point at the commit that already carries the
bumped version and changelog, or the published Release won't match.

1. **Green CI gate locally** (the same checks `ci.yml` runs — a red tag still publishes a
   broken Release):
   ```bash
   cargo fmt   --manifest-path src-tauri/Cargo.toml --check
   cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
   cargo test  --manifest-path src-tauri/Cargo.toml --lib
   pnpm exec tsc --noEmit
   ```
2. **Bump the version in all FOUR files** (they must stay in sync):
   `package.json`, `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`, and the `swex-ng`
   package entry in `src-tauri/Cargo.lock` (else the next build rewrites the lock and dirties
   the tree).
3. **Update `CHANGELOG.md`** — the Release body links to it, so it must be current. Add a
   `## [X.Y.Z] - YYYY-MM-DD` section (Keep a Changelog format: Added/Changed/Fixed), point the
   `[Unreleased]` compare link at the new tag, and add the `[X.Y.Z]: …/releases/tag/vX.Y.Z`
   ref.
4. **Commit + push to `main`** (branch protection requires the `check` status to pass — push
   the bump, let CI go green, only then tag).
5. **Tag + push the tag:** `git tag vX.Y.Z && git push origin vX.Y.Z`. Watch the run with
   `gh run watch` / verify the Release at `gh release view vX.Y.Z`.

Required repo secrets for `release.yml`: `TAURI_SIGNING_PRIVATE_KEY` **and**
`TAURI_SIGNING_PRIVATE_KEY_PASSWORD` (the local key in `~/.swex-ng-keys/updater.key` has an
**empty** password). `GITHUB_TOKEN` is provided automatically.

**DON'T install the locally-built bumped version into `/Applications` before releasing.** The
whole point of a release is to validate that the in-app auto-updater pulls the new version — but
that only works if the **previous** version is still the one installed. If you `cp` the freshly
built X.Y.Z `.app` into `/Applications` first, you're already on X.Y.Z and the updater has
nothing to do, so you can no longer test the update path for that version. Correct order:
leave the OLD version in `/Applications` → bump + tag + let `release.yml` publish → open the
running OLD app and confirm it detects and installs the update on its own.

Building locally is still fine for a quick smoke test, but either (a) do it from a build that is
NOT a version bump, or (b) install it somewhere other than `/Applications` so the real installed
copy stays on the prior version. The signing env vars must be set or `pnpm tauri build` fails
with *"no private key"*:
```bash
export TAURI_SIGNING_PRIVATE_KEY="$(cat ~/.swex-ng-keys/updater.key)"
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD=""
pnpm tauri build
# Only replace the installed app when you are NOT validating the updater for this version:
# rm -rf /Applications/SWEX-NG.app && cp -R src-tauri/target/release/bundle/macos/SWEX-NG.app /Applications/SWEX-NG.app
```
