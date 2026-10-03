//! Narrow, dynamically loaded interfaces to installed NVIDIA and AMD drivers.
//! No redistributable vendor binaries are bundled. The process retains each DLL
//! until its context is destroyed, and each display retains its original value.

use std::{
    ffi::{CString, c_char, c_void},
    ptr,
};

use libloading::{Library, os::windows::Library as WindowsLibrary};
use windows_sys::Win32::System::{
    LibraryLoader::LOAD_LIBRARY_SEARCH_SYSTEM32,
    Memory::{GetProcessHeap, HeapAlloc, HeapFree},
};

use super::{PlatformError, driver_level};

/// Owns optional driver sessions; absence is a capability result, not a startup failure.
pub(super) struct Vendors {
    nvidia: Option<Nvidia>,
    amd: Option<Amd>,
}

/// Captured per-display driver range and the exact value to restore on shutdown.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum VibranceTarget {
    Nvidia(NvDvcInfo),
    Amd(ColorRange),
}

#[cfg(test)]
impl VibranceTarget {
    pub(super) fn slightly_adjusted_percent(&self) -> f64 {
        let (current, min, neutral, max) = match self {
            Self::Nvidia(info) => (info.current, info.min, info.default, info.max),
            Self::Amd(range) => (range.current, range.min, range.default, range.max),
        };
        let percent = if current >= neutral && max != neutral {
            50.0 + 50.0 * (f64::from(current) - f64::from(neutral))
                / (f64::from(max) - f64::from(neutral))
        } else if min != neutral {
            50.0 * (f64::from(current) - f64::from(min)) / (f64::from(neutral) - f64::from(min))
        } else {
            50.0
        };
        if percent >= 99.0 {
            percent - 1.0
        } else {
            percent + 1.0
        }
    }
}

impl Vendors {
    /// Loads only system-installed drivers and initializes independently owned sessions.
    pub(super) fn new() -> Self {
        Self {
            nvidia: Nvidia::load(),
            amd: Amd::load(),
        }
    }

    /// Reads the current control for a GDI display name without changing its value.
    pub(super) fn capture(&self, name: &str) -> Option<VibranceTarget> {
        if let Some(driver) = &self.nvidia
            && let Ok(info) = driver.info(name)
        {
            return Some(VibranceTarget::Nvidia(info));
        }
        self.amd
            .as_ref()?
            .capture(name)
            .map(|display| VibranceTarget::Amd(display.range))
    }

    /// Maps a validated percentage around the driver's neutral point and writes it.
    pub(super) fn apply(
        &self,
        name: &str,
        target: &VibranceTarget,
        percent: f64,
    ) -> Result<(), PlatformError> {
        match target {
            VibranceTarget::Nvidia(original) => {
                let level = driver_level(percent, original.min, original.default, original.max, 1);
                self.nvidia
                    .as_ref()
                    .ok_or_else(|| unavailable("NVIDIA"))?
                    .set(name, *original, level)
            }
            VibranceTarget::Amd(_) => self
                .amd
                .as_ref()
                .ok_or_else(|| unavailable("AMD"))?
                .set(name, ColorValue::Percent(percent)),
        }
    }

    /// Writes the original captured driver value and reports driver failures unchanged.
    pub(super) fn restore(&self, name: &str, target: &VibranceTarget) -> Result<(), PlatformError> {
        match target {
            VibranceTarget::Nvidia(original) => self
                .nvidia
                .as_ref()
                .ok_or_else(|| unavailable("NVIDIA"))?
                .set(name, *original, original.current),
            VibranceTarget::Amd(range) => self
                .amd
                .as_ref()
                .ok_or_else(|| unavailable("AMD"))?
                .set(name, ColorValue::Native(range.current)),
        }
    }
}

fn unavailable(vendor: &str) -> PlatformError {
    PlatformError::Unavailable(format!("{vendor} digital vibrance is not available"))
}

