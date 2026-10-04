// Built-in window header for the frameless native window: brand plus round window controls.
// The header drags the window; minimizing hides to the tray and closing exits (Rust shell).
import { Button, Chip } from "@heroui/react";
import { useEffect, useState } from "react";
import type { ReactNode } from "react";
import { appWindow, isPreview } from "../api";

function useMaximized(): boolean {
  const [maximized, setMaximized] = useState(false);
  useEffect(() => {
    if (!appWindow) return;
    const window = appWindow;
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    const refresh = () => { void window.isMaximized().then((value) => { if (!cancelled) setMaximized(value); }); };
    refresh();
    void window.onResized(refresh).then((stop) => { if (cancelled) stop(); else unlisten = stop; });
    return () => { cancelled = true; unlisten?.(); };
  }, []);
  return maximized;
}

// Hairline glyphs drawn for these buttons, matching the weight of the interface icons.
function Glyph({ children }: { children: ReactNode }) {
  return <svg className="window-control__glyph" viewBox="0 0 12 12" aria-hidden="true" fill="none" stroke="currentColor" strokeWidth="1.25" strokeLinecap="round" strokeLinejoin="round">{children}</svg>;
}

/** Brand row and window controls; everything except the controls drags the window. */
export function TitleBar() {
  const maximized = useMaximized();
  return <header className="app-header" data-tauri-drag-region="deep">
    <div className="app-header__inner">
      <img src="/brand/logo.png" alt="" className="app-header__logo" draggable={false} />
      <span className="app-header__name">VibranceGUI <span className="app-header__version">v2</span></span>
      {isPreview && <Chip size="sm" variant="soft" className="app-header__preview">Preview</Chip>}
    </div>
    {appWindow && <div className="window-controls">
      <Button isIconOnly variant="ghost" className="window-control" aria-label="Minimize" onPress={() => void appWindow?.minimize()}>
        <Glyph><path d="M2.5 6h7" /></Glyph>
      </Button>
      <Button isIconOnly variant="ghost" className="window-control" aria-label={maximized ? "Restore" : "Maximize"} onPress={() => void appWindow?.toggleMaximize()}>
        {maximized
          ? <Glyph><rect x="2.5" y="4" width="5.5" height="5.5" rx="1.25" /><path d="M4.5 2.5h3.75a1.25 1.25 0 0 1 1.25 1.25V7.5" /></Glyph>
          : <Glyph><rect x="2.5" y="2.5" width="7" height="7" rx="1.5" /></Glyph>}
      </Button>
      <Button isIconOnly variant="ghost" className="window-control window-control--close" aria-label="Close" onPress={() => void appWindow?.close()}>
        <Glyph><path d="M3 3l6 6M9 3L3 9" /></Glyph>
      </Button>
    </div>}
  </header>;
}
