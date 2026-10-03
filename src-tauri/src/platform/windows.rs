//! Windows display discovery, reversible adjustments, and foreground inspection.
//! Baselines outlive temporary disconnections so reconnecting the same display
//! does not accidentally make an active game profile its new desktop baseline.

use std::{
    collections::{BTreeMap, BTreeSet},
    ptr,
};

use windows_sys::Win32::{
    Devices::Display::{
        DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO,
        DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME, DISPLAYCONFIG_DEVICE_INFO_HEADER,
        DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO, DISPLAYCONFIG_MODE_INFO, DISPLAYCONFIG_PATH_INFO,
        DISPLAYCONFIG_SOURCE_DEVICE_NAME, DisplayConfigGetDeviceInfo, GetDisplayConfigBufferSizes,
        QDC_ONLY_ACTIVE_PATHS, QueryDisplayConfig,
    },
    Foundation::{CloseHandle, ERROR_INSUFFICIENT_BUFFER, HWND, LPARAM},
    Graphics::Gdi::{
        CDS_TEST, ChangeDisplaySettingsExW, CreateDCW, DEVMODEW, DISP_CHANGE_SUCCESSFUL,
        DISPLAY_DEVICE_ATTACHED_TO_DESKTOP, DISPLAY_DEVICE_MIRRORING_DRIVER,
        DISPLAY_DEVICE_PRIMARY_DEVICE, DISPLAY_DEVICEW, DM_DISPLAYFREQUENCY, DM_PELSHEIGHT,
        DM_PELSWIDTH, DeleteDC, ENUM_CURRENT_SETTINGS, EnumDisplayDevicesW, EnumDisplaySettingsExW,
        GetMonitorInfoW, HDC, MONITOR_DEFAULTTONEAREST, MONITORINFOEXW, MonitorFromWindow,
    },
    System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
    },
    UI::{
        ColorSystem::{GetDeviceGammaRamp, SetDeviceGammaRamp},
        WindowsAndMessaging::{
            EnumWindows, GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId,
            IsWindowVisible,
        },
    },
};

use super::{
    DisplayInfo, DisplayMode, ForegroundApp, GammaRamp, PlatformError, RunningApp, compose_gamma,
    validate_adjustments,
    vendor::{Vendors, VibranceTarget},
};

struct Snapshot {
    identity: String,
    mode: DEVMODEW,
    gamma: Option<GammaRamp>,
    vibrance: Option<VibranceTarget>,
    mode_changed: bool,
    gamma_changed: bool,
    vibrance_changed: bool,
}

/// Owns the captured desktop state and restores modified connected displays on drop.
/// Callers serialize access; construction and discovery never change display state.
pub(crate) struct NativeController {
    displays: Vec<DisplayInfo>,
    snapshots: BTreeMap<String, Snapshot>,
    vendors: Vendors,
}

impl NativeController {
    /// Discovers active displays and captures restorable settings before any write.
    pub(crate) fn new() -> Result<Self, PlatformError> {
        let mut controller = Self {
            displays: Vec::new(),
            snapshots: BTreeMap::new(),
            vendors: Vendors::new(),
        };
        controller.refresh_displays()?;
        Ok(controller)
    }

    /// Returns the last discovered capabilities and the latest observed desktop modes.
    pub(crate) fn displays(&self) -> Vec<DisplayInfo> {
        self.displays.clone()
    }

    /// Reports unfinished restoration, including temporarily disconnected outputs.
    pub(crate) fn has_pending_restore(&self) -> bool {
        self.snapshots.values().any(|snapshot| {
            snapshot.mode_changed || snapshot.gamma_changed || snapshot.vibrance_changed
        })
    }

