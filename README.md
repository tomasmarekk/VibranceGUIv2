# VibranceGUI v2

A Windows desktop utility that switches digital vibrance, brightness, gamma and optional display resolution when you focus a game or application. The native controller is written in Rust; the desktop interface uses Tauri 2, React and HeroUI v3 in a dark theme only.

The layout keeps the original VibranceGUI workflow in one window with a built-in title bar: global settings and Windows colors above a program library with **Add** and **Add manually**. Each program appears as a tile with its own icon; selecting it opens its color profile, where it can be saved or removed. The implementation and visual assets are new; the original reference sources are not distributed.

## Install

Download the latest Windows x64 build from [Releases](https://github.com/tomasmarekk/VibranceGUIv2/releases).

- `VibranceGUIv2_<version>_x64-setup.exe` installs for your Windows user. It can download Microsoft's WebView2 Runtime if needed.
- `VibranceGUIv2_<version>_x64-portable.exe` runs without installing the application; WebView2 must already be installed. Settings still live in your Windows user profile.
- `SHA256SUMS.txt` contains checksums for the binaries and license notices. In PowerShell, use `Get-FileHash .\<downloaded-file>.exe -Algorithm SHA256` and compare the result.

The executables are currently **unsigned**, so Windows may show an unknown-publisher or SmartScreen prompt. Use the repository's release page as the download source.

Windows 10/11 x64 and an installed display driver are required. Digital vibrance depends on the connected output exposing NVIDIA NVAPI or AMD ADL color controls. Brightness and gamma use the Windows SDR gamma ramp and do not change a monitor's physical backlight. HDR, hybrid graphics, calibration tools and games with their own color pipeline can restrict these controls.

## Use

1. Set your **Windows colors**: digital vibrance, brightness and gamma.
2. Choose **Add** for a running application, or **Add manually** for an executable.
3. Adjust its profile, optionally select a supported resolution, and choose **Add program**. Select a program tile later to change or remove its profile.
4. Focus that application to activate its profile; focus another application to return to the Windows colors.

### Program graphics

Program profiles have two extra tools that the Windows colors do not:

- **Black Equalizer** brightens dark tones while leaving bright ones unchanged. **Strength** sets how much, **Range** how far into the midtones it reaches. It is applied through the same driver gamma ramp as brightness and gamma, so it works in every display mode.
- **Color Equalizer** finds one color on screen, such as an enemy highlight, and replaces it, boosts its saturation or changes its brightness. **Match range** decides how similar a pixel must be; darker shades of the same color match too. Up to four colors per program.

Add a screenshot of the game to the program's preview by pasting it (Ctrl+V), dropping a file or choosing one. Use the eyedropper to pick the color from the screenshot and compare **Before** and **After** while adjusting. The preview reproduces the gamma ramp and color matching exactly and approximates digital vibrance. Screenshots stay on your PC next to the settings.

Drivers cannot change a single color, so the Color Equalizer captures the display with Windows Desktop Duplication and draws only the matching pixels in a transparent, click-through window above the game. It does not inject into or read from the game process. It needs an SDR display and a game running in windowed fullscreen (also called borderless) or windowed mode, adds about one frame of delay to the recolored pixels only, and is hidden from screenshots and recordings. In exclusive fullscreen the game bypasses the Windows compositor, so no window can appear above it; the Color Equalizer then pauses and the status line asks you to switch the game's display mode. In Valorant, for example, choose **Settings → Video → General → Display Mode → Windowed Fullscreen**. Anti-cheat rules on screen overlays differ between games; check them before using it in competitive play.

The native observer checks foreground changes every 150 ms. **Primary monitor only** means the Windows primary display; otherwise profiles apply to all connected supported displays. Profiles match an executable name by default, with exact-path matching available. **Never change resolution** disables profile resolution changes. Pause or exit restores the display state captured before the app took control. Minimize hides the window to the tray; closing the window or choosing **Exit and restore display settings** in the tray menu quits the application. Autostart launches minimized.

Settings and the application log are stored at `%APPDATA%\com.tomasmarekk.vibranceguiv2\settings.json` and `application.log`. Missing game executables do not delete saved profiles.

Launch with `--paused` to inspect the interface and detected capabilities before applying any display settings. The tray menu or interface can then resume profiles. `--diagnostics` emits capability JSON without applying color or resolution changes; for a GUI executable, redirect its output explicitly:

```powershell
Start-Process -FilePath .\VibranceGUIv2_2.2.1_x64-portable.exe `
    -ArgumentList '--diagnostics' -Wait -WindowStyle Hidden `
    -RedirectStandardOutput .\diagnostics.json -RedirectStandardError .\diagnostics-errors.txt
```

Use the executable filename for your downloaded version. The JSON can include local process paths; review it before sharing. Hosted CI tests and builds the app, while display behavior still requires verification on the target GPU and monitor.

## Development

Install Node.js 24, stable Rust, and [Tauri's Windows prerequisites](https://v2.tauri.app/start/prerequisites/#windows): Microsoft C++ Build Tools with the desktop C++ workload and WebView2. The Rust workspace uses edition 2024 and commits its `Cargo.lock`; npm uses `package-lock.json`.

```powershell
npm ci
npm run tauri -- dev
```

For frontend-only development, `npm run dev` opens the browser preview. Native display controls require the Tauri application.

Run the complete checks and produce the same assets as CI:

```powershell
pwsh -File scripts/verify.ps1 -Build
```

The script runs the private-reference/version guard, `npm ci`, TypeScript checking, frontend tests/build, `cargo fmt --all --check`, locked workspace Clippy/tests/documentation, and a production Tauri NSIS build. Packaged binaries and notices are staged in `artifacts/`. Omit `-Build` to run only validation.

The source areas are `src/` for HeroUI and the IPC client, `src-tauri/src/` for settings, the observer and native Windows/driver integration, and `scripts/` for repository checks and packaging. Unit tests use controlled inputs; hosted CI does not prove that every physical monitor/driver accepts a color change.

## Release

Releases are manual. After updating the synchronized package versions, push the changes to `main`, wait for **Windows CI** to succeed, then run **Actions → Release Windows → Run workflow** from `main` and enter the checked-in version, for example `2.0.0`.

Keep versions synchronized in `package.json`, both root version entries of `package-lock.json`, `src-tauri/Cargo.toml`, the application entry of `Cargo.lock`, and `src-tauri/tauri.conf.json`. The workflow repeats validation, creates a tag pointing to the tested commit, uploads the portable executable, NSIS installer, SHA-256 checksums and license texts, then publishes the release. Existing tags/releases and mismatched versions are rejected. No push automatically publishes a release. If publication fails after uploading assets, inspect the retained draft before retrying; published tags are never overwritten.

## License

New project code is [MIT licensed](LICENSE). Third-party libraries retain their own licenses; their license texts are included in `THIRD-PARTY-LICENSES.txt` with each release. NVIDIA and AMD driver DLLs come from the installed vendor driver and are not bundled.
