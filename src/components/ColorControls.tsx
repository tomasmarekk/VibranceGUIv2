// Vibrance, brightness and gamma sliders shared by the Windows colors and the program page.
// Values stay local while dragging; callers decide when a change is persisted.
import { formatGamma, formatPercent } from "../format";
import { DEFAULT_COLOR } from "../types";
import type { ColorSettings } from "../types";
import { ValueSlider } from "./ValueSlider";

interface ColorControlsProps {
  value: ColorSettings;
  onChange: (value: ColorSettings) => void;
  /** Fires when a drag or key press finishes, with the final values. */
  onCommit?: (value: ColorSettings) => void;
  vibranceSupported: boolean;
  gammaSupported: boolean;
  /** "row" places the three sliders side by side; "stack" lists them vertically. */
  layout: "row" | "stack";
}

const controls = [
  { key: "vibrance", label: "Digital vibrance", min: 0, max: 100, step: 1, format: formatPercent },
  { key: "brightness", label: "Brightness", min: 0, max: 100, step: 1, format: formatPercent },
  { key: "gamma", label: "Gamma", min: 0.5, max: 3, step: 0.05, format: formatGamma },
] as const;

/** Renders the three color sliders; a faint tick marks each control's neutral value. */
export function ColorControls({ value, onChange, onCommit, vibranceSupported, gammaSupported, layout }: ColorControlsProps) {
  return <div className={`color-controls color-controls--${layout}`}>
    {controls.map((control) => <ValueSlider
      key={control.key}
      label={control.label}
      value={value[control.key]}
      min={control.min}
      max={control.max}
      step={control.step}
      neutral={DEFAULT_COLOR[control.key]}
      format={control.format}
      isDisabled={control.key === "vibrance" ? !vibranceSupported : !gammaSupported}
      onChange={(next) => onChange({ ...value, [control.key]: next })}
      onChangeEnd={(next) => onCommit?.({ ...value, [control.key]: next })}
    />)}
  </div>;
}