    /// Rescans attached outputs without replacing baselines of recognized displays.
    /// Disconnected snapshots remain available if that display reconnects later.
    pub(crate) fn refresh_displays(&mut self) -> Result<(), PlatformError> {
        let discovered = enumerate_displays()?;
        let mut displays = Vec::with_capacity(discovered.len());
        for (mut info, identity, mode) in discovered {
            if !self
                .snapshots
                .get(&info.id)
                .is_some_and(|snapshot| snapshot.identity == identity)
            {
                let gamma = read_gamma(&info.id).ok();
                let vibrance = self.vendors.capture(&info.id);
                self.snapshots.insert(
                    info.id.clone(),
                    Snapshot {
                        identity,
                        mode,
                        gamma,
                        vibrance,
                        mode_changed: false,
                        gamma_changed: false,
                        vibrance_changed: false,
                    },
                );
            }
            if let Some(snapshot) = self.snapshots.get_mut(&info.id) {
                if snapshot.vibrance.is_none() && !snapshot.vibrance_changed {
                    snapshot.vibrance = self.vendors.capture(&info.id);
                }
                if snapshot.gamma.is_none() {
                    snapshot.gamma = read_gamma(&info.id).ok();
                }
                info.vibrance_supported = snapshot.vibrance.is_some();
                info.gamma_supported =
                    snapshot.gamma.is_some() && advanced_color_enabled(&info.id) == Some(false);
            }
            displays.push(info);
        }
        self.displays = displays;
        Ok(())
    }

    /// Applies supported controls and reports every failure, retaining restoration
    /// bookkeeping for successful writes. `None` restores a previously changed mode.
    pub(crate) fn apply(
        &mut self,
        display_id: &str,
        vibrance: f64,
        brightness: f64,
        gamma: f64,
        resolution: Option<&DisplayMode>,
    ) -> Result<(), PlatformError> {
        validate_adjustments(vibrance, brightness, gamma)?;
        if !self.displays.iter().any(|display| display.id == display_id) {
            return Err(PlatformError::DisplayMissing(display_id.to_owned()));
        }
        let snapshot = self
            .snapshots
            .get_mut(display_id)
            .ok_or_else(|| PlatformError::DisplayMissing(display_id.to_owned()))?;
        let mut errors = Vec::new();

        let mode_result = match resolution {
            Some(mode) => set_mode(display_id, mode),
            None if snapshot.mode_changed => restore_mode(display_id, &snapshot.mode),
            None => Ok(false),
        };
        match mode_result {
            Ok(changed) => {
                // A mode switch can reset the hardware LUT even when the requested
                // color profile is neutral, so reapply the captured calibration.
                snapshot.gamma_changed |= changed && snapshot.gamma.is_some();
                if resolution.is_some() {
                    snapshot.mode_changed |= changed;
                } else {
                    snapshot.mode_changed = false;
                }
            }
            Err(error) => errors.push(error.to_string()),
        }

        match &snapshot.vibrance {
            Some(target) => {
                snapshot.vibrance_changed = true;
                if let Err(error) = self.vendors.apply(display_id, target, vibrance) {
                    errors.push(error.to_string());
                }
            }
            None if vibrance != 50.0 => errors.push(format!("digital vibrance is not supported on {display_id}; an NVIDIA or AMD display driver with color controls is required")),
            None => {},
        }

        match &snapshot.gamma {
            Some(baseline) if brightness != 50.0 || gamma != 1.0 || snapshot.gamma_changed => {
                let ramp = compose_gamma(baseline, brightness, gamma);
                // A driver can reject verification after changing its LUT; leave
                // restoration pending until the complete operation succeeds.
                snapshot.gamma_changed = true;
                match write_gamma(display_id, &ramp) {
                    Ok(()) => snapshot.gamma_changed = brightness != 50.0 || gamma != 1.0,
                    Err(error) => errors.push(error.to_string()),
                }
            }
            None if brightness != 50.0 || gamma != 1.0 => errors.push(format!(
                "brightness and gamma are not supported on {display_id}"
            )),
            _ => {}
        }
        if let Ok(mode) = current_mode(display_id)
            && let Some(display) = self
                .displays
                .iter_mut()
                .find(|display| display.id == display_id)
        {
            display.current_mode = public_mode(&mode);
        }
        collect_errors(errors)
    }

