// Main window, laid out like the original: settings, Windows colors, then the program library.
// Native state is authoritative; every change is saved through one ordered queue.
import { ArrowRotateLeft, Circle, CircleCheckFill, PauseFill, PlayFill, Plus } from "@gravity-ui/icons";
import { Alert, AlertDialog, Button, Card, Spinner, Toast, ToggleButton, toast } from "@heroui/react";
import { useCallback, useEffect, useRef, useState } from "react";
import { api } from "./api";
import { ColorControls } from "./components/ColorControls";
import { ProfileEditor } from "./components/ProfileEditor";
import { ProgramGrid } from "./components/ProgramGrid";
import { RunningAppPicker } from "./components/RunningAppPicker";
import { TitleBar } from "./components/TitleBar";
import { isNeutral, samePath } from "./format";
import { useExecutableIcons } from "./hooks/useExecutableIcons";
import { DEFAULT_COLOR } from "./types";
import type { AppState, ColorSettings, Profile, RunningApp, Settings } from "./types";

type Preference = keyof Pick<Settings, "autostart" | "primaryOnly" | "neverChangeResolution">;

const preferences: readonly { key: Preference; label: string }[] = [
  { key: "autostart", label: "Start with Windows" },
  { key: "primaryOnly", label: "Primary monitor only" },
  { key: "neverChangeResolution", label: "Never change resolution" },
];