fn load_driver(name: &str) -> Option<Library> {
    // SAFETY: only fixed vendor DLL names are used; the system-directory-only
    // search prevents a DLL in the application or working directory being loaded.
    unsafe { WindowsLibrary::load_with_flags(name, LOAD_LIBRARY_SEARCH_SYSTEM32) }
        .ok()
        .map(Library::from)
}

fn driver_result(
    vendor: &'static str,
    operation: &'static str,
    status: i32,
) -> Result<(), PlatformError> {
    if status == 0 {
        Ok(())
    } else {
        Err(PlatformError::Driver {
            vendor,
            operation,
            status,
        })
    }
}

// NVAPI's public resolver is documented in NVIDIA/nvapi. DVC EX is the private
// ABI independently described by NvAPIWrapper's Native/Delegates/Display.cs.
type NvQuery = unsafe extern "C" fn(u32) -> *const c_void;
type NvInit = unsafe extern "C" fn() -> i32;
type NvDisplay = unsafe extern "C" fn(*const c_char, *mut *mut c_void) -> i32;
type NvGet = unsafe extern "C" fn(*mut c_void, u32, *mut NvDvcInfo) -> i32;
type NvSet = unsafe extern "C" fn(*mut c_void, u32, *const NvDvcInfo) -> i32;

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Version-1 NVAPI DVC EX layout; its version includes the exact 20-byte size.
pub(super) struct NvDvcInfo {
    version: u32,
    current: i32,
    min: i32,
    max: i32,
    default: i32,
}

struct Nvidia {
    _library: Library,
    unload: NvInit,
    display: NvDisplay,
    get: NvGet,
    set: NvSet,
}

impl Nvidia {
    fn load() -> Option<Self> {
        let library = load_driver(if cfg!(target_pointer_width = "64") {
            "nvapi64.dll"
        } else {
            "nvapi.dll"
        })?;
        // SAFETY: nvapi_QueryInterface is the driver's C ABI resolver. Function
        // signatures below match NVAPI's interface table and DVC EX ABI.
        // DVC EX is a private, versioned interface; absent IDs disable support.
        let (initialize, unload, display, get, set) = unsafe {
            let query = *library.get::<NvQuery>(b"nvapi_QueryInterface\0").ok()?;
            let init = query(0x0150_e828);
            let unload = query(0xd22b_dd7e);
            let display = query(0x35c2_9134);
            let get = query(0x0e45_002d);
            let set = query(0x4a82_c2b1);
            if [init, unload, display, get, set]
                .iter()
                .any(|value| value.is_null())
            {
                return None;
            }
            (
                std::mem::transmute::<*const c_void, NvInit>(init),
                std::mem::transmute::<*const c_void, NvInit>(unload),
                std::mem::transmute::<*const c_void, NvDisplay>(display),
                std::mem::transmute::<*const c_void, NvGet>(get),
                std::mem::transmute::<*const c_void, NvSet>(set),
            )
        };
        // SAFETY: the initializer was resolved with its exact ABI and the DLL is live.
        if unsafe { initialize() } != 0 {
            return None;
        }
        Some(Self {
            _library: library,
            unload,
            display,
            get,
            set,
        })
    }

    fn display_handle(&self, name: &str) -> Result<*mut c_void, PlatformError> {
        let name = CString::new(name).map_err(|_| PlatformError::InvalidValue("display name"))?;
        let mut handle = ptr::null_mut();
        // SAFETY: name is NUL terminated and handle is a writable output pointer.
        let status = unsafe { (self.display)(name.as_ptr(), &mut handle) };
        driver_result("NVIDIA", "display lookup", status)?;
        if handle.is_null() {
            return Err(unavailable("NVIDIA"));
        }
        Ok(handle)
    }