    /// Restores every changed, currently connected display, retaining failed work
    /// for a later retry. Unplugged outputs are left pending until they reconnect.
    pub(crate) fn restore_all(&mut self) -> Result<(), PlatformError> {
        let mut errors = Vec::new();
        for display in &mut self.displays {
            let Some(snapshot) = self.snapshots.get_mut(&display.id) else {
                continue;
            };
            if snapshot.mode_changed {
                match restore_mode(&display.id, &snapshot.mode) {
                    Ok(changed) => {
                        snapshot.gamma_changed |= changed && snapshot.gamma.is_some();
                        snapshot.mode_changed = false;
                        display.current_mode = public_mode(&snapshot.mode);
                    }
                    Err(error) => errors.push(error.to_string()),
                }
            }
            if snapshot.vibrance_changed
                && let Some(target) = &snapshot.vibrance
            {
                match self.vendors.restore(&display.id, target) {
                    Ok(()) => snapshot.vibrance_changed = false,
                    Err(error) => errors.push(error.to_string()),
                }
            }
            if snapshot.gamma_changed
                && let Some(ramp) = &snapshot.gamma
            {
                match write_gamma(&display.id, ramp) {
                    Ok(()) => snapshot.gamma_changed = false,
                    Err(error) => errors.push(error.to_string()),
                }
            }
        }
        collect_errors(errors)
    }
}

impl Drop for NativeController {
    fn drop(&mut self) {
        let _ = self.restore_all();
    }
}

fn collect_errors(errors: Vec<String>) -> Result<(), PlatformError> {
    if errors.is_empty() {
        Ok(())
    } else {
        Err(PlatformError::Partial(errors.join("; ")))
    }
}

fn win_error(operation: &'static str) -> PlatformError {
    PlatformError::Windows {
        operation,
        source: std::io::Error::last_os_error(),
    }
}

fn wide(value: &str) -> Result<Vec<u16>, PlatformError> {
    if value.contains('\0') {
        return Err(PlatformError::InvalidValue("display name"));
    }
    Ok(value.encode_utf16().chain(std::iter::once(0)).collect())
}

fn from_wide(value: &[u16]) -> String {
    let end = value
        .iter()
        .position(|value| *value == 0)
        .unwrap_or(value.len());
    String::from_utf16_lossy(&value[..end])
}

fn display_device() -> DISPLAY_DEVICEW {
    DISPLAY_DEVICEW {
        cb: u32::try_from(size_of::<DISPLAY_DEVICEW>()).expect("DISPLAY_DEVICEW fits in u32"),
        ..Default::default()
    }
}

fn devmode() -> DEVMODEW {
    DEVMODEW {
        dmSize: u16::try_from(size_of::<DEVMODEW>()).expect("DEVMODEW fits in u16"),
        ..Default::default()
    }
}

fn enumerate_displays() -> Result<Vec<(DisplayInfo, String, DEVMODEW)>, PlatformError> {
    let mut displays = Vec::new();
    for index in 0..64 {
        let mut adapter = display_device();
        // SAFETY: a null device enumerates adapters and adapter is a sized writable structure.
        if unsafe { EnumDisplayDevicesW(ptr::null(), index, &mut adapter, 0) } == 0 {
            break;
        }
        if adapter.StateFlags & DISPLAY_DEVICE_ATTACHED_TO_DESKTOP == 0
            || adapter.StateFlags & DISPLAY_DEVICE_MIRRORING_DRIVER != 0
        {
            continue;
        }
        let id = from_wide(&adapter.DeviceName);
        let mode = match current_mode(&id) {
            Ok(mode) => mode,
            Err(_) => continue,
        };
        let mut monitor = display_device();
        // SAFETY: adapter's fixed DeviceName buffer is terminated by EnumDisplayDevicesW.
        let found_monitor =
            unsafe { EnumDisplayDevicesW(adapter.DeviceName.as_ptr(), 0, &mut monitor, 0) } != 0;
        let name = if found_monitor {
            from_wide(&monitor.DeviceString)
        } else {
            id.clone()
        };
        let identity = format!(
            "{}:{}",
            from_wide(&adapter.DeviceID),
            from_wide(&monitor.DeviceID)
        );
        displays.push((
            DisplayInfo {
                id,
                name,
                adapter: from_wide(&adapter.DeviceString),
                primary: adapter.StateFlags & DISPLAY_DEVICE_PRIMARY_DEVICE != 0,
                vibrance_supported: false,
                gamma_supported: false,
                current_mode: public_mode(&mode),
            },
            identity,
            mode,
        ));
    }
    if displays.is_empty() {
        return Err(PlatformError::Unavailable(
            "no active desktop displays were found".into(),
        ));
    }
    Ok(displays)
}

