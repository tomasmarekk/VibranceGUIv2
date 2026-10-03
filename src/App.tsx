// Composes the familiar settings, Windows baseline, programs, and observer workflow.
// Persisted state remains authoritative; profile dialogs keep unsaved edits isolated.
import { Button, Card, Chip, Description, Label, Modal, Spinner, Switch } from "@heroui/react";
import { useEffect, useState } from "react";
import { api, isPreview } from "./api";
import { ColorControls } from "./components/ColorControls";
import { ProfileEditor } from "./components/ProfileEditor";
import { RunningAppPicker } from "./components/RunningAppPicker";
import { DEFAULT_COLOR } from "./types";
import type { AppState, ColorSettings, Profile, RunningApp, Settings } from "./types";

const preferences: { key: keyof Pick<Settings, "autostart" | "primaryOnly" | "neverChangeResolution">; label: string; description: string }[] = [
  { key: "autostart", label: "Start with Windows", description: "Ready when you sign in" },
  { key: "primaryOnly", label: "Primary monitor only", description: "Keep other displays unchanged" },
  { key: "neverChangeResolution", label: "Never change resolution", description: "Keep your desktop display mode" },
];

function readableError(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function DesktopColors({ state, busy, onSave }: { state: AppState; busy: boolean; onSave: (color: ColorSettings) => Promise<boolean> }) {
  const [draft, setDraft] = useState(state.desktop);
  const { vibrance, brightness, gamma } = state.desktop;
  useEffect(() => { setDraft({ vibrance, brightness, gamma }); }, [vibrance, brightness, gamma]);
  const save = async (color: ColorSettings) => {
    setDraft(color);
    if (!await onSave(color)) setDraft(state.desktop);
  };
  return <Card className="panel desktop-panel">
    <Card.Header className="panel-heading">
      <div className="section-heading"><span className="section-icon" aria-hidden="true">▣</span><div><Card.Title>Windows color</Card.Title><Card.Description>Your everyday baseline. Restored when you leave a program.</Card.Description></div></div>
      <Button variant="ghost" size="sm" onPress={() => void save({ ...DEFAULT_COLOR })} isDisabled={busy}>Reset to default</Button>
    </Card.Header>
    <Card.Content><ColorControls labelPrefix="Windows" value={draft} onChange={setDraft} onCommit={(color) => void save(color)} disabled={busy} vibranceSupported={state.status.supportsVibrance} gammaSupported={state.status.supportsGamma} /></Card.Content>
    {!state.status.supportsVibrance && <p className="hardware-notice">Digital vibrance is unavailable on the selected display driver.</p>}
    {!state.status.supportsGamma && <p className="hardware-notice">Brightness and gamma are unavailable on the selected display. If HDR is enabled, switch to SDR to use these controls.</p>}
  </Card>;
}

/** Desktop application shell with native persistence, observable status, and explicit browser preview. */
export default function App() {
  const [state, setState] = useState<AppState | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState("");
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [editor, setEditor] = useState<Profile | null>(null);
  const [pickerOpen, setPickerOpen] = useState(false);
  const [removeOpen, setRemoveOpen] = useState(false);
  const [revision, setRevision] = useState(0);

  useEffect(() => {
    let cancelled = false;
    let unsubscribe: (() => void) | undefined;
    setError(null);
    void (async () => {
      try {
        unsubscribe = await api.subscribe((next) => { if (!cancelled) setState(next); });
        if (cancelled) { unsubscribe(); return; }
        const initial = await api.getState();
        if (!cancelled) setState(initial);
      } catch (reason) { if (!cancelled) setError(readableError(reason)); }
    })();
    return () => { cancelled = true; unsubscribe?.(); };
  }, [revision]);

  useEffect(() => {
    if (!notice) return;
    const timeout = window.setTimeout(() => setNotice(""), 3200);
    return () => window.clearTimeout(timeout);
  }, [notice]);

  async function mutate(action: () => Promise<AppState>, success?: string): Promise<boolean> {
    setBusy(true);
    setError(null);
    try {
      setState(await action());
      if (success) setNotice(success);
      return true;
    } catch (reason) {
      setError(readableError(reason));
      return false;
    } finally { setBusy(false); }
  }

  function beginProfile(app: Pick<RunningApp, "name" | "executablePath">) {
    setPickerOpen(false);
    setError(null);
    const existing = state?.profiles.find((profile) => profile.executablePath.toLocaleLowerCase() === app.executablePath.toLocaleLowerCase());
    setEditor(existing ?? { id: crypto.randomUUID(), name: app.name, executablePath: app.executablePath, matchByPath: false, color: { ...DEFAULT_COLOR }, resolution: null });
  }

  async function addManually() {
    setBusy(true);
    setError(null);
    try { const app = await api.pickExecutable(); if (app) beginProfile(app); }
    catch (reason) { setError(readableError(reason)); }
    finally { setBusy(false); }
  }

  const selected = state?.profiles.find((profile) => profile.id === selectedId);
  const active = state?.profiles.find((profile) => profile.id === state.status.activeProfileId);

  return <div className="app-shell dark" data-theme="dark">
    <header className="app-header">
      <div className="brand"><img src="/brand/logo.png" className="brand-logo" alt="" /><div><h1 className="brand-name">Vibrance<span>GUI</span><span className="version-badge">v2</span></h1><p>A little more life on your screen.</p></div></div>
      <div className="header-context">{state && <span className="gpu-label" title={state.status.gpuName}>{state.status.gpuName}</span>}<Chip variant="soft" className="platform-badge"><span className="platform-dot" />Windows</Chip></div>
    </header>

    {isPreview && <div className="preview-banner"><strong>Browser preview</strong><span>Controls are interactive. Display settings are not changed.</span></div>}

    {!state ? <main className="initial-state">{error ? <><h1>Could not connect to the app</h1><p role="alert">{error}</p><Button onPress={() => setRevision(revision + 1)}>Try again</Button></> : <><Spinner /><p>Connecting to your displays…</p></>}</main> : <main className="main-content">
      {error && !editor && !removeOpen && <div className="error-banner" role="alert"><span>{error}</span><Button size="sm" variant="ghost" onPress={() => setError(null)} aria-label="Dismiss error">×</Button></div>}
      {state.status.message && <div className="hardware-notice" role="status">{state.status.message}</div>}

      <Card className="panel settings-panel">
        <Card.Header><Card.Title>Settings</Card.Title></Card.Header>
        <Card.Content className="settings-grid">
          {preferences.map((preference) => <Switch key={preference.key} aria-label={preference.label} className="preference-switch" size="sm" isSelected={state.settings[preference.key]} isDisabled={busy} onChange={(value) => void mutate(() => api.saveSettings({ ...state.settings, [preference.key]: value }))}>
            <Switch.Content><Switch.Control><Switch.Thumb /></Switch.Control><Label>{preference.label}</Label></Switch.Content><Description>{preference.description}</Description>
          </Switch>)}
        </Card.Content>
      </Card>

      <DesktopColors state={state} busy={busy} onSave={(color) => mutate(() => api.saveDesktop(color), "Windows color saved")} />

      <Card className="panel programs-panel">
        <Card.Header className="panel-heading"><div className="section-heading"><span className="section-icon" aria-hidden="true">▦</span><div><Card.Title>Program settings <span className="count-badge">{state.profiles.length}</span></Card.Title><Card.Description>Automatic color profiles for your games and apps.</Card.Description></div></div></Card.Header>
        <div className="program-toolbar"><div className="toolbar-actions"><Button size="sm" onPress={() => { setError(null); setPickerOpen(true); }} isDisabled={busy}><span aria-hidden="true">＋</span>Add running app</Button><Button variant="secondary" size="sm" onPress={() => void addManually()} isDisabled={busy}>Add manually</Button></div><div className="toolbar-actions"><Button variant="ghost" size="sm" isDisabled={!selected || busy} onPress={() => { if (selected) { setError(null); setEditor(selected); } }}>Edit</Button><Button variant="ghost" size="sm" className="remove-button" isDisabled={!selected || busy} onPress={() => { setError(null); setRemoveOpen(true); }}>Remove</Button></div></div>
        <Card.Content className="program-content">
          {state.profiles.length === 0 ? <div className="empty-state"><div className="empty-display" aria-hidden="true"><div className="color-spectrum"><i /><i /><i /><i /><i /><i /></div></div><h2>Great color. Right on cue.</h2><p>Add a game or app to give it its own look.<br />We’ll take care of switching back to your desktop.</p><Button variant="ghost" size="sm" onPress={() => setPickerOpen(true)}>Add your first program <span aria-hidden="true">→</span></Button></div> : <div className="profile-list" role="list" aria-label="Program profiles">
            {state.profiles.map((profile) => <div role="listitem" key={profile.id}><Button variant="ghost" className={`profile-row ${selectedId === profile.id ? "is-selected" : ""}`} aria-pressed={selectedId === profile.id} onPress={() => setSelectedId(profile.id)} onDoubleClick={() => { setError(null); setEditor(profile); }}>
              <span className="app-monogram" aria-hidden="true">{profile.name.slice(0, 1).toUpperCase()}</span><span className="profile-identity"><strong>{profile.name}</strong><span title={profile.executablePath}>{profile.executablePath.split(/[\\/]/).pop()}</span></span><span className="profile-values"><span><b>{profile.color.vibrance}%</b> vibrance</span><span><b>{profile.color.brightness}%</b> brightness</span><span><b>{profile.color.gamma.toFixed(2)}</b> gamma</span></span>{state.status.activeProfileId === profile.id ? <span className="active-badge">Active</span> : <span className="profile-chevron" aria-hidden="true">›</span>}
            </Button></div>)}
          </div>}
        </Card.Content>
        <Card.Footer className="program-hint"><span className="hint-dot" aria-hidden="true" /><span>{state.profiles.length ? "Select a program to edit or remove it. Double-click to open its settings." : "Profiles activate when a program is in the foreground."}</span></Card.Footer>
      </Card>
    </main>}

    <footer className="app-footer"><div className="observer-status"><span className={`status-dot ${state?.status.enabled ? "is-running" : ""}`} /><span>Observer <strong>{state ? state.status.enabled ? "running" : "paused" : "connecting"}</strong></span>{state && <span className="status-detail">{active ? active.name : state.status.enabled ? "Watching for your programs" : "Automatic profiles paused"}</span>}</div>{state && <Button variant="ghost" size="sm" isDisabled={busy} onPress={() => void mutate(() => api.setEnabled(!state.status.enabled))}>{state.status.enabled ? "Pause" : "Resume"}</Button>}</footer>

    <div className={`save-notice ${notice ? "is-visible" : ""}`} role="status" aria-live="polite">{notice && <><span aria-hidden="true">✓</span>{notice}</>}</div>
    {pickerOpen && <RunningAppPicker onPick={beginProfile} onClose={() => setPickerOpen(false)} />}
    {editor && state && <ProfileEditor key={editor.id} profile={editor} state={state} busy={busy} error={error} onClose={() => { setEditor(null); setError(null); }} onSave={(profile) => mutate(() => api.saveProfile(profile), "Profile saved")} />}
    {removeOpen && selected && <Modal.Backdrop isOpen onOpenChange={(open) => { if (!open && !busy) setRemoveOpen(false); }} isDismissable={!busy} isKeyboardDismissDisabled={busy} variant="blur"><Modal.Container size="sm" placement="center"><Modal.Dialog><Modal.Header><Modal.Heading>Remove this profile?</Modal.Heading></Modal.Header><Modal.Body><p>Remove <strong>{selected.name}</strong> from VibranceGUI? The program itself will stay on your computer.</p>{error && <div className="error-banner" role="alert">{error}</div>}</Modal.Body><Modal.Footer><Button variant="tertiary" onPress={() => setRemoveOpen(false)} isDisabled={busy}>Cancel</Button><Button variant="danger" isPending={busy} onPress={async () => { if (await mutate(() => api.removeProfile(selected.id), "Profile removed")) { setRemoveOpen(false); setSelectedId(null); } }}>Remove profile</Button></Modal.Footer></Modal.Dialog></Modal.Container></Modal.Backdrop>}
  </div>;
}