    fn info(&self, name: &str) -> Result<NvDvcInfo, PlatformError> {
        let handle = self.display_handle(name)?;
        let mut info = NvDvcInfo {
            version: 20 | (1 << 16),
            current: 0,
            min: 0,
            max: 0,
            default: 0,
        };
        // SAFETY: the driver owns handle and info is the 20-byte version-1 DVC EX structure.
        let status = unsafe { (self.get)(handle, 0, &mut info) };
        driver_result("NVIDIA", "read digital vibrance", status)?;
        if info.min >= info.max
            || !(info.min..=info.max).contains(&info.default)
            || !(info.min..=info.max).contains(&info.current)
        {
            return Err(unavailable("NVIDIA"));
        }
        Ok(info)
    }

    fn set(&self, name: &str, mut info: NvDvcInfo, value: i32) -> Result<(), PlatformError> {
        let handle = self.display_handle(name)?;
        info.current = value;
        // SAFETY: handle was resolved for this call and info retains its validated ABI version.
        driver_result("NVIDIA", "set digital vibrance", unsafe {
            (self.set)(handle, 0, &info)
        })
    }
}

impl Drop for Nvidia {
    fn drop(&mut self) {
        // SAFETY: initialization succeeded and the DLL remains loaded during Drop.
        unsafe {
            (self.unload)();
        }
    }
}

// ADL2 functions are cdecl; AMD's allocator callback alone uses stdcall on x86.
// See GPUOpen-LibrariesAndSDKs/display-library/Sample/EDIDSampleTool/EDID.cpp.
type AdlAllocate = unsafe extern "system" fn(i32) -> *mut c_void;
type AdlCreate = unsafe extern "C" fn(AdlAllocate, i32, *mut *mut c_void) -> i32;
type AdlDestroy = unsafe extern "C" fn(*mut c_void) -> i32;
type AdlCount = unsafe extern "C" fn(*mut c_void, *mut i32) -> i32;
type AdlAdapters = unsafe extern "C" fn(*mut c_void, *mut AdapterInfo, i32) -> i32;
type AdlDisplays =
    unsafe extern "C" fn(*mut c_void, i32, *mut i32, *mut *mut AdlDisplayInfo, i32) -> i32;
type AdlGet = unsafe extern "C" fn(
    *mut c_void,
    i32,
    i32,
    i32,
    *mut i32,
    *mut i32,
    *mut i32,
    *mut i32,
    *mut i32,
) -> i32;
type AdlSet = unsafe extern "C" fn(*mut c_void, i32, i32, i32, i32) -> i32;

#[repr(C)]
#[derive(Clone, Copy)]
struct AdapterInfo {
    size: i32,
    index: i32,
    udid: [u8; 256],
    bus: i32,
    device: i32,
    function: i32,
    vendor: i32,
    adapter_name: [u8; 256],
    display_name: [u8; 256],
    present: i32,
    exists: i32,
    driver_path: [u8; 256],
    driver_path_ext: [u8; 256],
    pnp: [u8; 256],
    os_display_index: i32,
}

#[repr(C)]
struct AdlDisplayInfo {
    logical_display: i32,
    physical_display: i32,
    logical_adapter: i32,
    physical_adapter: i32,
    controller: i32,
    name: [u8; 256],
    manufacturer: [u8; 256],
    display_type: i32,
    output_type: i32,
    connector: i32,
    mask: i32,
    flags: i32,
}

/// AMD saturation bounds, step, default, and the original captured value.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct ColorRange {
    current: i32,
    default: i32,
    min: i32,
    max: i32,
    step: i32,
}

struct AmdDisplay {
    adapter: i32,
    display: i32,
    range: ColorRange,
}

enum ColorValue {
    Percent(f64),
    Native(i32),
}

struct Amd {
    _library: Library,
    // ADL contexts are opaque tokens. Keeping the address as an integer allows
    // ownership to move between engine threads; every operation is serialized.
    context: usize,
    destroy: AdlDestroy,
    count: AdlCount,
    adapters: AdlAdapters,
    displays: AdlDisplays,
    get: AdlGet,
    set: AdlSet,
}