fn current_mode(display_id: &str) -> Result<DEVMODEW, PlatformError> {
    let name = wide(display_id)?;
    let mut mode = devmode();
    // SAFETY: name is terminated and mode has the size expected by this Windows SDK.
    if unsafe { EnumDisplaySettingsExW(name.as_ptr(), ENUM_CURRENT_SETTINGS, &mut mode, 0) } == 0 {
        return Err(win_error("read display mode"));
    }
    Ok(mode)
}

fn public_mode(mode: &DEVMODEW) -> DisplayMode {
    DisplayMode {
        width: mode.dmPelsWidth,
        height: mode.dmPelsHeight,
        refresh_rate: mode.dmDisplayFrequency,
    }
}

/// Returns unique driver-supported modes; only 32-bit desktop color modes qualify.
pub(crate) fn display_modes(display_id: &str) -> Result<Vec<DisplayMode>, PlatformError> {
    let name = wide(display_id)?;
    current_mode(display_id)?;
    let mut modes = BTreeSet::new();
    for index in 0..4096 {
        let mut mode = devmode();
        // SAFETY: index is a bounded enumeration ordinal; name and mode are valid for the call.
        if unsafe { EnumDisplaySettingsExW(name.as_ptr(), index, &mut mode, 0) } == 0 {
            break;
        }
        if mode.dmBitsPerPel == 32 && mode.dmPelsWidth > 0 && mode.dmPelsHeight > 0 {
            modes.insert(public_mode(&mode));
        }
    }
    Ok(modes.into_iter().rev().collect())
}

fn set_mode(display_id: &str, wanted: &DisplayMode) -> Result<bool, PlatformError> {
    let mut mode = current_mode(display_id)?;
    if public_mode(&mode) == *wanted {
        return Ok(false);
    }
    if !display_modes(display_id)?.contains(wanted) {
        return Err(PlatformError::Unavailable(format!(
            "{} × {} at {} Hz is not supported on {display_id}",
            wanted.width, wanted.height, wanted.refresh_rate
        )));
    }
    mode.dmPelsWidth = wanted.width;
    mode.dmPelsHeight = wanted.height;
    mode.dmDisplayFrequency = wanted.refresh_rate;
    mode.dmFields = DM_PELSWIDTH | DM_PELSHEIGHT | DM_DISPLAYFREQUENCY;
    change_mode(display_id, &mode)?;
    Ok(true)
}

fn restore_mode(display_id: &str, original: &DEVMODEW) -> Result<bool, PlatformError> {
    let current = current_mode(display_id)?;
    if public_mode(&current) == public_mode(original) {
        return Ok(false);
    }
    change_mode(display_id, original)?;
    Ok(true)
}

fn change_mode(display_id: &str, mode: &DEVMODEW) -> Result<(), PlatformError> {
    let name = wide(display_id)?;
    // SAFETY: mode comes from Windows with a correct dmSize; no driver-private
    // extra data is requested. CDS_TEST verifies before the non-persistent write.
    let tested = unsafe {
        ChangeDisplaySettingsExW(name.as_ptr(), mode, ptr::null_mut(), CDS_TEST, ptr::null())
    };
    if tested != DISP_CHANGE_SUCCESSFUL {
        return Err(PlatformError::Driver {
            vendor: "Windows",
            operation: "test display mode",
            status: tested,
        });
    }
    // SAFETY: the same validated mode and terminated name are live for this call;
    // flags 0 change only this session, never the registry's saved display mode.
    let status =
        unsafe { ChangeDisplaySettingsExW(name.as_ptr(), mode, ptr::null_mut(), 0, ptr::null()) };
    if status != DISP_CHANGE_SUCCESSFUL {
        return Err(PlatformError::Driver {
            vendor: "Windows",
            operation: "change display mode",
            status,
        });
    }
    Ok(())
}

struct DeviceContext(HDC);

