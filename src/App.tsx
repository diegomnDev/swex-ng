import { useEffect, useRef, useState } from "react";
import { openPath } from "@tauri-apps/plugin-opener";
import {
  getStatus, setKey, startProxy, stopProxy, onLog, onProfile,
  setOutDir, resetOutDir, pickFolder, checkForUpdate, installUpdate,
  getAppVersion, openExternal, REPO_URL, onMenuCheckUpdate, notify,
  getSettings, setSettings as saveSettings, notifyCapture,
  type Status, type LogEntry, type ProfileCaptured, type Update, type Settings,
} from "./lib/api";

// Desktop/Documents/Downloads are the macOS TCC-protected folders that pop the
// "would like to access files" prompt (and re-prompt after each unsigned update).
const TCC_DIRS = ["/Desktop/", "/Documents/", "/Downloads/"];
const isTccPath = (p?: string) => !!p && TCC_DIRS.some((d) => p.includes(d));

function Toggle(props: {
  label: string; hint?: string; checked: boolean;
  disabled?: boolean; onChange: (v: boolean) => void;
}) {
  return (
    <label className="toggle">
      <input type="checkbox" checked={props.checked} disabled={props.disabled}
        onChange={(e) => props.onChange(e.target.checked)} />
      <span className="toggle-text">
        <b>{props.label}</b>
        {props.hint && <em>{props.hint}</em>}
      </span>
    </label>
  );
}

