import { useEffect, useRef, useState } from "react";
import { openPath } from "@tauri-apps/plugin-opener";
import {
  getStatus, setKey, startProxy, stopProxy, onLog, onProfile,
  setOutDir, resetOutDir, pickFolder, checkForUpdate, installUpdate,
  getAppVersion, openExternal, REPO_URL, onMenuCheckUpdate, notify,
  type Status, type LogEntry, type ProfileCaptured, type Update,
} from "./lib/api";

export default function App() {
  const [status, setStatus] = useState<Status | null>(null);
  const [keyInput, setKeyInput] = useState("");
  const [port, setPort] = useState(8080);
  const [logs, setLogs] = useState<LogEntry[]>([]);
  const [profile, setProfile] = useState<ProfileCaptured | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [update, setUpdate] = useState<Update | null>(null);
  const [updating, setUpdating] = useState(false);
  const [version, setVersion] = useState("");
  const logEnd = useRef<HTMLDivElement>(null);

  const refresh = () => getStatus().then(setStatus);

  useEffect(() => {
    refresh();
    getAppVersion().then(setVersion);
    checkForUpdate().then(setUpdate);
    const unlisten = [
      onLog((e) => setLogs((l) => [...l.slice(-199), e])),
      onProfile(setProfile),
      // "Check for Updates…" from the native macOS menu.
      onMenuCheckUpdate(async () => {
        const u = await checkForUpdate();
        setUpdate(u);
        if (!u) await notify("You're up to date.");
      }),
    ];
    return () => { unlisten.forEach((p) => p.then((u) => u())); };
  }, []);

  const doUpdate = async () => {
    if (!update) return;
    setError(null); setUpdating(true);
    try { await installUpdate(update); }
    catch (e) { setError(String(e)); setUpdating(false); }
  };

  useEffect(() => { logEnd.current?.scrollIntoView({ behavior: "smooth" }); }, [logs]);

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

  return (
    <div className="app">
      <div className="topbar">
        <span className="spacer" />
        <span className={"dot " + (status?.running ? "on" : "off")} />
        <span className="muted">{status?.running ? "running" : "stopped"}</span>
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

      <div className="topgrid">
        <section className="card">
          <h2>Proxy</h2>
          <div className="row">
            <label>Port</label>
            <input className="port" type="number" value={port}
              onChange={(e) => setPort(Number(e.target.value))} disabled={status?.running} />
            <button className={status?.running ? "stop" : "start"} onClick={toggle}
              disabled={busy || !status?.key_loaded}>
              {status?.running ? "Stop" : "Start"}
            </button>
          </div>
        </section>

        <section className="card">
          <h2>Output folder</h2>
          <div className="row">
            <button onClick={changeFolder} disabled={status?.running}>Change…</button>
            <button onClick={useDefaultFolder} disabled={status?.running}>Default</button>
          </div>
          <code className="path" title={status?.out_dir}>{status?.out_dir ?? "…"}</code>
        </section>
      </div>

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
