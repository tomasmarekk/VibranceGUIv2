// Edits one isolated program profile and commits it only through Save.
// Closing the dialog discards the draft without modifying persisted settings.
import { Button, Checkbox, Description, Input, Label, ListBox, Modal, Select, TextField } from "@heroui/react";
import { useState } from "react";
import type { AppState, Profile, Resolution } from "../types";
import { DEFAULT_COLOR } from "../types";
import { ColorControls } from "./ColorControls";

interface ProfileEditorProps {
  profile: Profile;
  state: AppState;
  onSave: (profile: Profile) => Promise<boolean>;
  onClose: () => void;
  busy: boolean;
  error: string | null;
}

function resolutionKey(mode: Resolution): string {
  return `${mode.width}x${mode.height}@${mode.refreshRate}`;
}

/** Owns a disposable draft; errors preserve it so saving can be retried. */
export function ProfileEditor({ profile, state, onSave, onClose, busy, error }: ProfileEditorProps) {
  const [draft, setDraft] = useState<Profile>(() => structuredClone(profile));
  const resolutionDisabled = state.settings.neverChangeResolution;
  const modes = [...state.resolutions];
  if (draft.resolution && !modes.some((mode) => resolutionKey(mode) === resolutionKey(draft.resolution!))) modes.push(draft.resolution);

  return <Modal.Backdrop isOpen onOpenChange={(open) => { if (!open && !busy) onClose(); }} isDismissable={false} isKeyboardDismissDisabled={busy} variant="blur">
    <Modal.Container size="md" placement="center">
      <Modal.Dialog className="profile-dialog">
        <Modal.CloseTrigger isDisabled={busy} />
        <Modal.Header>
          <span className="eyebrow">PROGRAM PROFILE</span>
          <Modal.Heading>{state.profiles.some((item) => item.id === profile.id) ? "Edit profile" : "New profile"}</Modal.Heading>
          <p className="dialog-subtitle">Your settings, whenever this program is in focus.</p>
        </Modal.Header>
        <Modal.Body className="editor-body">
          <TextField value={draft.name} onChange={(name) => setDraft({ ...draft, name })} isRequired maxLength={120}>
            <Label>Program name</Label><Input placeholder="Name your program" />
          </TextField>
          <div className="executable-path" title={draft.executablePath}><span className="path-label">EXECUTABLE</span><span>{draft.executablePath}</span></div>
          <div className="editor-section-heading"><h3>In-game color</h3><Button variant="ghost" size="sm" isDisabled={busy} onPress={() => setDraft({ ...draft, color: { ...DEFAULT_COLOR } })}>Reset</Button></div>
          <ColorControls labelPrefix="Profile" value={draft.color} onChange={(color) => setDraft({ ...draft, color })} disabled={busy} vibranceSupported={state.status.supportsVibrance} gammaSupported={state.status.supportsGamma} layout="column" />
          <div className="resolution-section">
            <Checkbox isSelected={draft.resolution !== null} isDisabled={busy || resolutionDisabled || modes.length === 0} onChange={(selected) => setDraft({ ...draft, resolution: selected ? (modes[0] ?? null) : null })}>
              <Checkbox.Content><Checkbox.Control><Checkbox.Indicator /></Checkbox.Control>Change resolution in game</Checkbox.Content>
            </Checkbox>
            <p className="help-text">For windowed and borderless games. Your desktop resolution is restored when you switch away.</p>
            {resolutionDisabled && <p className="inline-notice">Resolution changes are turned off in your global settings.</p>}
            {draft.resolution && <Select
              aria-label="In-game resolution"
              className="resolution-select"
              value={resolutionKey(draft.resolution)}
              onChange={(key) => setDraft({ ...draft, resolution: modes.find((mode) => resolutionKey(mode) === String(key)) ?? null })}
              isDisabled={busy || resolutionDisabled}
            >
              <Select.Trigger><Select.Value /><Select.Indicator /></Select.Trigger>
              <Select.Popover><ListBox>{modes.map((mode) => <ListBox.Item key={resolutionKey(mode)} id={resolutionKey(mode)} textValue={`${mode.width} × ${mode.height} · ${mode.refreshRate} Hz`}><Label>{mode.width} × {mode.height} <span className="muted">· {mode.refreshRate} Hz</span></Label><ListBox.ItemIndicator /></ListBox.Item>)}</ListBox></Select.Popover>
            </Select>}
          </div>
          <Checkbox isSelected={draft.matchByPath} isDisabled={busy} onChange={(matchByPath) => setDraft({ ...draft, matchByPath })}>
            <Checkbox.Content><Checkbox.Control><Checkbox.Indicator /></Checkbox.Control>Match this exact executable path</Checkbox.Content>
            <Description>Leave off to match the executable name, even if the program moves.</Description>
          </Checkbox>
          {error && <div className="error-banner" role="alert">{error}</div>}
        </Modal.Body>
        <Modal.Footer>
          <Button variant="tertiary" onPress={onClose} isDisabled={busy}>Cancel</Button>
          <Button isPending={busy} isDisabled={!draft.name.trim()} onPress={async () => { if (await onSave({ ...draft, name: draft.name.trim() })) onClose(); }}>Save profile</Button>
        </Modal.Footer>
      </Modal.Dialog>
    </Modal.Container>
  </Modal.Backdrop>;
}
