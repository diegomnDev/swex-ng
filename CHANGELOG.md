# Changelog

All notable changes to this project are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-06-05

First public release. Native macOS, no Electron, no Rosetta.

### Added
- Pure-Rust decrypt pipeline (`aes-128-cbc` zero-IV + PKCS7 + zlib inflate), a faithful
  port of SWEX's `smon_decryptor.js`. Verified end-to-end against live game traffic.
- `hudsucker` MITM proxy scoped strictly to `*.qpyou.cn`; everything else tunnelled.
- Profile capture on `HubUserLogin` / `GuestLogin`, saved as `{wizard_name}-{wizard_id}.json`.
- Automatic macOS setup: silent CA trust in the user login keychain (no admin prompt)
  and system HTTPS proxy configuration, both reverted on Stop / app exit.
- Configurable output folder (persisted), with a native folder picker.
- In-app, signature-verified auto-updates via GitHub Releases.
- Ported `mapping.js` helpers: `monster_name`, `rune_efficiency` (ancient-aware), with
  tests asserting parity against the original.

[Unreleased]: https://github.com/diegomnDev/swex-ng/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/diegomnDev/swex-ng/releases/tag/v0.1.0
