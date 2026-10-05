// Behavioral coverage for persistence, profile drafts, filtering, removal and recoverable errors.
// Native hardware is replaced at the IPC boundary rather than mocking HeroUI controls.
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import { api } from "./api";
import type { AppState, Profile } from "./types";

vi.mock("./api", () => ({ isPreview: false, appWindow: null, api: { getState: vi.fn(), subscribe: vi.fn(), saveSettings: vi.fn(), saveDesktop: vi.fn(), saveProfile: vi.fn(), removeProfile: vi.fn(), setEnabled: vi.fn(), listRunningApps: vi.fn(), pickExecutable: vi.fn(), executableIcons: vi.fn(), referenceImage: vi.fn(), saveReferenceImage: vi.fn(), deleteReferenceImage: vi.fn() } }));

const profile: Profile = { id: "game", name: "My game", executablePath: "C:\\Games\\game.exe", matchByPath: false, color: { vibrance: 75, brightness: 50, gamma: 1 }, resolution: null, blackEqualizer: { strength: 0, range: 50 }, colorRules: [] };
let state: AppState;

beforeEach(() => {
  vi.clearAllMocks();
  state = { settings: { autostart: false, primaryOnly: false, neverChangeResolution: false }, desktop: { vibrance: 50, brightness: 50, gamma: 1 }, profiles: [], status: { enabled: true, activeProfileId: null, gpuName: "Test display", supportsVibrance: true, supportsGamma: true, message: null }, resolutions: [{ width: 1920, height: 1080, refreshRate: 144 }] };
  vi.mocked(api.getState).mockImplementation(async () => structuredClone(state));
  vi.mocked(api.subscribe).mockResolvedValue(() => {});
  vi.mocked(api.saveSettings).mockImplementation(async (settings) => ({ ...state, settings }));
  vi.mocked(api.saveDesktop).mockImplementation(async (desktop) => ({ ...state, desktop }));
  vi.mocked(api.saveProfile).mockImplementation(async (item) => ({ ...state, profiles: [item] }));
  vi.mocked(api.removeProfile).mockImplementation(async () => ({ ...state, profiles: [] }));
  vi.mocked(api.setEnabled).mockImplementation(async (enabled) => ({ ...state, status: { ...state.status, enabled } }));
  vi.mocked(api.pickExecutable).mockResolvedValue({ name: "My game", executablePath: "C:\\Games\\game.exe" });
  vi.mocked(api.executableIcons).mockImplementation(async (paths) => paths.map(() => null));
  vi.mocked(api.referenceImage).mockResolvedValue(null);
  vi.mocked(api.saveReferenceImage).mockResolvedValue();
  vi.mocked(api.deleteReferenceImage).mockResolvedValue();
  vi.mocked(api.listRunningApps).mockResolvedValue([{ name: "Game window", executablePath: "C:\\Games\\game.exe", pid: 7 }, { name: "Text editor", executablePath: "C:\\Editor\\edit.exe", pid: 8 }]);
});

async function renderApp() {
  const user = userEvent.setup();
  render(<App />);
  await screen.findByRole("heading", { name: "Settings" });
  return user;
}

