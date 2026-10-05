// Keeps the preview math in step with the native implementation: the expected values
// below are the ones asserted by color_match.rs and platform.rs.
import { describe, expect, it } from "vitest";
import { blackLift, linearToSrgb, oklabToLinear, parseHex, recolor, rgbToOklab, ruleParams, toHex, toneCurve } from "./colorMatch";
import type { ColorRule } from "./types";

function rule(source: string, target: string, tolerance: number): ColorRule {
  return { id: "rule", enabled: true, source, target, tolerance, strength: 100, saturation: 0, brightness: 0 };
}

describe("color matching", () => {
  it("uses the same OKLab values as the native code", () => {
    const yellow = rgbToOklab([254, 254, 57]);
    expect(yellow[0]).toBeCloseTo(0.966449, 5);
    expect(yellow[1]).toBeCloseTo(-0.066179, 5);
    expect(yellow[2]).toBeCloseTo(0.185985, 5);
    const back = oklabToLinear(yellow).map((channel) => Math.round(linearToSrgb(channel) * 255));
    expect(back).toEqual([254, 254, 57]);
  });

  it("parses and formats hex colors", () => {
    expect(parseHex("#fefe39")).toEqual([254, 254, 57]);
    expect(parseHex("FEFE39")).toBeNull();
    expect(parseHex("#FEF")).toBeNull();
    expect(toHex([254, 254, 57])).toBe("#FEFE39");
  });

  it("recolors the outline and its shaded edge but leaves scenery alone", () => {
    const params = ruleParams([rule("#FEFE39", "#FF00FF", 30)]);
    const exact = recolor([254, 254, 57], params);
    expect(exact.opacity).toBeCloseTo(1, 9);
    expect(exact.color.map((channel) => Math.round(channel * 255))).toEqual([255, 0, 255]);
    expect(recolor([250, 248, 70], params).opacity).toBeGreaterThan(0.9);
    expect(recolor([150, 150, 30], params).opacity).toBeGreaterThan(0.5);
    for (const scenery of [[216, 196, 160], [224, 128, 64], [255, 255, 255], [20, 20, 22]] as const) {
      expect(recolor([...scenery], params).opacity).toBe(0);
    }
  });

  it("skips disabled, invisible and malformed rules", () => {
    expect(ruleParams([{ ...rule("#FEFE39", "#FF00FF", 30), enabled: false }, { ...rule("#FEFE39", "#FF00FF", 30), strength: 0 }, rule("yellow", "#FF00FF", 30)])).toEqual([]);
  });
});

describe("tone curve", () => {
  it("is the identity at neutral settings", () => {
    for (const value of [0, 0.1, 0.5, 1]) {
      expect(toneCurve(value, { vibrance: 50, brightness: 50, gamma: 1 }, { strength: 0, range: 50 })).toBeCloseTo(value, 12);
    }
  });

  it("lifts shadows monotonically and keeps black and highlights", () => {
    expect(blackLift(0, 100, 50)).toBe(0);
    expect(blackLift(0.8, 100, 50)).toBe(0.8);
    expect(blackLift(30 / 255, 80, 50)).toBeGreaterThan(30 / 255 + 3000 / 65535);
    let previous = -1;
    for (let step = 0; step <= 1000; step++) {
      const lifted = blackLift(step / 1000, 100, 50);
      expect(lifted).toBeGreaterThanOrEqual(previous);
      previous = lifted;
    }
  });
});