function readableError(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

interface WindowsColorsProps {
  state: AppState;
  onSave: (color: ColorSettings) => Promise<boolean>;
}

function WindowsColors({ state, onSave }: WindowsColorsProps) {
  const [draft, setDraft] = useState(state.desktop);
  const { vibrance, brightness, gamma } = state.desktop;
  useEffect(() => { setDraft({ vibrance, brightness, gamma }); }, [vibrance, brightness, gamma]);

  async function commit(color: ColorSettings) {
    setDraft(color);
    if (!await onSave(color)) setDraft({ vibrance, brightness, gamma });
  }

  return <section className="section" aria-labelledby="windows-title">
    <div className="section__header">
      <h2 className="section__title" id="windows-title">Windows colors</h2>
      <Button size="sm" variant="ghost" className="section__link" isDisabled={isNeutral(draft)} onPress={() => void commit({ ...DEFAULT_COLOR })}><ArrowRotateLeft aria-hidden="true" />Reset</Button>
    </div>
    <Card className="colors-card">
      <ColorControls layout="row" value={draft} onChange={setDraft} onCommit={(color) => void commit(color)} vibranceSupported={state.status.supportsVibrance} gammaSupported={state.status.supportsGamma} />
      {!state.status.supportsVibrance && <p className="colors-card__note">Digital vibrance isn’t available on this display driver.</p>}
      {!state.status.supportsGamma && <p className="colors-card__note">Brightness and gamma need an SDR display. Turn off HDR to use them.</p>}
    </Card>
  </section>;
}

/** Desktop application shell with native persistence and an explicit browser preview. */
export default function App() {
  const [state, setState] = useState<AppState | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [revision, setRevision] = useState(0);
  const [pickerOpen, setPickerOpen] = useState(false);
  const [editor, setEditor] = useState<{ profile: Profile; isNew: boolean } | null>(null);
  // The target outlives the open flag so the dialog keeps its text while animating out.
  const [removeTarget, setRemoveTarget] = useState<Profile | null>(null);
  const [removeOpen, setRemoveOpen] = useState(false);
  const [removePending, setRemovePending] = useState(false);
  const stateRef = useRef<AppState | null>(null);
  const queue = useRef<Promise<unknown>>(Promise.resolve());
  const iconFor = useExecutableIcons([...(state?.profiles.map((profile) => profile.executablePath) ?? []), ...(editor ? [editor.profile.executablePath] : [])]);

  const accept = useCallback((next: AppState) => {
    stateRef.current = next;
    setState(next);
  }, []);

  useEffect(() => {
    let cancelled = false;
    let unsubscribe: (() => void) | undefined;
    setLoadError(null);
    void (async () => {
      try {
        unsubscribe = await api.subscribe((next) => { if (!cancelled) accept(next); });
        if (cancelled) { unsubscribe(); return; }
        const initial = await api.getState();
        if (!cancelled) accept(initial);
      } catch (reason) { if (!cancelled) setLoadError(readableError(reason)); }
    })();
    return () => { cancelled = true; unsubscribe?.(); };
  }, [revision, accept]);

  /**
   * Runs native mutations one at a time against the newest state, so a slow save can
   * never be overwritten by a request built from an older snapshot.
   */
  const run = useCallback((action: (current: AppState) => Promise<AppState>, failure: string): Promise<boolean> => {
    const result = queue.current.then(async () => {
      const current = stateRef.current;
      if (!current) return false;
      try {
        accept(await action(current));
        return true;
      } catch (reason) {
        toast.danger(failure, { description: readableError(reason) });
        return false;
      }
    });
    queue.current = result;
    return result;
  }, [accept]);

  function openProgram(app: Pick<RunningApp, "name" | "executablePath">) {
    setPickerOpen(false);
    const existing = stateRef.current?.profiles.find((profile) => samePath(profile.executablePath, app.executablePath));
    setEditor(existing
      ? { profile: existing, isNew: false }
      : { profile: { id: crypto.randomUUID(), name: app.name, executablePath: app.executablePath, matchByPath: false, color: { ...DEFAULT_COLOR }, resolution: null }, isNew: true });
  }

  async function addManually() {
    try {
      const app = await api.pickExecutable();
      if (app) openProgram(app);
    } catch (reason) {
      toast.danger("Couldn’t open the program", { description: readableError(reason) });
    }
  }

  async function confirmRemove(profile: Profile) {
    setRemovePending(true);
    const removed = await run(() => api.removeProfile(profile.id), "Couldn’t remove the program");
    setRemovePending(false);
    if (!removed) return;
    setRemoveOpen(false);
    setEditor(null);
    const id = toast(`${profile.name} removed`, {
      actionProps: {
        children: "Undo",
        variant: "tertiary",
        onPress: () => {
          toast.close(id);
          void run(() => api.saveProfile(profile), "Couldn’t restore the program");
        },
      },
    });
  }

  const enabled = state?.status.enabled ?? false;
  const activeProgram = state?.profiles.find((profile) => profile.id === state.status.activeProfileId);

  return <div className="app">
    <TitleBar />
    {!state ? <main className="startup">
      {loadError ? <div className="startup__error" role="alert">
        <h1>Couldn’t connect to VibranceGUI</h1>
        <p>{loadError}</p>
        <Button variant="primary" onPress={() => setRevision((value) => value + 1)}>Try again</Button>
      </div> : <Spinner aria-label="Loading" />}
    </main> : <>
      <main className="page">
        <div className="page__inner">
          {state.status.message && <Alert status="warning" className="page__alert"><Alert.Indicator /><Alert.Content><Alert.Description>{state.status.message}</Alert.Description></Alert.Content></Alert>}

          <section className="section" aria-labelledby="settings-title">
            <div className="section__header"><h2 className="section__title" id="settings-title">Settings</h2></div>
            <div className="preferences">
              {preferences.map((preference) => <ToggleButton key={preference.key} className="preference" isSelected={state.settings[preference.key]} onChange={(value) => void run((current) => api.saveSettings({ ...current.settings, [preference.key]: value }), "Couldn’t save settings")}>
                {({ isSelected }) => <>{isSelected ? <CircleCheckFill aria-hidden="true" /> : <Circle aria-hidden="true" />}{preference.label}</>}
              </ToggleButton>)}
            </div>
          </section>

          <WindowsColors state={state} onSave={(color) => run(() => api.saveDesktop(color), "Couldn’t save Windows colors")} />

          <section className="section section--programs" aria-labelledby="programs-title">
            <div className="section__header">
              <h2 className="section__title" id="programs-title">Programs{state.profiles.length > 0 && <span className="section__count">{state.profiles.length}</span>}</h2>
              <div className="section__actions">
                <Button variant="secondary" className="pill-button" onPress={() => void addManually()}>Add manually</Button>
                <Button variant="primary" className="pill-button" onPress={() => setPickerOpen(true)}><Plus aria-hidden="true" />Add</Button>
              </div>
            </div>
            {state.profiles.length > 0
              ? <ProgramGrid profiles={state.profiles} activeId={enabled ? state.status.activeProfileId : null} iconFor={iconFor} onOpen={(profile) => setEditor({ profile, isNew: false })} />
              : <div className="programs-empty">
                <p className="programs-empty__title">No programs yet</p>
                <p className="programs-empty__text">Add a game and its colors switch on whenever it’s in focus.</p>
              </div>}
          </section>
        </div>
      </main>

      <footer className="statusbar">
        <div className="statusbar__inner">
          <p className={`status ${enabled ? "is-running" : "is-paused"}`} role="status">
            <span className="status__dot" aria-hidden="true" />
            <span className="status__label">{enabled ? "Running" : "Paused"}</span>
            {enabled && activeProgram && <span className="status__detail">{activeProgram.name}</span>}
          </p>
          <Button size="sm" variant="tertiary" className="pill-button" onPress={() => void run((current) => api.setEnabled(!current.status.enabled), "Couldn’t change automatic profiles")}>
            {enabled ? <><PauseFill aria-hidden="true" />Pause</> : <><PlayFill aria-hidden="true" />Resume</>}
          </Button>
        </div>
      </footer>
    </>}

    {pickerOpen && <RunningAppPicker onPick={openProgram} onBrowse={() => { setPickerOpen(false); void addManually(); }} onClose={() => setPickerOpen(false)} />}
    {editor && state && <ProfileEditor
      key={editor.profile.id}
      profile={editor.profile}
      isNew={editor.isNew}
      state={state}
      icon={iconFor(editor.profile.executablePath)}
      onSave={(profile) => run(() => api.saveProfile(profile), editor.isNew ? "Couldn’t add the program" : "Couldn’t save the program")}
      onRemove={(profile) => { setRemoveTarget(profile); setRemoveOpen(true); }}
      onClose={() => setEditor(null)}
    />}
    <AlertDialog.Backdrop isOpen={removeOpen} onOpenChange={(open) => { if (!open && !removePending) setRemoveOpen(false); }}>
      <AlertDialog.Container size="sm">
        <AlertDialog.Dialog className="confirm">
          <AlertDialog.Header>
            <AlertDialog.Heading>Remove {removeTarget?.name}?</AlertDialog.Heading>
          </AlertDialog.Header>
          <AlertDialog.Body><p>Its color profile is deleted. The program itself stays installed.</p></AlertDialog.Body>
          <AlertDialog.Footer>
            <Button variant="tertiary" onPress={() => setRemoveOpen(false)} isDisabled={removePending}>Cancel</Button>
            <Button variant="danger" isPending={removePending} onPress={() => { if (removeTarget) void confirmRemove(removeTarget); }}>Remove</Button>
          </AlertDialog.Footer>
        </AlertDialog.Dialog>
      </AlertDialog.Container>
    </AlertDialog.Backdrop>
    <Toast.Provider placement="bottom" />
  </div>;
}