describe("program and display workflow", () => {
  it("starts empty and persists one setting without changing the others", async () => {
    const user = await renderApp();
    expect(screen.getByText("No programs yet")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Primary monitor only" }));
    await waitFor(() => expect(api.saveSettings).toHaveBeenCalledWith({ autostart: false, primaryOnly: true, neverChangeResolution: false }));
    expect(screen.getByRole("button", { name: "Primary monitor only" })).toHaveAttribute("aria-pressed", "true");
  });

  it("discards an unsaved manually added profile", async () => {
    const user = await renderApp();
    await user.click(screen.getByRole("button", { name: "Add manually" }));
    const dialog = await screen.findByRole("region", { name: "New program" });
    await user.clear(within(dialog).getByRole("textbox", { name: "Program name" }));
    await user.type(within(dialog).getByRole("textbox", { name: "Program name" }), "Discarded draft");
    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(api.saveProfile).not.toHaveBeenCalled();
    expect(screen.getByText("No programs yet")).toBeInTheDocument();
  });

  it("saves edited color values and an advertised resolution", async () => {
    const user = await renderApp();
    await user.click(screen.getByRole("button", { name: "Add manually" }));
    const dialog = await screen.findByRole("region", { name: "New program" });
    within(dialog).getByRole("slider", { name: "Gamma" }).focus();
    await user.keyboard("{ArrowRight}{ArrowRight}");
    await user.click(within(dialog).getByRole("button", { name: /Resolution/ }));
    await user.click(await screen.findByRole("option", { name: "1920 × 1080 · 144 Hz" }));
    await user.click(within(dialog).getByRole("button", { name: "Add program" }));
    await waitFor(() => expect(api.saveProfile).toHaveBeenCalledWith(expect.objectContaining({ name: "My game", color: { vibrance: 50, brightness: 50, gamma: 1.1 }, resolution: state.resolutions[0], blackEqualizer: { strength: 0, range: 50 }, colorRules: [] })));
    expect(await screen.findByRole("button", { name: /My game/ })).toBeInTheDocument();
  });

  it("saves the black equalizer and color rules with the program", async () => {
    const user = await renderApp();
    await user.click(screen.getByRole("button", { name: "Add manually" }));
    const page = await screen.findByRole("region", { name: "New program" });
    const blackEqualizer = within(page).getByRole("region", { name: "Black Equalizer" });
    within(blackEqualizer).getByRole("slider", { name: "Strength" }).focus();
    await user.keyboard("{ArrowRight}{ArrowRight}{ArrowRight}");
    await user.click(within(page).getByRole("button", { name: "Add color" }));
    await user.click(within(page).getByRole("button", { name: "Add color" }));
    await user.click(within(page).getByRole("button", { name: "Remove color 2" }));
    await user.click(within(page).getByRole("switch", { name: "Color 1 enabled" }));
    await user.click(within(page).getByRole("button", { name: "Add program" }));
    await waitFor(() => expect(api.saveProfile).toHaveBeenCalledWith(expect.objectContaining({
      blackEqualizer: { strength: 3, range: 50 },
      colorRules: [expect.objectContaining({ enabled: false, source: "#FFFF00", target: "#FF3BD4", tolerance: 30, strength: 100 })],
    })));
  });

  it("stores and removes the reference screenshot of a program", async () => {
    state.profiles = [profile];
    vi.mocked(api.referenceImage).mockResolvedValue("data:image/png;base64,iVBORw0KGgo=");
    const user = await renderApp();
    await user.click(screen.getByRole("button", { name: /My game/ }));
    const page = await screen.findByRole("region", { name: "Edit My game" });
    await waitFor(() => expect(api.referenceImage).toHaveBeenCalledWith("game"));
    await user.click(await within(page).findByRole("button", { name: "Remove image" }));
    expect(api.deleteReferenceImage).toHaveBeenCalledWith("game");
    const file = new File([new Uint8Array([137, 80, 78, 71])], "shot.png", { type: "image/png" });
    await user.upload(page.querySelector<HTMLInputElement>("input[type=file]")!, file);
    await waitFor(() => expect(api.saveReferenceImage).toHaveBeenCalledWith("game", expect.stringMatching(/^data:image\/png;base64,/)));
  });

  it("filters running processes before opening the selected profile", async () => {
    const user = await renderApp();
    await user.click(screen.getByRole("button", { name: "Add" }));
    const dialog = await screen.findByRole("dialog", { name: "Add a running app" });
    await within(dialog).findByText("Text editor");
    await user.type(within(dialog).getByRole("searchbox", { name: "Search running apps" }), "game.exe");
    expect(within(dialog).queryByText("Text editor")).not.toBeInTheDocument();
    await user.click(within(dialog).getByRole("option", { name: /Game window/ }));
    const editor = await screen.findByRole("region", { name: "New program" });
    expect(within(editor).getByRole("textbox", { name: "Program name" })).toHaveValue("Game window");
  });

  it("keeps a failed save draft available for retry", async () => {
    vi.mocked(api.saveProfile).mockRejectedValueOnce(new Error("Configuration could not be written"));
    const user = await renderApp();
    await user.click(screen.getByRole("button", { name: "Add manually" }));
    const dialog = await screen.findByRole("region", { name: "New program" });
    await user.click(within(dialog).getByRole("button", { name: "Add program" }));
    expect(await screen.findByText("Configuration could not be written")).toBeInTheDocument();
    expect(within(dialog).getByRole("textbox", { name: "Program name" })).toHaveValue("My game");
    await user.click(within(dialog).getByRole("button", { name: "Add program" }));
    await waitFor(() => expect(api.saveProfile).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(screen.queryByRole("region", { name: "New program" })).not.toBeInTheDocument());
  });

  it("requires confirmation before removing a profile and can undo it", async () => {
    state.profiles = [profile];
    const user = await renderApp();
    await user.click(screen.getByRole("button", { name: /My game/ }));
    const editor = await screen.findByRole("region", { name: "Edit My game" });
    await user.click(within(editor).getByRole("button", { name: "Remove" }));
    const confirm = await screen.findByRole("alertdialog", { name: "Remove My game?" });
    expect(api.removeProfile).not.toHaveBeenCalled();
    await user.click(within(confirm).getByRole("button", { name: "Remove" }));
    await waitFor(() => expect(api.removeProfile).toHaveBeenCalledWith("game"));
    expect(await screen.findByText("No programs yet")).toBeInTheDocument();
    await user.click(await screen.findByRole("button", { name: "Undo" }));
    await waitFor(() => expect(api.saveProfile).toHaveBeenCalledWith(profile));
  });

  it("marks the profile that is applied right now", async () => {
    state.profiles = [profile];
    state.status.activeProfileId = "game";
    await renderApp();
    expect(screen.getByRole("button", { name: /My game/ })).toHaveTextContent("Active");
    expect(screen.getByRole("contentinfo")).toHaveTextContent("Running·My game".replace("·", ""));
  });

  it("pauses and resumes the native observer", async () => {
    const user = await renderApp();
    await user.click(screen.getByRole("button", { name: "Pause" }));
    expect(api.setEnabled).toHaveBeenCalledWith(false);
    expect(await screen.findByText("Paused")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Resume" }));
    expect(api.setEnabled).toHaveBeenLastCalledWith(true);
    expect(await screen.findByText("Running")).toBeInTheDocument();
  });

  it("disables only the color controls unsupported by the selected hardware", async () => {
    state.status.supportsGamma = false;
    await renderApp();
    expect(screen.getByRole("slider", { name: "Digital vibrance" })).toBeEnabled();
    expect(screen.getByRole("slider", { name: "Brightness" })).toBeDisabled();
    expect(screen.getByRole("slider", { name: "Gamma" })).toBeDisabled();
  });

  it("commits keyboard Windows color adjustments when interaction ends", async () => {
    const user = await renderApp();
    screen.getByRole("slider", { name: "Brightness" }).focus();
    await user.keyboard("{ArrowRight}");
    await waitFor(() => expect(api.saveDesktop).toHaveBeenCalledWith({ vibrance: 50, brightness: 51, gamma: 1 }));
  });
});
