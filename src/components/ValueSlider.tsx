// Labelled HeroUI slider in the app's pill style, with a faint tick at the neutral value.
// Used for color, black-equalizer and color-rule controls alike.
import { Label, Slider } from "@heroui/react";
import type { CSSProperties } from "react";

interface ValueSliderProps {
  label: string;
  value: number;
  min: number;
  max: number;
  step?: number;
  /** Value that leaves the picture unchanged, marked on the track when it lies inside it. */
  neutral?: number;
  format: (value: number) => string;
  onChange: (value: number) => void;
  /** Fires once a drag or key press finishes. */
  onChangeEnd?: (value: number) => void;
  isDisabled?: boolean;
}

/** One control row: label and value above a pill track. */
export function ValueSlider({ label, value, min, max, step = 1, neutral, format, onChange, onChangeEnd, isDisabled = false }: ValueSliderProps) {
  const marked = neutral !== undefined && neutral > min && neutral < max;
  const neutralPosition = marked ? ((neutral - min) / (max - min)) * 100 : 0;
  return <Slider
    className="color-control"
    style={{ "--neutral-position": `${neutralPosition}%` } as CSSProperties}
    value={value}
    minValue={min}
    maxValue={max}
    step={step}
    isDisabled={isDisabled}
    onChange={(next) => onChange(Number(next))}
    onChangeEnd={(next) => onChangeEnd?.(Number(next))}
  >
    <Label>{label}</Label>
    <Slider.Output className="color-control__value">{format(value)}</Slider.Output>
    <Slider.Track className="color-control__track">
      {marked && <span className="color-control__neutral" aria-hidden="true" />}
      <Slider.Fill />
      <Slider.Thumb />
    </Slider.Track>
  </Slider>;
}
