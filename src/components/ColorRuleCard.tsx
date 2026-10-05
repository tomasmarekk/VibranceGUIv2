// Editor for one color-equalizer rule: the color to find, its replacement, and how
// matched pixels are blended. The eyedropper picks the source from the preview image.
import { TrashBin } from "@gravity-ui/icons";
import { Button, Card, ColorArea, ColorField, ColorPicker, ColorSlider, ColorSwatch, Switch, Tooltip } from "@heroui/react";
import { formatPercent } from "../format";
import type { ColorRule } from "../types";
import { EyedropperIcon } from "./EyedropperIcon";
import { ValueSlider } from "./ValueSlider";

interface ColorInputProps {
  label: string;
  value: string;
  onChange: (hex: string) => void;
  picking?: boolean;
  onPick?: () => void;
}

const signed = (value: number) => (value > 0 ? `+${Math.round(value)}` : `${Math.round(value)}`);

function ColorInput({ label, value, onChange, picking = false, onPick }: ColorInputProps) {
  const commit = (hex: string) => onChange(hex.toUpperCase());
  return <div className="color-input">
    <span className="color-input__label">{label}</span>
    <div className="color-input__row">
      <ColorPicker value={value} onChange={(color) => commit(color.toString("hex"))}>
        <ColorPicker.Trigger className="color-input__trigger" aria-label={`${label}: choose color`}>
          <ColorSwatch size="md" className="color-input__swatch" />
        </ColorPicker.Trigger>
        <ColorPicker.Popover className="color-input__popover">
          <ColorArea aria-label={`${label} saturation and brightness`} colorSpace="hsb" xChannel="saturation" yChannel="brightness" className="color-input__area">
            <ColorArea.Thumb />
          </ColorArea>
          <ColorSlider channel="hue" colorSpace="hsb" aria-label={`${label} hue`}>
            <ColorSlider.Track>
              <ColorSlider.Thumb />
            </ColorSlider.Track>
          </ColorSlider>
        </ColorPicker.Popover>
      </ColorPicker>
      <ColorField aria-label={`${label} hex`} value={value} onChange={(color) => { if (color) commit(color.toString("hex")); }} className="color-input__field">
        <ColorField.Group>
          <ColorField.Input />
        </ColorField.Group>
      </ColorField>
      {onPick && <Tooltip delay={300}>
        <Button isIconOnly size="sm" variant={picking ? "primary" : "ghost"} className="color-input__pick" aria-label={`${label}: pick from screenshot`} aria-pressed={picking} onPress={onPick}><EyedropperIcon aria-hidden="true" /></Button>
        <Tooltip.Content>{picking ? "Click the screenshot (Esc cancels)" : "Pick from screenshot"}</Tooltip.Content>
      </Tooltip>}
    </div>
  </div>;
}

interface ColorRuleCardProps {
  rule: ColorRule;
  index: number;
  picking: boolean;
  onChange: (rule: ColorRule) => void;
  onRemove: () => void;
  onPick: () => void;
}

/** Card for one rule; every change goes to the page draft and is saved with the profile. */
export function ColorRuleCard({ rule, index, picking, onChange, onRemove, onPick }: ColorRuleCardProps) {
  const update = (patch: Partial<ColorRule>) => onChange({ ...rule, ...patch });
  return <Card className={`rule ${rule.enabled ? "" : "is-off"}`}>
    <div className="rule__header">
      <Switch size="sm" isSelected={rule.enabled} onChange={(enabled) => update({ enabled })}>
        <Switch.Content>
          <Switch.Control><Switch.Thumb /></Switch.Control>
          <span className="sr-only">Color {index + 1} enabled</span>
        </Switch.Content>
      </Switch>
      <span className="rule__title">Color {index + 1}</span>
      <Tooltip delay={300}>
        <Button isIconOnly size="sm" variant="ghost" className="rule__remove" aria-label={`Remove color ${index + 1}`} onPress={onRemove}><TrashBin aria-hidden="true" /></Button>
        <Tooltip.Content>Remove color</Tooltip.Content>
      </Tooltip>
    </div>
    <div className="rule__colors">
      <ColorInput label="Find" value={rule.source} onChange={(source) => update({ source })} picking={picking} onPick={onPick} />
      <ColorInput label="Replace with" value={rule.target} onChange={(target) => update({ target })} />
    </div>
    <div className="rule__sliders">
      <ValueSlider label="Match range" value={rule.tolerance} min={0} max={100} format={(value) => `${Math.round(value)}`} onChange={(tolerance) => update({ tolerance })} />
      <ValueSlider label="Strength" value={rule.strength} min={0} max={100} format={formatPercent} onChange={(strength) => update({ strength })} />
      <ValueSlider label="Saturation" value={rule.saturation} min={-100} max={100} neutral={0} format={signed} onChange={(saturation) => update({ saturation })} />
      <ValueSlider label="Brightness" value={rule.brightness} min={-100} max={100} neutral={0} format={signed} onChange={(brightness) => update({ brightness })} />
    </div>
  </Card>;
}
