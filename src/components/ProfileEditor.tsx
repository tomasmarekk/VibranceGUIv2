// Edits one program profile in isolation; nothing is stored until Save.
// Closing or cancelling discards the draft, so a new program is only added on Save.
import { TrashBin } from "@gravity-ui/icons";
import { Button, Description, Input, Label, ListBox, Modal, Select, Switch, TextField } from "@heroui/react";
import { useState } from "react";
import { formatResolution, resolutionKey } from "../format";
import type { AppState, Profile } from "../types";
import { AppIcon } from "./AppIcon";
import { ColorControls } from "./ColorControls";

const KEEP_RESOLUTION = "keep";

interface ProfileEditorProps {
  profile: Profile;
  /** True when the profile is not saved yet; Save then adds it. */
  isNew: boolean;
  state: AppState;
  icon: string | null | undefined;
  onSave: (profile: Profile) => Promise<boolean>;
  onRemove: (profile: Profile) => void;
  onClose: () => void;
}

/** Owns a disposable draft; a failed save keeps it open so the user can retry. */
export function ProfileEditor({ profile, isNew, state, icon, onSave, onRemove, onClose }: ProfileEditorProps) {
  const [draft, setDraft] = useState<Profile>(() => structuredClone(profile));
  const [saving, setSaving] = useState(false);
  const resolutionLocked = state.settings.neverChangeResolution;
  const modes = [...state.resolutions];
  if (draft.resolution && !modes.some((mode) => resolutionKey(mode) === resolutionKey(draft.resolution!))) modes.push(draft.resolution);
  const active = !isNew && state.status.enabled && state.status.activeProfileId === profile.id;

  async function save() {
    setSaving(true);
    const saved = await onSave({ ...draft, name: draft.name.trim() });
    setSaving(false);
    if (saved) onClose();
  }

  return <Modal.Backdrop isOpen onOpenChange={(open) => { if (!open && !saving) onClose(); }} isKeyboardDismissDisabled={saving} variant="blur">
    <Modal.Container size="md" placement="center">
      <Modal.Dialog className="editor" aria-label={isNew ? "New program" : `Edit ${profile.name}`}>
        <Modal.CloseTrigger isDisabled={saving} />
        <Modal.Header className="editor__header">
          <AppIcon name={draft.name || profile.name} src={icon} size="lg" />
          <div className="editor__identity">
            <TextField aria-label="Program name" className="editor__name" value={draft.name} onChange={(name) => setDraft({ ...draft, name })} maxLength={120} isRequired>
              <Input spellCheck={false} />
            </TextField>
            <p className="editor__path" title={draft.executablePath}>{active && <span className="live-pill"><span className="live-dot" aria-hidden="true" />Active</span>}<span>{draft.executablePath}</span></p>
          </div>
        </Modal.Header>
        <Modal.Body className="editor__body">
          <ColorControls layout="stack" value={draft.color} onChange={(color) => setDraft({ ...draft, color })} vibranceSupported={state.status.supportsVibrance} gammaSupported={state.status.supportsGamma} />
          <div className="editor__options">
            <div className="option-row">
              <div className="option-row__text">
                <Label id="resolution-label">Resolution</Label>
                {resolutionLocked && <Description>Turned off by “Never change resolution”</Description>}
              </div>
              <Select
                aria-labelledby="resolution-label"
                className="option-row__select"
                value={draft.resolution ? resolutionKey(draft.resolution) : KEEP_RESOLUTION}
                onChange={(key) => setDraft({ ...draft, resolution: modes.find((mode) => resolutionKey(mode) === String(key)) ?? null })}
                isDisabled={saving || resolutionLocked || modes.length === 0}
              >
                <Select.Trigger><Select.Value /><Select.Indicator /></Select.Trigger>
                <Select.Popover>
                  <ListBox>
                    <ListBox.Item id={KEEP_RESOLUTION} textValue="Don’t change">Don’t change<ListBox.ItemIndicator /></ListBox.Item>
                    {modes.map((mode) => <ListBox.Item key={resolutionKey(mode)} id={resolutionKey(mode)} textValue={formatResolution(mode)}>{formatResolution(mode)}<ListBox.ItemIndicator /></ListBox.Item>)}
                  </ListBox>
                </Select.Popover>
              </Select>
            </div>
            <Switch className="option-row option-switch" isSelected={draft.matchByPath} isDisabled={saving} onChange={(matchByPath) => setDraft({ ...draft, matchByPath })}>
              <Switch.Content>
                <Label>Match exact path</Label>
                <Description>Ignore copies of this program in other folders</Description>
              </Switch.Content>
              <Switch.Control><Switch.Thumb /></Switch.Control>
            </Switch>
          </div>
        </Modal.Body>
        <Modal.Footer className="editor__footer">
          {!isNew && <Button variant="ghost" className="editor__remove" onPress={() => onRemove(profile)} isDisabled={saving}><TrashBin aria-hidden="true" />Remove</Button>}
          <Button variant="tertiary" className="editor__cancel" onPress={onClose} isDisabled={saving}>Cancel</Button>
          <Button variant="primary" onPress={() => void save()} isPending={saving} isDisabled={!draft.name.trim()}>{isNew ? "Add program" : "Save"}</Button>
        </Modal.Footer>
      </Modal.Dialog>
    </Modal.Container>
  </Modal.Backdrop>;
}
