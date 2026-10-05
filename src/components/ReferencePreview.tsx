// Shows a reference screenshot with the program's settings applied, so colors can be
// tuned without switching to the game. Rendering uses the same math as the native
// gamma ramp and color overlay (see colorMatch.ts); vibrance is approximated.
import { ArrowUpFromSquare, Picture, TrashBin } from "@gravity-ui/icons";
import { Button, ToggleButton, ToggleButtonGroup, Tooltip, toast } from "@heroui/react";
import type { Key } from "@heroui/react";
import { useEffect, useRef, useState } from "react";
import type { ChangeEvent, CSSProperties, DragEvent, PointerEvent } from "react";
import { PREVIEW_FRAGMENT_SHADER, PREVIEW_VERTEX_SHADER, ruleParams, toHex, vibranceChroma } from "../colorMatch";
import type { BlackEqualizer, ColorRule, ColorSettings } from "../types";
import { EyedropperIcon } from "./EyedropperIcon";

/** Largest image accepted, matching the native store. */
const MAX_IMAGE_BYTES = 25 * 1024 * 1024;
const IMAGE_TYPES = ["image/png", "image/jpeg", "image/webp", "image/bmp"];

interface ReferencePreviewProps {
  image: string | null;
  onImage: (dataUrl: string) => void;
  onRemoveImage: () => void;
  color: ColorSettings;
  black: BlackEqualizer;
  rules: readonly ColorRule[];
  /** When true, a click on the image reports that pixel's color. */
  picking: boolean;
  onPick: (hex: string) => void;
  onCancelPick: () => void;
}

interface Renderer {
  draw: (settings: { original: boolean; color: ColorSettings; black: BlackEqualizer; rules: readonly ColorRule[] }) => void;
  dispose: () => void;
}

function createRenderer(canvas: HTMLCanvasElement, image: HTMLImageElement): Renderer | null {
  const gl = canvas.getContext("webgl2", { premultipliedAlpha: false, antialias: false });
  if (!gl) return null;
  const compile = (type: number, source: string) => {
    const shader = gl.createShader(type);
    if (!shader) return null;
    gl.shaderSource(shader, source);
    gl.compileShader(shader);
    return gl.getShaderParameter(shader, gl.COMPILE_STATUS) ? shader : null;
  };
  const vertex = compile(gl.VERTEX_SHADER, PREVIEW_VERTEX_SHADER);
  const fragment = compile(gl.FRAGMENT_SHADER, PREVIEW_FRAGMENT_SHADER);
  const program = gl.createProgram();
  if (!vertex || !fragment || !program) return null;
  gl.attachShader(program, vertex);
  gl.attachShader(program, fragment);
  gl.linkProgram(program);
  if (!gl.getProgramParameter(program, gl.LINK_STATUS)) return null;
  const vao = gl.createVertexArray();
  const texture = gl.createTexture();
  gl.bindTexture(gl.TEXTURE_2D, texture);
  // Pixel values must reach the shader untouched, exactly as the desktop capture does.
  gl.pixelStorei(gl.UNPACK_COLORSPACE_CONVERSION_WEBGL, gl.NONE);
  gl.pixelStorei(gl.UNPACK_PREMULTIPLY_ALPHA_WEBGL, false);
  gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA8, gl.RGBA, gl.UNSIGNED_BYTE, image);
  for (const parameter of [gl.TEXTURE_MIN_FILTER, gl.TEXTURE_MAG_FILTER]) gl.texParameteri(gl.TEXTURE_2D, parameter, gl.NEAREST);
  for (const parameter of [gl.TEXTURE_WRAP_S, gl.TEXTURE_WRAP_T]) gl.texParameteri(gl.TEXTURE_2D, parameter, gl.CLAMP_TO_EDGE);
  const uniform = (name: string) => gl.getUniformLocation(program, name);
  return {
    draw({ original, color, black, rules }) {
      const params = ruleParams(rules).slice(0, 4);
      gl.viewport(0, 0, canvas.width, canvas.height);
      gl.useProgram(program);
      gl.bindVertexArray(vao);
      gl.uniform1i(uniform("uImage"), 0);
      gl.uniform1i(uniform("uOriginal"), original ? 1 : 0);
      gl.uniform1i(uniform("uRuleCount"), params.length);
      if (params.length > 0) {
        gl.uniform3fv(uniform("uSource"), params.flatMap((rule) => rule.source));
        gl.uniform3fv(uniform("uTarget"), params.flatMap((rule) => rule.target));
        gl.uniform4fv(uniform("uParams"), params.flatMap((rule) => [rule.radius, rule.strength, rule.chroma, rule.lightness]));
      }
      gl.uniform1f(uniform("uVibrance"), vibranceChroma(color.vibrance));
      gl.uniform4f(uniform("uTone"), color.gamma, 2 ** ((color.brightness - 50) / 50), black.strength, black.range);
      gl.drawArrays(gl.TRIANGLES, 0, 3);
    },
    dispose() {
      gl.deleteTexture(texture);
      gl.deleteVertexArray(vao);
      gl.deleteProgram(program);
      gl.deleteShader(vertex);
      gl.deleteShader(fragment);
    },
  };
}

