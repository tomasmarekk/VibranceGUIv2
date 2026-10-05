// Full-window editor for one program profile: color, black equalizer, color equalizer and
// display options beside a live screenshot preview. Profile changes stay in a draft until
// Save; the reference screenshot is stored right away because it is only a preview aid.
import { ArrowLeft, Plus, TrashBin } from "@gravity-ui/icons";
import { Button, Card, Description, Input, Label, ListBox, Select, Switch, TextField, toast } from "@heroui/react";
import { useCallback, useEffect, useState } from "react";
import { api } from "../api";
import { formatPercent, formatResolution, resolutionKey } from "../format";
import { DEFAULT_BLACK_EQUALIZER, MAX_COLOR_RULES } from "../types";
import type { AppState, ColorRule, Profile } from "../types";
import { AppIcon } from "./AppIcon";
import { ColorControls } from "./ColorControls";
import { ColorRuleCard } from "./ColorRuleCard";
import { ReferencePreview } from "./ReferencePreview";
import { ValueSlider } from "./ValueSlider";

const KEEP_RESOLUTION = "keep";

function readableError(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function newRule(): ColorRule {
  return { id: crypto.randomUUID(), enabled: true, source: "#FFFF00", target: "#FF3BD4", tolerance: 30, strength: 100, saturation: 0, brightness: 0 };
}

interface ProgramPageProps {
  profile: Profile;
  /** True when the profile is not saved yet; Save then adds it. */
  isNew: boolean;
  state: AppState;
  icon: string | null | undefined;
  onSave: (profile: Profile) => Promise<boolean>;
  onRemove: (profile: Profile) => void;
  onClose: () => void;
}

/** Owns a disposable draft; a failed save keeps the page open so the user can retry. */
export function ProgramPage({ profile, isNew, state, icon, onSave, onRemove, onClose }: ProgramPageProps) {
  const [draft, setDraft] = useState<Profile>(() => ({ ...structuredClone(profile), blackEqualizer: profile.blackEqualizer ?? { ...DEFAULT_BLACK_EQUALIZER }, colorRules: profile.colorRules ?? [] }));
  const [saving, setSaving] = useState(false);
  const [image, setImage] = useState<string | null>(null);
  const [pickingRule, setPickingRule] = useState<string | null>(null);
  const resolutionLocked = state.settings.neverChangeResolution;
  const modes = [...state.resolutions];
  if (draft.resolution && !modes.some((mode) => resolutionKey(mode) === resolutionKey(draft.resolution!))) modes.push(draft.resolution);
  const active = !isNew && state.status.enabled && state.status.activeProfileId === profile.id;

  useEffect(() => {
    let cancelled = false;
    void api.referenceImage(profile.id).then((stored) => { if (!cancelled) setImage(stored); }).catch(() => {});
    return () => { cancelled = true; };
  }, [profile.id]);

  const cancelPick = useCallback(() => setPickingRule(null), []);

  function changeImage(dataUrl: string | null) {
    setImage(dataUrl);
    const stored = dataUrl ? api.saveReferenceImage(profile.id, dataUrl) : api.deleteReferenceImage(profile.id);
    stored.catch((reason: unknown) => toast.danger("Couldn’t store the screenshot", { description: readableError(reason) }));
  }

  function updateRule(rule: ColorRule) {
    setDraft((current) => ({ ...current, colorRules: current.colorRules.map((item) => (item.id === rule.id ? rule : item)) }));
  }

  async function save() {
    setSaving(true);
    const saved = await onSave({ ...draft, name: draft.name.trim() });
    setSaving(false);
    if (saved) onClose();
  }

  return <section className="program" aria-label={isNew ? "New program" : `Edit ${profile.name}`}>
    <header className="program__header">
      <Button isIconOnly variant="ghost" className="program__back" aria-label="Back" onPress={onClose} isDisabled={saving}><ArrowLeft aria-hidden="true" /></Button>
      <AppIcon name={draft.name || profile.name} src={icon} size="lg" />
      <div className="program__identity">
        <TextField aria-label="Program name" className="program__name" value={draft.name} onChange={(name) => setDraft({ ...draft, name })} maxLength={120} isRequired>
          <Input spellCheck={false} />
        </TextField>
        <p className="program__path" title={draft.executablePath}>{active && <span className="live-pill"><span className="live-dot" aria-hidden="true" />Active</span>}<span>{draft.executablePath}</span></p>
      </div>
      {!isNew && <Button variant="ghost" className="program__remove" onPress={() => onRemove(profile)} isDisabled={saving}><TrashBin aria-hidden="true" />Remove</Button>}
    </header>

    <div className="program__body">
      <div className="program__layout">
        <div className="program__controls">
          <section className="section" aria-labelledby="program-color">
            <h2 className="section__title" id="program-color">Color</h2>
            <Card className="colors-card">
              <ColorControls layout="stack" value={draft.color} onChange={(color) => setDraft({ ...draft, color })} vibranceSupported={state.status.supportsVibrance} gammaSupported={state.status.supportsGamma} />
            </Card>
          </section>

          <section className="section" aria-labelledby="program-black">
            <div className="section__heading">
              <h2 className="section__title" id="program-black">Black Equalizer</h2>
              <p className="section__hint">Brightens dark areas and leaves bright ones as they are.</p>
            </div>
            <Card className="colors-card">
              <div className="color-controls color-controls--stack">
                <ValueSlider label="Strength" value={draft.blackEqualizer.strength} min={0} max={100} format={formatPercent} isDisabled={!state.status.supportsGamma} onChange={(strength) => setDraft({ ...draft, blackEqualizer: { ...draft.blackEqualizer, strength } })} />
                <ValueSlider label="Range" value={draft.blackEqualizer.range} min={0} max={100} neutral={DEFAULT_BLACK_EQUALIZER.range} format={formatPercent} isDisabled={!state.status.supportsGamma} onChange={(range) => setDraft({ ...draft, blackEqualizer: { ...draft.blackEqualizer, range } })} />
              </div>
            </Card>
          </section>

          <section className="section" aria-labelledby="program-match">
            <div className="section__header">
              <div className="section__heading">
                <h2 className="section__title" id="program-match">Color Equalizer</h2>
                <p className="section__hint">Recolors one color, like enemy outlines. Set the game to windowed fullscreen (borderless); nothing can draw over exclusive fullscreen.</p>
              </div>
              <Button size="sm" variant="secondary" className="pill-button" isDisabled={draft.colorRules.length >= MAX_COLOR_RULES} onPress={() => setDraft({ ...draft, colorRules: [...draft.colorRules, newRule()] })}><Plus aria-hidden="true" />Add color</Button>
            </div>
            {draft.colorRules.map((rule, index) => <ColorRuleCard
              key={rule.id}
              rule={rule}
              index={index}
              picking={pickingRule === rule.id}
              onChange={updateRule}
              onRemove={() => { setDraft({ ...draft, colorRules: draft.colorRules.filter((item) => item.id !== rule.id) }); if (pickingRule === rule.id) setPickingRule(null); }}
              onPick={() => setPickingRule(pickingRule === rule.id ? null : rule.id)}
            />)}
          </section>

          <section className="section" aria-labelledby="program-display">
            <h2 className="section__title" id="program-display">Display</h2>
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
                <Switch.Content className="option-switch__button">
                  <Label>Match exact path</Label>
                  <Switch.Control><Switch.Thumb /></Switch.Control>
                </Switch.Content>
                <Description>Ignore copies of this program in other folders</Description>
              </Switch>
            </div>
          </section>
        </div>

        <aside className="program__preview" aria-label="Preview">
          <ReferencePreview
            image={image}
            onImage={(dataUrl) => changeImage(dataUrl)}
            onRemoveImage={() => changeImage(null)}
            color={draft.color}
            black={draft.blackEqualizer}
            rules={draft.colorRules}
            picking={pickingRule !== null}
            onPick={(hex) => {
              const rule = draft.colorRules.find((item) => item.id === pickingRule);
              if (rule) updateRule({ ...rule, source: hex });
              setPickingRule(null);
            }}
            onCancelPick={cancelPick}
          />
        </aside>
      </div>
    </div>

    <footer className="program__footer">
      <Button variant="tertiary" className="pill-button" onPress={onClose} isDisabled={saving}>Cancel</Button>
      <Button variant="primary" className="pill-button" onPress={() => void save()} isPending={saving} isDisabled={!draft.name.trim()}>{isNew ? "Add program" : "Save"}</Button>
    </footer>
  </section>;
}
