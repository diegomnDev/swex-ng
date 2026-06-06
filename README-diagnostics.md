# Diagnostic mode — command discovery (CAPTURE-ALL + HUNT)

Off by default. Normal export is **unchanged** when these env vars are unset.

Use this to find a game command we don't handle yet. The motivating case: the
**World Guild Battle (WGB) defense** units are *not* in the `HubUserLogin` export
— they arrive in some *other* `gateway_c2.php` command sent only when you open
that screen, which we currently drop in the `else` branch of `handle_command`.

> **Note on the command name.** It is **not verified** against `sw-exporter` —
> its plugins only handle `GetGuildWarBattleLogByGuildId` /
> `GetGuildWarBattleLogByWizardId` (battle *logs*), never a defense-deck *set*.
> So we don't assume a name; the whole point of HUNT is to discover it (it may
> even be a newer command sw-exporter never saw). Nothing here widens the
> `*.qpyou.cn` interception scope.

## The two probes

1. **CAPTURE-ALL** (`SWEX_CAPTURE_ALL=1`) — dumps the full decrypted JSON of
   **every** command to `<out_dir>/captures/{epoch_ms}-{command}.json`
   (subdir auto-created). Disabled by default so it never fills the disk.

2. **HUNT** (`SWEX_HUNT_IDS="id1,id2,..."`) — for every decrypted payload,
   recursively searches for any of those `unit_id`s anywhere in the JSON. On a
   match it logs (level `success`, visible in the in-app log) the **command
   name**, the **ids found**, and the **JSON path(s)**, e.g.
   `deck_list[0].unit_id = 27391078482`. This pins down the command even when we
   don't know its name. Ids that com2us ships as strings also match.

The two are independent — use either or both.

## How to run

```bash
# from repo root
export SWEX_CAPTURE_ALL=1
export SWEX_HUNT_IDS="27391078482,6928412455,8469990197,5954832488,9242668568,26294927442,10421719528"
pnpm tauri dev
```

Then, **in the game** (proxied through the app as usual):

1. Log in once (writes the normal profile + starts capturing).
2. Open the **WGB / Guild Battle defense** screen — the one that shows your
   defense team(s). This is what triggers the unknown command.
3. Watch the in-app log for a `HUNT match in command '...'` line. That command
   name is the answer.

Stop the proxy, `unset SWEX_CAPTURE_ALL SWEX_HUNT_IDS`, restart for normal use.
(Flags are read once at proxy start, so toggling needs a restart.)

## Where output lands

`<out_dir>` is the export folder configured in the app (Settings → output
folder). Captures go to `<out_dir>/captures/`. Send me:

- the `HUNT match ...` log line(s), and
- the matching `captures/{epoch_ms}-{command}.json` file(s).

The test ids above are my real WGB defense (Craka `27391078482` is the one I set;
plus Geldnir, Ophilia, Theomars, Seara, Nora, Anavel). Craka should appear in a
defense team with two others — so the matching command is the one carrying it.

## Next steps (after we identify the command)

1. swex-ng merges that command's defense data into the export.
2. The sw-builder importer reads the new field.

Both depend on knowing the command name + structure, which this mode produces.