function readImage(file: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result));
    reader.onerror = () => reject(reader.error ?? new Error("The image could not be read."));
    reader.readAsDataURL(file);
  });
}

/** Before/after preview with paste, drop and file input, plus an eyedropper. */
export function ReferencePreview({ image, onImage, onRemoveImage, color, black, rules, picking, onPick, onCancelPick }: ReferencePreviewProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const rendererRef = useRef<Renderer | null>(null);
  const pixelsRef = useRef<CanvasRenderingContext2D | null>(null);
  const [size, setSize] = useState<{ width: number; height: number } | null>(null);
  const [mode, setMode] = useState<"after" | "before">("after");
  const [hovered, setHovered] = useState<string | null>(null);
  const [unsupported, setUnsupported] = useState(false);
  const [dragging, setDragging] = useState(false);
  const latest = useRef({ color, black, rules, mode });
  latest.current = { color, black, rules, mode };

  async function accept(file: Blob | null | undefined) {
    if (!file) return;
    if (!IMAGE_TYPES.includes(file.type)) { toast.danger("Use a PNG, JPEG, WebP or BMP image"); return; }
    if (file.size > MAX_IMAGE_BYTES) { toast.danger("The image is larger than 25 MB"); return; }
    try { onImage(await readImage(file)); }
    catch (reason) { toast.danger("Couldn’t read the image", { description: reason instanceof Error ? reason.message : String(reason) }); }
  }

  // Screenshots pasted anywhere on the page become the reference image.
  useEffect(() => {
    const onPaste = (event: ClipboardEvent) => {
      const item = [...(event.clipboardData?.items ?? [])].find((entry) => entry.type.startsWith("image/"));
      if (!item) return;
      event.preventDefault();
      void accept(item.getAsFile());
    };
    window.addEventListener("paste", onPaste);
    return () => window.removeEventListener("paste", onPaste);
  });

  useEffect(() => {
    if (!picking) { setHovered(null); return; }
    const onKey = (event: KeyboardEvent) => { if (event.key === "Escape") onCancelPick(); };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [picking, onCancelPick]);

  useEffect(() => {
    setSize(null);
    setUnsupported(false);
    if (!image) return;
    let cancelled = false;
    const element = new Image();
    element.onload = () => {
      const canvas = canvasRef.current;
      if (cancelled || !canvas) return;
      canvas.width = element.naturalWidth;
      canvas.height = element.naturalHeight;
      const pixels = document.createElement("canvas");
      pixels.width = element.naturalWidth;
      pixels.height = element.naturalHeight;
      const context = pixels.getContext("2d", { willReadFrequently: true, colorSpace: "srgb" });
      context?.drawImage(element, 0, 0);
      pixelsRef.current = context;
      rendererRef.current?.dispose();
      rendererRef.current = createRenderer(canvas, element);
      setUnsupported(!rendererRef.current);
      setSize({ width: element.naturalWidth, height: element.naturalHeight });
    };
    element.src = image;
    return () => {
      cancelled = true;
      rendererRef.current?.dispose();
      rendererRef.current = null;
      pixelsRef.current = null;
    };
  }, [image]);

  useEffect(() => {
    if (!size) return;
    const frame = requestAnimationFrame(() => {
      const { color: currentColor, black: currentBlack, rules: currentRules, mode: currentMode } = latest.current;
      rendererRef.current?.draw({ original: currentMode === "before", color: currentColor, black: currentBlack, rules: currentRules });
    });
    return () => cancelAnimationFrame(frame);
  }, [size, color, black, rules, mode]);

  function pixelAt(event: PointerEvent<HTMLCanvasElement>): string | null {
    const canvas = canvasRef.current;
    const context = pixelsRef.current;
    if (!canvas || !context) return null;
    const bounds = canvas.getBoundingClientRect();
    const x = Math.min(canvas.width - 1, Math.max(0, Math.floor(((event.clientX - bounds.left) / bounds.width) * canvas.width)));
    const y = Math.min(canvas.height - 1, Math.max(0, Math.floor(((event.clientY - bounds.top) / bounds.height) * canvas.height)));
    const [red, green, blue] = context.getImageData(x, y, 1, 1).data;
    return toHex([red, green, blue]);
  }

  const onDrop = (event: DragEvent<HTMLElement>) => {
    event.preventDefault();
    setDragging(false);
    void accept(event.dataTransfer.files[0]);
  };
  const dropProps = {
    onDragOver: (event: DragEvent<HTMLElement>) => { event.preventDefault(); setDragging(true); },
    onDragLeave: () => setDragging(false),
    onDrop,
  };

  return <div className={`preview ${dragging ? "is-dragging" : ""}`} {...dropProps}>
    <input ref={inputRef} type="file" accept={IMAGE_TYPES.join(",")} hidden onChange={(event: ChangeEvent<HTMLInputElement>) => { void accept(event.target.files?.[0]); event.target.value = ""; }} />
    {image ? <>
      <div className="preview__toolbar">
        <ToggleButtonGroup aria-label="Preview" selectionMode="single" disallowEmptySelection selectedKeys={[mode]} onSelectionChange={(keys: Set<Key>) => { const [key] = [...keys]; if (key === "before" || key === "after") setMode(key); }} className="preview__modes">
          <ToggleButton id="before" className="preview__mode">Before</ToggleButton>
          <ToggleButton id="after" className="preview__mode">After</ToggleButton>
        </ToggleButtonGroup>
        {picking
          ? <span className="preview__picking" role="status">{hovered && <i className="preview__swatch" style={{ "--swatch": hovered } as CSSProperties} aria-hidden="true" />}{hovered ?? "Click a pixel"}</span>
          : <div className="preview__actions">
            <Tooltip delay={300}><Button isIconOnly variant="ghost" size="sm" aria-label="Replace image" onPress={() => inputRef.current?.click()}><ArrowUpFromSquare aria-hidden="true" /></Button><Tooltip.Content>Replace image</Tooltip.Content></Tooltip>
            <Tooltip delay={300}><Button isIconOnly variant="ghost" size="sm" aria-label="Remove image" onPress={onRemoveImage}><TrashBin aria-hidden="true" /></Button><Tooltip.Content>Remove image</Tooltip.Content></Tooltip>
          </div>}
      </div>
      <div className="preview__frame" style={size ? ({ aspectRatio: `${size.width} / ${size.height}` } as CSSProperties) : undefined}>
        <canvas
          ref={canvasRef}
          className={`preview__canvas ${picking ? "is-picking" : ""}`}
          aria-label={picking ? "Pick a color from the screenshot" : "Screenshot preview"}
          role="img"
          onPointerMove={(event) => { if (picking) setHovered(pixelAt(event)); }}
          onPointerLeave={() => setHovered(null)}
          onPointerDown={(event) => { if (!picking) return; const hex = pixelAt(event); if (hex) onPick(hex); }}
        />
        {unsupported && <p className="preview__notice">This device can’t render the preview.</p>}
      </div>
    </> : <div className="preview__empty">
      <Picture className="preview__empty-icon" aria-hidden="true" />
      <p className="preview__empty-title">Add a screenshot from the game</p>
      <p className="preview__empty-text">Paste it with Ctrl+V or drop an image here.</p>
      <Button size="sm" variant="secondary" className="pill-button" onPress={() => inputRef.current?.click()}><ArrowUpFromSquare aria-hidden="true" />Choose image</Button>
      {picking && <p className="preview__empty-text"><EyedropperIcon aria-hidden="true" className="preview__inline-icon" />Add a screenshot to pick a color from it.</p>}
    </div>}
  </div>;
}
