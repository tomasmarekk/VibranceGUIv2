// Selects a visible running application using native process enumeration.
// Filtering happens locally and never executes or opens a listed process.
import { Button, Input, Label, Modal, Spinner, TextField } from "@heroui/react";
import { useEffect, useState } from "react";
import { api, isPreview } from "../api";
import type { RunningApp } from "../types";

interface RunningAppPickerProps {
  onPick: (app: RunningApp) => void;
  onClose: () => void;
}

/** Enumerates on mount or refresh; failed reads remain visible and retryable. */
export function RunningAppPicker({ onPick, onClose }: RunningAppPickerProps) {
  const [apps, setApps] = useState<RunningApp[]>([]);
  const [query, setQuery] = useState("");
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [revision, setRevision] = useState(0);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(null);
    void api.listRunningApps().then((items) => { if (!cancelled) setApps(items); }).catch((reason: unknown) => { if (!cancelled) setError(String(reason)); }).finally(() => { if (!cancelled) setLoading(false); });
    return () => { cancelled = true; };
  }, [revision]);

  const filtered = apps.filter((app) => `${app.name} ${app.executablePath}`.toLocaleLowerCase().includes(query.toLocaleLowerCase()));
  return <Modal.Backdrop isOpen onOpenChange={(open) => { if (!open) onClose(); }} variant="blur">
    <Modal.Container size="md" placement="center"><Modal.Dialog className="picker-dialog">
      <Modal.CloseTrigger />
      <Modal.Header><span className="eyebrow">ADD A PROGRAM</span><Modal.Heading>Choose a running app</Modal.Heading><p className="dialog-subtitle">Open your game first, then select it below.</p></Modal.Header>
      <Modal.Body>
        {isPreview && <p className="inline-notice">These are example programs for the browser preview.</p>}
        <TextField value={query} onChange={setQuery} className="search-field"><Label className="sr-only">Search running apps</Label><Input autoFocus placeholder="Search by name or executable…" /></TextField>
        <div className="running-app-list" aria-live="polite">
          {loading ? <div className="loading-state"><Spinner size="sm" /><span>Finding running apps…</span></div> : error ? <div className="error-banner" role="alert">{error}</div> : filtered.length === 0 ? <div className="picker-empty"><strong>{query ? "No matching apps" : "No apps found"}</strong><p>{query ? "Try a different name or executable." : "Open your program and refresh, or add its executable manually."}</p></div> : filtered.map((app) => <Button className="running-app-item" variant="ghost" key={`${app.executablePath}-${app.pid}`} onPress={() => onPick(app)}>
            <span className="app-monogram" aria-hidden="true">{app.name.slice(0, 1).toUpperCase()}</span><span className="running-app-text"><strong>{app.name}</strong><span>{app.executablePath}</span></span><span className="muted" aria-hidden="true">＋</span>
          </Button>)}
        </div>
      </Modal.Body>
      <Modal.Footer><Button variant="tertiary" onPress={() => setRevision(revision + 1)} isDisabled={loading}>Refresh list</Button><Button variant="secondary" onPress={onClose}>Cancel</Button></Modal.Footer>
    </Modal.Dialog></Modal.Container>
  </Modal.Backdrop>;
}
