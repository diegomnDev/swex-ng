# Changelog

All notable changes to this project are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Community rune/artifact stats collection (`SWEX_RUNESTATS=1`, off by default). Captures the
  in-game per-monster recommendation stats (`getUnitStatsRuneInfo` / `getUnitStatsArtifactInfo`)
  — com2us's GLOBAL set/main-stat/sub-stat usage counts — and writes one clean, merged file per
  monster to `out_dir/runestats/{name}-{master_id}.json`. The monster is resolved from the
  paired request (the response itself never names it). This is community data, not account data,
  so it is kept entirely out of the profile export.

### Changed
- Diagnostic capture (`SWEX_CAPTURE_ALL=1`) now also decrypts and saves the matching
  `gateway_c2.php` **request** next to each response (`{ts}-{command}.request.json`). Some
  responses don't identify their subject — e.g. `getUnitStatsRuneInfo` carries community rune
  stats but the monster id is only in the request — so the pair is needed to attribute them.

### Fixed
- The capture summary now reports the **total** rune count (free + equipped) instead of only
  the free/unequipped runes. The previous figure was misleadingly low — most runes are
  equipped on monsters (`unit_list[*].runes`), not loose in `runes`. WGB-defense runes are not
  double-counted.

## [0.2.0] - 2026-06-06

### Added
- World Guild Battle (WGB) defense is now merged into the profile export. These decks are
  **not** in the `HubUserLogin` payload — they arrive in `GetServerGuildWarDefenseDeckList`
  only when that screen is opened (command name verified against a real capture; sw-exporter
  has no plugin for it). The login profile is written as before, then re-opened and enriched
  with a new top-level `guildwar_defense` key (`deck_list` + full per-unit `round_unit_list`),
  namespaced so it can't clobber the login's own `deck_list`. The original Summoners War
  Exporter doesn't capture this — a small bonus on top of full profile parity, not a
  competition.
- Diagnostic discovery mode (off by default): `SWEX_CAPTURE_ALL=1` dumps every decrypted
  command to `out_dir/captures/`, and `SWEX_HUNT_IDS="id,…"` logs the command + JSON path of
  any payload containing those unit ids. See `README-diagnostics.md`.

### Changed
- macOS app icon reshaped to the standard rounded-square (squircle) with safe-area padding.

### Fixed
- Profile export now matches sw-exporter's `sortUserData`. The game's PHP backend sometimes
  serializes rune lists as JSON **objects** keyed by arbitrary integers instead of arrays
  (com2us quirk); these are now coerced to arrays via the same `Object.values` logic as the
  original. `unit_list`, equipped/inventory runes, and `rune_craft_item_list` are sorted to the
  in-game order. Combined with `serde_json`'s `preserve_order` (server key order is preserved
  instead of alphabetized), exported profiles are now structurally identical to the original
  Summoners War Exporter. Verified live against a real capture.

[0.1.1]: https://github.com/diegomnDev/swex-ng/releases/tag/v0.1.1

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

[Unreleased]: https://github.com/diegomnDev/swex-ng/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/diegomnDev/swex-ng/releases/tag/v0.2.0
[0.1.0]: https://github.com/diegomnDev/swex-ng/releases/tag/v0.1.0