impl Amd {
    fn load() -> Option<Self> {
        let library = load_driver("atiadlxx.dll").or_else(|| load_driver("atiadlxy.dll"))?;
        // SAFETY: these signatures match AMD's published ADL2 headers. Each
        // function pointer is retained only while its library is owned by Self.
        let (create, destroy, count, adapters, displays, get, set) = unsafe {
            (
                *library
                    .get::<AdlCreate>(b"ADL2_Main_Control_Create\0")
                    .ok()?,
                *library
                    .get::<AdlDestroy>(b"ADL2_Main_Control_Destroy\0")
                    .ok()?,
                *library
                    .get::<AdlCount>(b"ADL2_Adapter_NumberOfAdapters_Get\0")
                    .ok()?,
                *library
                    .get::<AdlAdapters>(b"ADL2_Adapter_AdapterInfo_Get\0")
                    .ok()?,
                *library
                    .get::<AdlDisplays>(b"ADL2_Display_DisplayInfo_Get\0")
                    .ok()?,
                *library.get::<AdlGet>(b"ADL2_Display_Color_Get\0").ok()?,
                *library.get::<AdlSet>(b"ADL2_Display_Color_Set\0").ok()?,
            )
        };
        let mut context = ptr::null_mut();
        // SAFETY: the allocator uses the process heap and the context output is writable.
        if unsafe { create(adl_allocate, 1, &mut context) } != 0 || context.is_null() {
            return None;
        }
        Some(Self {
            _library: library,
            context: context as usize,
            destroy,
            count,
            adapters,
            displays,
            get,
            set,
        })
    }

    fn capture(&self, name: &str) -> Option<AmdDisplay> {
        let mut count = 0;
        // SAFETY: context is live and count is a writable integer output.
        if unsafe { (self.count)(self.context as *mut c_void, &mut count) } != 0
            || !(1..=256).contains(&count)
        {
            return None;
        }
        // SAFETY: AdapterInfo contains only integers and fixed byte arrays, all valid when zeroed.
        let empty: AdapterInfo = unsafe { std::mem::zeroed() };
        let mut adapters = vec![empty; usize::try_from(count).ok()?];
        for adapter in &mut adapters {
            adapter.size = i32::try_from(size_of::<AdapterInfo>()).ok()?;
        }
        let bytes = i32::try_from(adapters.len().checked_mul(size_of::<AdapterInfo>())?).ok()?;
        // SAFETY: the buffer contains exactly bytes writable bytes of ADL AdapterInfo structures.
        if unsafe { (self.adapters)(self.context as *mut c_void, adapters.as_mut_ptr(), bytes) }
            != 0
        {
            return None;
        }
        for adapter in adapters {
            let end = adapter
                .display_name
                .iter()
                .position(|byte| *byte == 0)
                .unwrap_or(adapter.display_name.len());
            if adapter.present == 0
                || !String::from_utf8_lossy(&adapter.display_name[..end]).eq_ignore_ascii_case(name)
            {
                continue;
            }
            let mut count = 0;
            let mut displays = ptr::null_mut();
            // SAFETY: ADL allocates the output with adl_allocate; HeapAllocation
            // below takes ownership immediately and frees even on invalid counts.
            let status = unsafe {
                (self.displays)(
                    self.context as *mut c_void,
                    adapter.index,
                    &mut count,
                    &mut displays,
                    1,
                )
            };
            let _allocation = HeapAllocation(displays.cast());
            if status != 0 || displays.is_null() || !(1..=256).contains(&count) {
                continue;
            }
            // SAFETY: successful ADL output promises count initialized entries;
            // the bounded count is validated and allocation lives through the loop.
            let displays =
                unsafe { std::slice::from_raw_parts(displays, usize::try_from(count).ok()?) };
            for display in displays {
                if display.logical_adapter != adapter.index || display.flags & 3 != 3 {
                    continue;
                }
                if let Some(range) = self.range(adapter.index, display.logical_display) {
                    return Some(AmdDisplay {
                        adapter: adapter.index,
                        display: display.logical_display,
                        range,
                    });
                }
            }
        }
        None
    }

