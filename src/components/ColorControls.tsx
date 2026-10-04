// Vibrance, brightness and gamma sliders shared by the Windows colors and the program editor.
// Values stay local while dragging; callers decide when a change is persisted.
import { Label, Slider } from "@heroui/react";
import type { CSSProperties } from "react";
import { formatGamma, formatPercent } from "../format";
import { DEFAULT_COLOR } from "../types";
import type { ColorSettings } from "../types";

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
    {controls.map((control) => {
      const neutral = ((DEFAULT_COLOR[control.key] - control.min) / (control.max - control.min)) * 100;
      return <Slider
        key={control.key}
        className="color-control"
        style={{ "--neutral-position": `${neutral}%` } as CSSProperties}
        value={value[control.key]}
        minValue={control.min}
        maxValue={control.max}
        step={control.step}
        isDisabled={control.key === "vibrance" ? !vibranceSupported : !gammaSupported}
        onChange={(next) => onChange({ ...value, [control.key]: Number(next) })}
        onChangeEnd={(next) => onCommit?.({ ...value, [control.key]: Number(next) })}
      >
        <Label>{control.label}</Label>
        <Slider.Output className="color-control__value">{control.format(value[control.key])}</Slider.Output>
        <Slider.Track className="color-control__track">
          <span className="color-control__neutral" aria-hidden="true" />
          <Slider.Fill />
          <Slider.Thumb />
        </Slider.Track>
      </Slider>;
    })}
  </div>;
}