export default function App() {
  const [status, setStatus] = useState<Status | null>(null);
  const [settings, setSettingsState] = useState<Settings | null>(null);
  const [tab, setTab] = useState<"proxy" | "settings">("proxy");
  const [keyInput, setKeyInput] = useState("");
  const [logs, setLogs] = useState<LogEntry[]>([]);
  const [profile, setProfile] = useState<ProfileCaptured | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [update, setUpdate] = useState<Update | null>(null);
  const [updating, setUpdating] = useState(false);
  const [version, setVersion] = useState("");
  const logEnd = useRef<HTMLDivElement>(null);
  const settingsRef = useRef<Settings | null>(null);
  const autoStarted = useRef(false);

  const refresh = () => getStatus().then(setStatus);
  const port = settings?.port ?? 8080;

  useEffect(() => {
    refresh();
    getSettings().then((s) => { setSettingsState(s); settingsRef.current = s; });
    getAppVersion().then(setVersion);
    checkForUpdate().then(setUpdate);
    const unlisten = [
      onLog((e) => setLogs((l) => [...l.slice(-199), e])),
      onProfile((p) => {
        setProfile(p);
        if (settingsRef.current?.notify_on_capture) {
          void notifyCapture("Profile captured", `${p.wizard_name} · ${p.monster_count} monsters · ${p.rune_count} runes`);
        }
      }),
      onMenuCheckUpdate(async () => {
        const u = await checkForUpdate();
        setUpdate(u);
        if (!u) await notify("You're up to date.");
      }),
    ];
    return () => { unlisten.forEach((p) => p.then((u) => u())); };
  }, []);

  // Auto-start the proxy once, if enabled and a key is loaded.
  useEffect(() => {
    if (autoStarted.current || !settings?.auto_start) return;
    if (status?.key_loaded && !status.running) {
      autoStarted.current = true;
      startProxy(settings.port).then(refresh).catch((e) => setError(String(e)));
    }
  }, [settings, status]);

  useEffect(() => { logEnd.current?.scrollIntoView({ behavior: "smooth" }); }, [logs]);

  const patchSettings = (p: Partial<Settings>) => {
    if (!settings) return;
    const next = { ...settings, ...p };
    setSettingsState(next);
    settingsRef.current = next;
    saveSettings(next).catch((e) => setError(String(e)));
  };

  const doUpdate = async () => {
    if (!update) return;
    setError(null); setUpdating(true);
    try { await installUpdate(update); }
    catch (e) { setError(String(e)); setUpdating(false); }
  };

  const saveKey = async () => {
    setError(null);
    try { await setKey(keyInput.trim()); setKeyInput(""); await refresh(); }
    catch (e) { setError(String(e)); }
  };

  const toggle = async () => {
    setError(null); setBusy(true);
    try {
      if (status?.running) await stopProxy();
      else await startProxy(port);
      await refresh();
    } catch (e) { setError(String(e)); }
    finally { setBusy(false); }
  };

  const changeFolder = async () => {
    setError(null);
    try {
      const dir = await pickFolder(status?.out_dir);
      if (!dir) return;
      await setOutDir(dir);
      await refresh();
    } catch (e) { setError(String(e)); }
  };

  const useDefaultFolder = async () => {
    setError(null);
    try { await resetOutDir(); await refresh(); }
    catch (e) { setError(String(e)); }
  };

  const running = !!status?.running;

  return (
    <div className="app">
      <div className="topbar">
        <nav className="tabs">
          <button className={tab === "proxy" ? "tab on" : "tab"} onClick={() => setTab("proxy")}>Proxy</button>
          <button className={tab === "settings" ? "tab on" : "tab"} onClick={() => setTab("settings")}>Settings</button>
        </nav>
        <span className="spacer" />
        <span className={"dot " + (running ? "on" : "off")} />
        <span className="muted">{running ? "running" : "stopped"}</span>
      </div>

      {!status?.key_loaded && (
        <section className="card warn">
          <h2>Decryption key</h2>
          <div className="row">
            <input placeholder="32 hex chars (one-time) — see README" value={keyInput}
              onChange={(e) => setKeyInput(e.target.value)} spellCheck={false} />
            <button onClick={saveKey} disabled={keyInput.trim().length !== 32}>Save key</button>
          </div>
        </section>
      )}

      {tab === "proxy" ? (
        <>
          <section className="card">
            <h2>Proxy</h2>
            <div className="row">
              <label>Port</label>
              <input className="port" type="number" value={port}
                onChange={(e) => patchSettings({ port: Number(e.target.value) })} disabled={running} />
              <button className={running ? "stop" : "start"} onClick={toggle}
                disabled={busy || !status?.key_loaded}>
                {running ? "Stop" : "Start"}
              </button>
            </div>
          </section>

          {profile && (
            <div className="banner ok">
              <span className="banner-main">✓ <b>{profile.wizard_name}</b> · {profile.monster_count} monsters · {profile.rune_count} runes</span>
              <code className="path" title={profile.path}>{profile.path}</code>
              <button onClick={() => openPath(status!.out_dir)}>Open folder</button>
            </div>
          )}

          {error && <div className="error">{error}</div>}

          <section className="card grow">
            <h2>Log</h2>
            <div className="log">
              {logs.map((l, i) => <div key={i} className={"line " + l.level}>{l.message}</div>)}
              <div ref={logEnd} />
            </div>
          </section>
        </>
      ) : (
        <div className="settings">
          {error && <div className="error">{error}</div>}

          <section className="card">
            <h2>Output folder</h2>
            <div className="row">
              <button onClick={changeFolder} disabled={running}>Change…</button>
              <button onClick={useDefaultFolder} disabled={running}>Default</button>
              <button onClick={() => status && openPath(status.out_dir)}>Open</button>
            </div>
            <code className="path" title={status?.out_dir}>{status?.out_dir ?? "…"}</code>
            {isTccPath(status?.out_dir) && (
              <p className="hint warn-text">⚠️ This folder is under Desktop/Documents/Downloads — macOS will
                ask for permission, and re-ask after each app update (unsigned app). Pick another folder to avoid it.</p>
            )}
          </section>

          {settings && (
            <>
              <section className="card">
                <h2>Diagnostics</h2>
                <Toggle label="Verbose log" hint="Show debug lines (per-command, ignored decrypts)"
                  checked={settings.verbose} onChange={(v) => patchSettings({ verbose: v })} />
                <Toggle label="Capture every command" hint="Dump all decrypted commands to captures/"
                  checked={settings.capture_all} onChange={(v) => patchSettings({ capture_all: v })} />
                <Toggle label="Save raw requests" hint="Write the matching request next to each capture"
                  checked={settings.save_request} onChange={(v) => patchSettings({ save_request: v })} />
                <Toggle label="Collect rune/artifact stats" hint="Community per-monster stats → runestats/"
                  checked={settings.runestats} onChange={(v) => patchSettings({ runestats: v })} />
                <label className="field">
                  <span>Hunt unit IDs</span>
                  <input placeholder="e.g. 27391078482, 6928412455" value={settings.hunt_ids}
                    spellCheck={false} onChange={(e) => patchSettings({ hunt_ids: e.target.value })} />
                </label>
              </section>

              <section className="card">
                <h2>Files</h2>
                <Toggle label="Merge WGB defense" hint="Add guild-war defense to the profile"
                  checked={settings.merge_wgb} onChange={(v) => patchSettings({ merge_wgb: v })} />
                <Toggle label="Timestamped copy" hint="Keep a dated copy under profile saves/"
                  checked={settings.timestamped_copy} onChange={(v) => patchSettings({ timestamped_copy: v })} />
                <Toggle label="Pretty JSON" hint="Off = compact, smaller files"
                  checked={settings.pretty_json} onChange={(v) => patchSettings({ pretty_json: v })} />
              </section>

              <section className="card">
                <h2>Behavior</h2>
                <Toggle label="Notify on capture" hint="macOS notification when a profile is saved"
                  checked={settings.notify_on_capture} onChange={(v) => patchSettings({ notify_on_capture: v })} />
                <Toggle label="Auto-start proxy" hint="Start on launch if a key is loaded"
                  checked={settings.auto_start} onChange={(v) => patchSettings({ auto_start: v })} />
              </section>
              <p className="hint">Changes save automatically. Diagnostic/file options apply on the next proxy start.</p>
            </>
          )}
        </div>
      )}

      <footer className="footer">
        <span className="muted">derived from sw-exporter · Apache-2.0</span>
        <a className="link" onClick={() => openExternal(REPO_URL)}>GitHub</a>
        <a className="link" onClick={() => openExternal(REPO_URL + "/issues/new/choose")}>Report issue</a>
        <a className="link" onClick={() => openExternal(REPO_URL + "/blob/main/CHANGELOG.md")}>Changelog</a>
        <span className="spacer" />
        {update ? (
          <button className="link version" onClick={doUpdate} disabled={updating}
            title={`Update available: v${update.version}`}>
            <span className="dot on" />
            {updating ? "Updating…" : `v${version} → v${update.version}`}
          </button>
        ) : (
          <span className="muted">v{version || "—"}</span>
        )}
      </footer>
    </div>
  );
}