    fn range(&self, adapter: i32, display: i32) -> Option<ColorRange> {
        let mut range = ColorRange {
            current: 0,
            default: 0,
            min: 0,
            max: 0,
            step: 0,
        };
        // SAFETY: context and display identifiers came from ADL; output fields
        // are disjoint writable integers. 4 is ADL_DISPLAY_COLOR_SATURATION.
        let status = unsafe {
            (self.get)(
                self.context as *mut c_void,
                adapter,
                display,
                4,
                &mut range.current,
                &mut range.default,
                &mut range.min,
                &mut range.max,
                &mut range.step,
            )
        };
        if status != 0
            || range.min >= range.max
            || !(range.min..=range.max).contains(&range.default)
            || !(range.min..=range.max).contains(&range.current)
        {
            return None;
        }
        Some(range)
    }

    fn set(&self, name: &str, value: ColorValue) -> Result<(), PlatformError> {
        // ADL logical indices can change after reconnecting a monitor. Resolve
        // its GDI name on every write; only the baseline value survives refresh.
        let target = self.capture(name).ok_or_else(|| unavailable("AMD"))?;
        let value = match value {
            ColorValue::Percent(percent) => driver_level(
                percent,
                target.range.min,
                target.range.default,
                target.range.max,
                target.range.step,
            ),
            ColorValue::Native(value) => value,
        };
        if !(target.range.min..=target.range.max).contains(&value) {
            return Err(PlatformError::Unavailable(format!(
                "the original AMD saturation value is outside the current driver range for {name}"
            )));
        }
        // SAFETY: ADL owns the live context; identifiers and bounds were resolved
        // immediately above for this display name, not retained across reconnects.
        driver_result("AMD", "set saturation", unsafe {
            (self.set)(
                self.context as *mut c_void,
                target.adapter,
                target.display,
                4,
                value,
            )
        })
    }
}

impl Drop for Amd {
    fn drop(&mut self) {
        // SAFETY: this uniquely owned context was created successfully and its DLL is live.
        unsafe {
            (self.destroy)(self.context as *mut c_void);
        }
    }
}

unsafe extern "system" fn adl_allocate(bytes: i32) -> *mut c_void {
    let Ok(bytes) = usize::try_from(bytes) else {
        return ptr::null_mut();
    };
    if bytes == 0 {
        return ptr::null_mut();
    }
    // SAFETY: GetProcessHeap returns the process's live heap; HeapAlloc checks
    // allocation failure and returns a pointer suitable for any ADL structure.
    unsafe { HeapAlloc(GetProcessHeap(), 0, bytes) }
}

struct HeapAllocation(*mut c_void);