impl DeviceContext {
    fn for_display(display_id: &str) -> Result<Self, PlatformError> {
        let name = wide(display_id)?;
        // SAFETY: DISPLAY and name are terminated strings; no port or device mode is used.
        let dc = unsafe {
            CreateDCW(
                windows_sys::w!("DISPLAY"),
                name.as_ptr(),
                ptr::null(),
                ptr::null(),
            )
        };
        if dc.is_null() {
            return Err(win_error("open display context"));
        }
        Ok(Self(dc))
    }
}

impl Drop for DeviceContext {
    fn drop(&mut self) {
        // SAFETY: this handle was created by CreateDCW and is released exactly once.
        unsafe {
            DeleteDC(self.0);
        }
    }
}

fn read_gamma(display_id: &str) -> Result<GammaRamp, PlatformError> {
    require_sdr(display_id)?;
    let dc = DeviceContext::for_display(display_id)?;
    let mut ramp = [[0; 256]; 3];
    // SAFETY: GammaRamp has exactly the contiguous 3×256 WORD layout required by GDI.
    if unsafe { GetDeviceGammaRamp(dc.0, ramp.as_mut_ptr().cast()) } == 0 {
        return Err(PlatformError::Unavailable(format!(
            "the display driver cannot read a gamma ramp for {display_id}"
        )));
    }
    if ramp
        .iter()
        .any(|channel| channel.iter().all(|sample| *sample == 0))
    {
        return Err(PlatformError::Unavailable(format!(
            "the display driver returned an invalid gamma ramp for {display_id}"
        )));
    }
    Ok(ramp)
}

fn write_gamma(display_id: &str, ramp: &GammaRamp) -> Result<(), PlatformError> {
    require_sdr(display_id)?;
    let dc = DeviceContext::for_display(display_id)?;
    // SAFETY: ramp remains live and has GDI's contiguous 3×256 WORD layout.
    if unsafe { SetDeviceGammaRamp(dc.0, ramp.as_ptr().cast()) } == 0 {
        return Err(PlatformError::Unavailable(format!(
            "the display driver rejected brightness/gamma for {display_id}; use an SDR display and supported driver"
        )));
    }
    let actual = read_gamma(display_id)?;
    // Windows may report success while rejecting unsafe curves. Allow one
    // 8-bit quantization step because some DACs expose a lower precision LUT.
    if actual
        .iter()
        .flatten()
        .zip(ramp.iter().flatten())
        .any(|(actual, wanted)| actual.abs_diff(*wanted) > 257)
    {
        return Err(PlatformError::Unavailable(format!(
            "the display driver did not retain brightness/gamma for {display_id}; another color manager or game may control the ramp"
        )));
    }
    Ok(())
}

fn require_sdr(display_id: &str) -> Result<(), PlatformError> {
    match advanced_color_enabled(display_id) {
        Some(false) => Ok(()),
        Some(true) => Err(PlatformError::Unavailable(format!(
            "brightness/gamma require SDR mode on {display_id}"
        ))),
        None => Err(PlatformError::Unavailable(format!(
            "brightness/gamma are unavailable because Windows could not determine the color mode for {display_id}"
        ))),
    }
}

