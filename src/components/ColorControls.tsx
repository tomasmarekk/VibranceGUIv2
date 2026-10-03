// Accessible HeroUI sliders shared by the Windows baseline and program editor.
// Changes stay local until the caller chooses a commit boundary.
import { Label, Slider } from "@heroui/react";
import type { ColorSettings } from "../types";

interface ColorControlsProps {
  value: ColorSettings;
  onChange: (value: ColorSettings) => void;
  onCommit?: (value: ColorSettings) => void;
  disabled?: boolean;
  vibranceSupported?: boolean;
  gammaSupported?: boolean;
  layout?: "row" | "column";
  labelPrefix: string;
}

const controls = [
  { key: "vibrance", label: "Digital vibrance", min: 0, max: 100, step: 1, symbol: "◉", low: "Muted", high: "Vivid" },
  { key: "brightness", label: "Brightness", min: 0, max: 100, step: 1, symbol: "☼", low: "Darker", high: "Brighter" },
  { key: "gamma", label: "Gamma", min: 0.5, max: 3, step: 0.05, symbol: "γ", low: "0.50", high: "3.00" },
] as const;

/** Renders bounded color sliders; onCommit fires on completed pointer or keyboard changes. */
export function ColorControls({ value, onChange, onCommit, disabled = false, vibranceSupported = true, gammaSupported = true, layout = "row", labelPrefix }: ColorControlsProps) {
  return <div className={`color-controls color-controls--${layout}`}>
    {controls.map((control) => <Slider
      key={control.key}
      className={`color-control color-control--${control.key}`}
      value={value[control.key]}
      minValue={control.min}
      maxValue={control.max}
      step={control.step}
      isDisabled={disabled || (control.key === "vibrance" ? !vibranceSupported : !gammaSupported)}
      onChange={(next) => onChange({ ...value, [control.key]: Number(next) })}
      onChangeEnd={(next) => onCommit?.({ ...value, [control.key]: Number(next) })}
    >
      <div className="slider-heading">
        <Label><span className="control-symbol" aria-hidden="true">{control.symbol}</span><span aria-hidden="true">{control.label}</span><span className="sr-only">{`${labelPrefix} ${control.label}`}</span></Label>
        <Slider.Output>{control.key === "gamma" ? value.gamma.toFixed(2) : `${value[control.key]}%`}</Slider.Output>
      </div>
      <Slider.Track><Slider.Fill /><Slider.Thumb /></Slider.Track>
      <div className="slider-range" aria-hidden="true"><span>{control.low}</span><span>{control.high}</span></div>
    </Slider>)}
  </div>;
}