impl Drop for HeapAllocation {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: ADL obtained this allocation from adl_allocate and transfers
            // ownership to the caller, which frees once with the same heap.
            unsafe {
                HeapFree(GetProcessHeap(), 0, self.0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockAmd {
        adapter: i32,
        display: i32,
        value: i32,
        connected: bool,
        writes: Vec<(i32, i32, i32)>,
    }

    unsafe extern "C" fn mock_destroy(_: *mut c_void) -> i32 {
        0
    }

    unsafe extern "C" fn mock_count(_: *mut c_void, count: *mut i32) -> i32 {
        // SAFETY: the tested wrapper passes a writable i32 output.
        unsafe {
            count.write(1);
        }
        0
    }

    unsafe extern "C" fn mock_adapters(
        context: *mut c_void,
        output: *mut AdapterInfo,
        bytes: i32,
    ) -> i32 {
        if bytes != 1572 {
            return -1;
        }
        // SAFETY: the test keeps MockAmd alive until the driver is dropped; the
        // wrapper allocated one correctly sized AdapterInfo output above.
        unsafe {
            let context = &*(context.cast::<MockAmd>());
            let mut adapter: AdapterInfo = std::mem::zeroed();
            adapter.size = bytes;
            adapter.index = context.adapter;
            adapter.present = i32::from(context.connected);
            let name = br"\\.\DISPLAY2";
            adapter.display_name[..name.len()].copy_from_slice(name);
            output.write(adapter);
        }
        0
    }

    unsafe extern "C" fn mock_displays(
        context: *mut c_void,
        adapter: i32,
        count: *mut i32,
        output: *mut *mut AdlDisplayInfo,
        _: i32,
    ) -> i32 {
        // SAFETY: context is the test's live MockAmd and the wrapper provides
        // writable outputs; the returned buffer uses the same heap as production.
        unsafe {
            let context = &*(context.cast::<MockAmd>());
            if context.adapter != adapter {
                return -1;
            }
            let allocation = adl_allocate(552).cast::<AdlDisplayInfo>();
            if allocation.is_null() {
                return -1;
            }
            let mut display: AdlDisplayInfo = std::mem::zeroed();
            display.logical_adapter = context.adapter;
            display.logical_display = context.display;
            display.flags = 3;
            allocation.write(display);
            count.write(1);
            output.write(allocation);
        }
        0
    }

    unsafe extern "C" fn mock_get(
        context: *mut c_void,
        adapter: i32,
        display: i32,
        _: i32,
        current: *mut i32,
        neutral: *mut i32,
        min: *mut i32,
        max: *mut i32,
        step: *mut i32,
    ) -> i32 {
        // SAFETY: the wrapper supplies disjoint i32 outputs and the test-owned
        // context remains live; this shim never invokes a physical driver.
        unsafe {
            let context = &*(context.cast::<MockAmd>());
            if context.adapter != adapter || context.display != display {
                return -1;
            }
            current.write(context.value);
            neutral.write(100);
            min.write(0);
            max.write(300);
            step.write(1);
        }
        0
    }

    unsafe extern "C" fn mock_set(
        context: *mut c_void,
        adapter: i32,
        display: i32,
        _: i32,
        value: i32,
    ) -> i32 {
        // SAFETY: the test calls this shim serially with a uniquely owned, live
        // context. Allocation failure aborts rather than unwinding through C.
        unsafe {
            let context = &mut *(context.cast::<MockAmd>());
            if context.adapter != adapter || context.display != display {
                return -1;
            }
            context.value = value;
            context.writes.push((adapter, display, value));
        }
        0
    }

    #[test]
    fn amd_re_resolves_indices_before_apply_and_restore_after_reconnect() {
        let mut context = Box::new(MockAmd {
            adapter: 3,
            display: 7,
            value: 115,
            connected: true,
            writes: Vec::new(),
        });
        let driver = Amd {
            // This inert library fills the ownership field; every callable API
            // points to the in-memory shims, so this test cannot change a display.
            _library: load_driver("kernel32.dll").unwrap(),
            context: (&mut *context as *mut MockAmd) as usize,
            destroy: mock_destroy,
            count: mock_count,
            adapters: mock_adapters,
            displays: mock_displays,
            get: mock_get,
            set: mock_set,
        };
        let vendors = Vendors {
            nvidia: None,
            amd: Some(driver),
        };
        let target = vendors.capture(r"\\.\DISPLAY2").unwrap();
        context.adapter = 23;
        context.display = 91;
        vendors.apply(r"\\.\DISPLAY2", &target, 75.0).unwrap();
        assert_eq!(context.writes.last(), Some(&(23, 91, 200)));
        context.adapter = 33;
        context.display = 101;
        vendors.restore(r"\\.\DISPLAY2", &target).unwrap();
        assert_eq!(context.writes.last(), Some(&(33, 101, 115)));
        context.connected = false;
        assert!(vendors.apply(r"\\.\DISPLAY2", &target, 75.0).is_err());
        assert_eq!(context.writes.len(), 2);
        drop(vendors);
    }

    #[test]
    fn driver_structures_match_published_windows_abis() {
        assert_eq!(size_of::<NvDvcInfo>(), 20);
        assert_eq!(size_of::<AdapterInfo>(), 1572);
        assert_eq!(size_of::<AdlDisplayInfo>(), 552);
    }
}