fn advanced_color_enabled(display_id: &str) -> Option<bool> {
    for _ in 0..3 {
        let mut path_count = 0;
        let mut mode_count = 0;
        // SAFETY: both counters are writable and QDC_ONLY_ACTIVE_PATHS requests
        // bounded desktop topology metadata without changing configuration.
        let status = unsafe {
            GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut path_count, &mut mode_count)
        };
        if status != 0 || path_count > 256 || mode_count > 1024 {
            return None;
        }
        let Ok(paths_len) = usize::try_from(path_count) else {
            return None;
        };
        let Ok(modes_len) = usize::try_from(mode_count) else {
            return None;
        };
        let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); paths_len];
        let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); modes_len];
        // SAFETY: the initialized buffers match their input counts, which Windows
        // updates on success; null topology is required for active-path queries.
        let status = unsafe {
            QueryDisplayConfig(
                QDC_ONLY_ACTIVE_PATHS,
                &mut path_count,
                paths.as_mut_ptr(),
                &mut mode_count,
                modes.as_mut_ptr(),
                ptr::null_mut(),
            )
        };
        if status == ERROR_INSUFFICIENT_BUFFER {
            continue;
        }
        if status != 0 {
            return None;
        }
        let mut found = false;
        let mut unknown = false;
        for path in paths.iter().take(usize::try_from(path_count).unwrap_or(0)) {
            let mut name = DISPLAYCONFIG_SOURCE_DEVICE_NAME {
                header: DISPLAYCONFIG_DEVICE_INFO_HEADER {
                    r#type: DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME,
                    size: u32::try_from(size_of::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>())
                        .expect("display source structure fits in u32"),
                    adapterId: path.sourceInfo.adapterId,
                    id: path.sourceInfo.id,
                },
                ..Default::default()
            };
            // SAFETY: the source-name structure starts with header, whose type and
            // size identify the full writable structure; IDs came from Windows.
            if unsafe { DisplayConfigGetDeviceInfo(&mut name.header) } != 0 {
                unknown = true;
                continue;
            }
            if !from_wide(&name.viewGdiDeviceName).eq_ignore_ascii_case(display_id) {
                continue;
            }
            found = true;
            let mut color = DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO {
                header: DISPLAYCONFIG_DEVICE_INFO_HEADER {
                    r#type: DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO,
                    size: u32::try_from(size_of::<DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO>())
                        .expect("advanced color structure fits in u32"),
                    adapterId: path.targetInfo.adapterId,
                    id: path.targetInfo.id,
                },
                ..Default::default()
            };
            // SAFETY: header identifies the full color structure and the target IDs are OS outputs.
            if unsafe { DisplayConfigGetDeviceInfo(&mut color.header) } != 0 {
                unknown = true;
                continue;
            }
            // SAFETY: successful output initializes the union's u32 flags;
            // bit 1 is the SDK's advancedColorEnabled field.
            if unsafe { color.Anonymous.value & 2 != 0 } {
                return Some(true);
            }
        }
        return (found && !unknown).then_some(false);
    }
    None
}

fn window_process(window: HWND) -> Option<(u32, String, String)> {
    let mut pid = 0;
    // SAFETY: window is an OS-returned handle and pid is a writable integer output.
    unsafe {
        GetWindowThreadProcessId(window, &mut pid);
    }
    if pid == 0 {
        return None;
    }
    // SAFETY: querying a process by its OS PID requests only limited read access.
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        return None;
    }
    let mut buffer = vec![0u16; 32768];
    let mut length = 32768;
    // SAFETY: process is live and buffer contains length writable UTF-16 units.
    let success =
        unsafe { QueryFullProcessImageNameW(process, 0, buffer.as_mut_ptr(), &mut length) };
    // SAFETY: OpenProcess returned this owned handle; it is no longer used after closure.
    unsafe {
        CloseHandle(process);
    }
    if success == 0 {
        return None;
    }
    let path = String::from_utf16_lossy(buffer.get(..usize::try_from(length).ok()?)?);
    let name = path.rsplit(['\\', '/']).next()?.to_owned();
    Some((pid, name, path))
}

/// Reads the focused executable and its nearest display without activating windows.
pub(crate) fn foreground_app() -> Option<ForegroundApp> {
    // SAFETY: these OS queries do not dereference caller memory.
    let window = unsafe { GetForegroundWindow() };
    if window.is_null() {
        return None;
    }
    let (pid, exe_name, exe_path) = window_process(window)?;
    // SAFETY: window is an OS handle; Windows returns null if it has since disappeared.
    let monitor = unsafe { MonitorFromWindow(window, MONITOR_DEFAULTTONEAREST) };
    if monitor.is_null() {
        return None;
    }
    let mut info = MONITORINFOEXW::default();
    info.monitorInfo.cbSize = u32::try_from(size_of::<MONITORINFOEXW>()).ok()?;
    // SAFETY: MONITORINFOEXW begins with MONITORINFO and cbSize requests the full structure.
    if unsafe { GetMonitorInfoW(monitor, &mut info.monitorInfo) } == 0 {
        return None;
    }
    Some(ForegroundApp {
        exe_name,
        exe_path,
        display_id: from_wide(&info.szDevice),
        pid,
    })
}

