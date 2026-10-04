// Chooses a program from the visible windows reported by native process enumeration.
// Filtering happens locally; picking never starts, focuses or modifies the process.
import { ArrowsRotateRight, FolderOpen } from "@gravity-ui/icons";
import { Alert, Button, Description, Label, ListBox, Modal, SearchField, Spinner } from "@heroui/react";
import { useEffect, useState } from "react";
import { api, isPreview } from "../api";
import { fileName } from "../format";
import { useExecutableIcons } from "../hooks/useExecutableIcons";
import type { RunningApp } from "../types";
import { AppIcon } from "./AppIcon";

interface RunningAppPickerProps {
  onPick: (app: RunningApp) => void;
  onBrowse: () => void;
  onClose: () => void;
}

/** Enumerates on open and on refresh; failed reads stay visible and retryable. */
export function RunningAppPicker({ onPick, onBrowse, onClose }: RunningAppPickerProps) {
  const [apps, setApps] = useState<RunningApp[]>([]);
  const [query, setQuery] = useState("");
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [revision, setRevision] = useState(0);
  const iconFor = useExecutableIcons(apps.map((app) => app.executablePath));

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(null);
    void api.listRunningApps()
      .then((items) => { if (!cancelled) setApps(items); })
      .catch((reason: unknown) => { if (!cancelled) setError(reason instanceof Error ? reason.message : String(reason)); })
      .finally(() => { if (!cancelled) setLoading(false); });
    return () => { cancelled = true; };
  }, [revision]);

  const needle = query.trim().toLocaleLowerCase();
  const filtered = apps.filter((app) => `${app.name} ${app.executablePath}`.toLocaleLowerCase().includes(needle));

  return <Modal.Backdrop isOpen onOpenChange={(open) => { if (!open) onClose(); }} variant="blur">
    <Modal.Container size="md" placement="center">
      <Modal.Dialog className="picker">
        <Modal.CloseTrigger />
        <Modal.Header><Modal.Heading>Add a running app</Modal.Heading></Modal.Header>
        <Modal.Body className="picker__body">
          <SearchField aria-label="Search running apps" value={query} onChange={setQuery} className="picker__search" autoFocus>
            <SearchField.Group>
              <SearchField.SearchIcon />
              <SearchField.Input placeholder="Search" />
              <SearchField.ClearButton />
            </SearchField.Group>
          </SearchField>
          <div className="picker__list" aria-busy={loading}>
            {loading ? <div className="picker__state"><Spinner size="sm" /></div>
              : error ? <Alert status="danger"><Alert.Indicator /><Alert.Content><Alert.Title>Couldn’t list running apps</Alert.Title><Alert.Description>{error}</Alert.Description></Alert.Content></Alert>
              : filtered.length === 0 ? <div className="picker__state">
                <p className="picker__state-title">{needle ? "No matching apps" : "No open apps found"}</p>
                <p className="picker__state-text">{needle ? "Try another name." : "Start your game, then refresh."}</p>
              </div>
              : <ListBox aria-label="Running apps" selectionMode="none" className="picker__options" onAction={(key) => {
                const app = filtered.find((item) => item.executablePath === String(key));
                if (app) onPick(app);
              }}>
                {filtered.map((app) => <ListBox.Item key={app.executablePath} id={app.executablePath} textValue={app.name} className="picker__option">
                  <AppIcon name={app.name} src={iconFor(app.executablePath)} size="md" />
                  <div className="picker__option-text"><Label>{app.name}</Label><Description>{fileName(app.executablePath)}</Description></div>
                </ListBox.Item>)}
              </ListBox>}
          </div>
          {isPreview && <p className="picker__preview-note">Example apps — the browser preview can’t see your programs.</p>}
        </Modal.Body>
        <Modal.Footer className="picker__footer">
          <Button variant="ghost" onPress={() => setRevision((value) => value + 1)} isDisabled={loading}><ArrowsRotateRight aria-hidden="true" />Refresh</Button>
          <Button variant="secondary" onPress={onBrowse}><FolderOpen aria-hidden="true" />Browse for .exe…</Button>
        </Modal.Footer>
      </Modal.Dialog>
    </Modal.Container>
  </Modal.Backdrop>;
}