/// Enumerates one visible titled window per executable, skipping inaccessible processes.
pub(crate) fn running_apps() -> Vec<RunningApp> {
    let mut apps: Vec<RunningApp> = Vec::new();
    // SAFETY: EnumWindows calls synchronously and receives this live vector's
    // address. The callback catches panics before returning across the FFI boundary.
    unsafe {
        EnumWindows(
            Some(enumerate_window),
            (&mut apps as *mut Vec<RunningApp>) as LPARAM,
        );
    }
    apps.sort_by_cached_key(|app| (app.exe_name.to_lowercase(), app.exe_path.to_lowercase()));
    apps.dedup_by(|left, right| left.exe_path.eq_ignore_ascii_case(&right.exe_path));
    apps
}

unsafe extern "system" fn enumerate_window(window: HWND, parameter: LPARAM) -> i32 {
    let result = std::panic::catch_unwind(|| {
        // SAFETY: EnumWindows supplies a valid window handle for this callback.
        if unsafe { IsWindowVisible(window) } == 0 {
            return;
        }
        let mut title = [0u16; 1024];
        // SAFETY: title has 1024 writable elements; Windows truncates longer titles.
        let length = unsafe { GetWindowTextW(window, title.as_mut_ptr(), 1024) };
        if length <= 0 {
            return;
        }
        if let Some((pid, exe_name, exe_path)) = window_process(window) {
            if pid == std::process::id() {
                return;
            }
            // SAFETY: only running_apps invokes this callback, passing a uniquely
            // borrowed Vec that remains live throughout synchronous enumeration.
            let apps = unsafe { &mut *(parameter as *mut Vec<RunningApp>) };
            apps.push(RunningApp {
                exe_name,
                exe_path,
                title: from_wide(&title),
                pid,
            });
        }
    });
    i32::from(result.is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires an interactive Windows desktop and installed display drivers"]
    fn discover_local_displays_without_changing_settings() {
        let controller = NativeController::new().unwrap();
        let displays = controller.displays();
        assert!(!displays.is_empty());
        assert!(displays.iter().any(|display| display.primary));
        for display in displays {
            assert!(display.current_mode.width > 0 && display.current_mode.height > 0);
            assert!(!display_modes(&display.id).unwrap().is_empty());
            eprintln!("{display:?}");
        }
        assert!(
            controller
                .snapshots
                .values()
                .all(|snapshot| !snapshot.mode_changed
                    && !snapshot.gamma_changed
                    && !snapshot.vibrance_changed)
        );
    }

    #[test]
    #[ignore = "writes physical display color briefly; requires an interactive SDR desktop and no running app instance"]
    fn physical_color_roundtrip_restores_original_values() {
        let mut controller = NativeController::new().unwrap();
        let display = controller
            .displays()
            .into_iter()
            .find(|display| {
                display.primary && display.vibrance_supported && display.gamma_supported
            })
            .expect("this opt-in test requires a supported SDR primary display");
        let original_vibrance = controller.vendors.capture(&display.id).unwrap();
        let original_gamma = read_gamma(&display.id).unwrap();
        let original_mode = public_mode(&current_mode(&display.id).unwrap());
        let percent = original_vibrance.slightly_adjusted_percent();
        let applied = controller.apply(&display.id, percent, 51.0, 1.02, None);
        let active_vibrance = controller.vendors.capture(&display.id);
        let active_gamma = read_gamma(&display.id);
        // Always restore before assertions, including the driver-rejection path.
        // Drop is a second restoration attempt if any assertion then fails.
        let restored = controller.restore_all();
        let restored_vibrance = controller.vendors.capture(&display.id);
        let restored_gamma = read_gamma(&display.id);
        assert_eq!(
            public_mode(&current_mode(&display.id).unwrap()),
            original_mode
        );
        restored.unwrap();
        assert_eq!(restored_vibrance, Some(original_vibrance));
        assert_eq!(restored_gamma.unwrap(), original_gamma);
        assert!(!controller.has_pending_restore());
        applied.unwrap();
        assert_ne!(active_vibrance, restored_vibrance);
        assert_ne!(active_gamma.unwrap(), original_gamma);
    }
}
