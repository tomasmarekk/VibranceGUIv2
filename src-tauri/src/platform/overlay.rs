//! Color-rule overlay for program profiles.
//! Driver gamma ramps change each channel on its own and cannot single out one color,
//! so matching pixels are redrawn instead: DXGI Desktop Duplication captures the
//! display, `overlay.hlsl` recolors only matching pixels, and a click-through window
//! excluded from capture composites them above the game. Nothing is injected into the
//! game; untouched pixels stay transparent, so the rest of the picture has no added delay.

use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use windows::{
    Win32::{
        Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::{
            Direct3D::{
                D3D_DRIVER_TYPE_UNKNOWN, D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST, Fxc::D3DCompile,
            },
            Direct3D11::{
                D3D11_BIND_CONSTANT_BUFFER, D3D11_BIND_SHADER_RESOURCE, D3D11_BUFFER_DESC,
                D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC,
                D3D11_USAGE_DEFAULT, D3D11_VIEWPORT, D3D11CreateDevice, ID3D11Buffer, ID3D11Device,
                ID3D11DeviceContext, ID3D11PixelShader, ID3D11RenderTargetView,
                ID3D11ShaderResourceView, ID3D11Texture2D, ID3D11VertexShader,
            },
            DirectComposition::{
                DCompositionCreateDevice, IDCompositionDevice, IDCompositionTarget,
                IDCompositionVisual,
            },
            Dxgi::{
                Common::{
                    DXGI_ALPHA_MODE_PREMULTIPLIED, DXGI_FORMAT_B8G8R8A8_UNORM,
                    DXGI_MODE_ROTATION_IDENTITY, DXGI_MODE_ROTATION_UNSPECIFIED, DXGI_SAMPLE_DESC,
                },
                CreateDXGIFactory1, DXGI_ERROR_ACCESS_LOST, DXGI_ERROR_WAIT_TIMEOUT,
                DXGI_OUTDUPL_FRAME_INFO, DXGI_PRESENT, DXGI_SCALING_STRETCH, DXGI_SWAP_CHAIN_DESC1,
                DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL, DXGI_USAGE_RENDER_TARGET_OUTPUT, IDXGIAdapter1,
                IDXGIDevice, IDXGIFactory1, IDXGIFactory2, IDXGIOutput1, IDXGIOutputDuplication,
                IDXGISwapChain1,
            },
        },
        System::LibraryLoader::GetModuleHandleW,
        UI::WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, HTTRANSPARENT,
            HWND_TOPMOST, LWA_ALPHA, MSG, PM_REMOVE, PeekMessageW, RegisterClassW,
            SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOOWNERZORDER, SWP_NOSIZE,
            SetLayeredWindowAttributes, SetWindowDisplayAffinity, SetWindowPos, ShowWindow,
            TranslateMessage, WDA_EXCLUDEFROMCAPTURE, WM_NCHITTEST, WNDCLASSW, WS_EX_LAYERED,
            WS_EX_NOACTIVATE, WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
            WS_EX_TRANSPARENT, WS_POPUP,
        },
    },
    core::{Interface, PCSTR, s, w},
};

use crate::{
    color_match::{RuleParams, rule_params},
    settings::{ColorRule, MAX_COLOR_RULES},
};

const SHADER_SOURCE: &str = include_str!("overlay.hlsl");
/// How long one capture wait may block, so stop requests and rule edits stay responsive.
const FRAME_WAIT_MS: u32 = 50;
/// Delay before rebuilding after a failure such as a lost GPU device.
const RETRY_DELAY: Duration = Duration::from_secs(2);

#[derive(Debug, thiserror::Error)]
enum OverlayError {
    #[error("{context} failed: {source}")]
    Windows {
        context: &'static str,
        #[source]
        source: windows::core::Error,
    },
    #[error("{0}")]
    Unsupported(String),
    /// The display changed size or format; rebuild silently.
    #[error("the display changed")]
    Rebuild,
}

type OverlayResult<T> = Result<T, OverlayError>;

trait Context<T> {
    fn context(self, context: &'static str) -> OverlayResult<T>;
}

impl<T> Context<T> for windows::core::Result<T> {
    fn context(self, context: &'static str) -> OverlayResult<T> {
        self.map_err(|source| OverlayError::Windows { context, source })
    }
}

struct RuleSet {
    generation: u64,
    rules: Vec<ColorRule>,
}

struct Shared {
    stop: AtomicBool,
    rules: Mutex<RuleSet>,
    error: Mutex<Option<String>>,
}

/// Owns the overlay thread for one display; dropping it removes the overlay.
pub(crate) struct ColorOverlay {
    display_id: String,
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl ColorOverlay {
    /// Starts drawing `rules` over `display_id`. Setup runs on the overlay thread, so
    /// failures are reported through [`ColorOverlay::error`] rather than returned.
    pub(crate) fn start(display_id: &str, rules: &[ColorRule]) -> Self {
        let shared = Arc::new(Shared {
            stop: AtomicBool::new(false),
            rules: Mutex::new(RuleSet {
                generation: 1,
                rules: rules.to_vec(),
            }),
            error: Mutex::new(None),
        });
        let thread = thread::Builder::new().name("color-overlay".into()).spawn({
            let shared = Arc::clone(&shared);
            let display_id = display_id.to_owned();
            move || run(&display_id, &shared)
        });
        let thread = match thread {
            Ok(thread) => Some(thread),
            Err(error) => {
                *shared.error.lock().expect("overlay error mutex poisoned") =
                    Some(format!("Color equalizer could not start: {error}"));
                None
            }
        };
        Self {
            display_id: display_id.to_owned(),
            shared,
            thread,
        }
    }

    /// The display this overlay covers.
    pub(crate) fn display_id(&self) -> &str {
        &self.display_id
    }

    /// Replaces the drawn rules; unchanged rules cost nothing.
    pub(crate) fn set_rules(&self, rules: &[ColorRule]) {
        let mut current = self
            .shared
            .rules
            .lock()
            .expect("overlay rules mutex poisoned");
        if current.rules != rules {
            current.rules = rules.to_vec();
            current.generation += 1;
        }
    }

    /// The latest setup or capture failure, cleared once drawing works again.
    pub(crate) fn error(&self) -> Option<String> {
        self.shared
            .error
            .lock()
            .expect("overlay error mutex poisoned")
            .clone()
    }
}

impl Drop for ColorOverlay {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take()
            && thread.join().is_err()
        {
            tracing::error!("color overlay thread panicked");
        }
    }
}

fn run(display_id: &str, shared: &Shared) {
    while !shared.stop.load(Ordering::Relaxed) {
        let result = Overlay::create(display_id).and_then(|mut overlay| overlay.run(shared));
        let report = |message: Option<String>| {
            *shared.error.lock().expect("overlay error mutex poisoned") = message;
        };
        match result {
            Ok(()) => return,
            Err(OverlayError::Rebuild) => continue,
            Err(error) => {
                tracing::warn!(%error, display_id, "color overlay stopped");
                report(Some(format!("Color equalizer: {error}")));
                let retry_at = Instant::now() + RETRY_DELAY;
                while Instant::now() < retry_at && !shared.stop.load(Ordering::Relaxed) {
                    thread::sleep(Duration::from_millis(50));
                }
            }
        }
    }
}

/// Shader constants; the layout matches the `Rules` cbuffer in `overlay.hlsl`.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct RuleConstants {
    source: [[f32; 4]; MAX_COLOR_RULES],
    target: [[f32; 4]; MAX_COLOR_RULES],
    params: [[f32; 4]; MAX_COLOR_RULES],
    count: [u32; 4],
}

impl RuleConstants {
    fn new(rules: &[RuleParams]) -> Self {
        let mut constants = Self::default();
        let used = rules.len().min(MAX_COLOR_RULES);
        // Shader math is single precision; narrowing is intended.
        let narrow = |values: [f64; 3]| values.map(|value| value as f32);
        for (index, rule) in rules.iter().take(used).enumerate() {
            let [l, a, b] = narrow(rule.source);
            constants.source[index] = [l, a, b, 0.0];
            let [l, a, b] = narrow(rule.target);
            constants.target[index] = [l, a, b, 0.0];
            constants.params[index] = [
                rule.radius as f32,
                rule.strength as f32,
                rule.chroma as f32,
                rule.lightness as f32,
            ];
        }
        constants.count[0] = u32::try_from(used).unwrap_or(0);
        constants
    }
}

fn compile(entry: PCSTR, target: PCSTR) -> OverlayResult<Vec<u8>> {
    let mut code = None;
    let mut errors = None;
    // SAFETY: the source pointer and length describe a live UTF-8 string, entry and
    // target are NUL-terminated literals, and both output slots are writable.
    let result = unsafe {
        D3DCompile(
            SHADER_SOURCE.as_ptr().cast(),
            SHADER_SOURCE.len(),
            s!("overlay.hlsl"),
            None,
            None,
            entry,
            target,
            0,
            0,
            &mut code,
            Some(&mut errors),
        )
    };
    let blob_bytes = |blob: &windows::Win32::Graphics::Direct3D::ID3DBlob| {
        // SAFETY: a blob's pointer is valid for its reported size while the blob lives.
        unsafe {
            std::slice::from_raw_parts(blob.GetBufferPointer().cast::<u8>(), blob.GetBufferSize())
                .to_vec()
        }
    };
    match (result, code) {
        (Ok(()), Some(code)) => Ok(blob_bytes(&code)),
        (result, _) => {
            let detail = errors
                .as_ref()
                .map(|blob| String::from_utf8_lossy(&blob_bytes(blob)).into_owned())
                .unwrap_or_default();
            Err(OverlayError::Unsupported(format!(
                "the color shader did not compile ({:?}): {detail}",
                result.err()
            )))
        }
    }
}

/// The color-rule draw call, shared by the live overlay and the shader tests.
struct Renderer {
    vertex: ID3D11VertexShader,
    pixel: ID3D11PixelShader,
    constants: ID3D11Buffer,
}

impl Renderer {
    fn new(device: &ID3D11Device) -> OverlayResult<Self> {
        let vertex_code = compile(s!("VSMain"), s!("vs_4_0"))?;
        let pixel_code = compile(s!("PSMain"), s!("ps_4_0"))?;
        let mut vertex = None;
        let mut pixel = None;
        let mut constants = None;
        let buffer = D3D11_BUFFER_DESC {
            ByteWidth: u32::try_from(size_of::<RuleConstants>()).unwrap_or(u32::MAX),
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_CONSTANT_BUFFER.0.cast_unsigned(),
            ..Default::default()
        };
        // SAFETY: bytecode slices come from the compiler and the descriptor matches
        // RuleConstants, whose size is a multiple of 16 bytes as cbuffers require.
        unsafe {
            device
                .CreateVertexShader(&vertex_code, None, Some(&mut vertex))
                .context("creating the vertex shader")?;
            device
                .CreatePixelShader(&pixel_code, None, Some(&mut pixel))
                .context("creating the pixel shader")?;
            device
                .CreateBuffer(&buffer, None, Some(&mut constants))
                .context("creating the rule buffer")?;
        }
        let missing = || OverlayError::Unsupported("the GPU returned no shader object".into());
        Ok(Self {
            vertex: vertex.ok_or_else(missing)?,
            pixel: pixel.ok_or_else(missing)?,
            constants: constants.ok_or_else(missing)?,
        })
    }

    fn set_rules(&self, context: &ID3D11DeviceContext, rules: &[RuleParams]) {
        let constants = RuleConstants::new(rules);
        // SAFETY: the buffer was created with exactly size_of::<RuleConstants>() bytes.
        unsafe {
            context.UpdateSubresource(
                &self.constants,
                0,
                None,
                (&raw const constants).cast(),
                0,
                0,
            );
        }
    }

    fn draw(
        &self,
        context: &ID3D11DeviceContext,
        source: &ID3D11ShaderResourceView,
        target: &ID3D11RenderTargetView,
        (width, height): (u32, u32),
    ) {
        let viewport = D3D11_VIEWPORT {
            Width: width as f32,
            Height: height as f32,
            MaxDepth: 1.0,
            ..Default::default()
        };
        // SAFETY: all bound objects belong to the context's device and outlive the call.
        unsafe {
            context.ClearRenderTargetView(target, &[0.0; 4]);
            context.OMSetRenderTargets(Some(&[Some(target.clone())]), None);
            context.RSSetViewports(Some(&[viewport]));
            context.IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            context.VSSetShader(&self.vertex, None);
            context.PSSetShader(&self.pixel, None);
            context.PSSetShaderResources(0, Some(&[Some(source.clone())]));
            context.PSSetConstantBuffers(0, Some(&[Some(self.constants.clone())]));
            context.Draw(3, 0);
            // Unbind the capture copy so the next CopyResource has no read hazard.
            context.PSSetShaderResources(0, Some(&[None]));
        }
    }
}

const WINDOW_CLASS: windows::core::PCWSTR = w!("VibranceGuiColorOverlay");

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_NCHITTEST {
        // Clicks fall through to whatever is underneath, normally the game.
        return LRESULT(HTTRANSPARENT as isize);
    }
    // SAFETY: forwards the unmodified message for a window this module created.
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}

/// Topmost, click-through window that never activates and is hidden from capture,
/// which also keeps Desktop Duplication from reading back its own output.
struct OverlayWindow(HWND);

impl OverlayWindow {
    fn create(rect: RECT) -> OverlayResult<Self> {
        // SAFETY: the class name and procedure are 'static; a repeated registration
        // fails harmlessly with ERROR_CLASS_ALREADY_EXISTS. Every handle passed below
        // was just returned by the system.
        unsafe {
            let instance = GetModuleHandleW(None).context("reading the module handle")?;
            let class = WNDCLASSW {
                lpfnWndProc: Some(window_proc),
                hInstance: instance.into(),
                lpszClassName: WINDOW_CLASS,
                ..Default::default()
            };
            RegisterClassW(&raw const class);
            let window = CreateWindowExW(
                WS_EX_NOREDIRECTIONBITMAP
                    | WS_EX_TOPMOST
                    | WS_EX_TOOLWINDOW
                    | WS_EX_NOACTIVATE
                    | WS_EX_TRANSPARENT
                    | WS_EX_LAYERED,
                WINDOW_CLASS,
                w!("VibranceGUI color overlay"),
                WS_POPUP,
                rect.left,
                rect.top,
                rect.right - rect.left,
                rect.bottom - rect.top,
                None,
                None,
                Some(instance.into()),
                None,
            )
            .context("creating the overlay window")?;
            let overlay = Self(window);
            SetLayeredWindowAttributes(window, COLORREF(0), 255, LWA_ALPHA)
                .context("preparing the overlay window")?;
            SetWindowDisplayAffinity(window, WDA_EXCLUDEFROMCAPTURE).map_err(|_| {
                OverlayError::Unsupported("Windows 10 version 2004 or newer is required".into())
            })?;
            let _ = ShowWindow(window, SW_SHOWNOACTIVATE);
            Ok(overlay)
        }
    }

    fn keep_on_top(&self) {
        // SAFETY: the handle is owned by this value and still alive.
        let result = unsafe {
            SetWindowPos(
                self.0,
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER,
            )
        };
        if let Err(error) = result {
            tracing::debug!(%error, "failed to keep the color overlay on top");
        }
    }
}

impl Drop for OverlayWindow {
    fn drop(&mut self) {
        // SAFETY: the window was created by this thread and is destroyed exactly once.
        if let Err(error) = unsafe { DestroyWindow(self.0) } {
            tracing::debug!(%error, "failed to destroy the color overlay window");
        }
    }
}

fn pump_messages() {
    let mut message = MSG::default();
    // SAFETY: `message` is a writable MSG; only this thread's windows are processed.
    unsafe {
        while PeekMessageW(&raw mut message, None, 0, 0, PM_REMOVE).as_bool() {
            let _ = TranslateMessage(&raw const message);
            DispatchMessageW(&raw const message);
        }
    }
}

fn find_output(display_id: &str) -> OverlayResult<(IDXGIAdapter1, IDXGIOutput1, RECT)> {
    // SAFETY: plain factory and enumeration calls; descriptors are written by DXGI.
    unsafe {
        let factory: IDXGIFactory1 = CreateDXGIFactory1().context("creating a DXGI factory")?;
        for adapter_index in 0.. {
            let Ok(adapter) = factory.EnumAdapters1(adapter_index) else {
                break;
            };
            for output_index in 0.. {
                let Ok(output) = adapter.EnumOutputs(output_index) else {
                    break;
                };
                let description = output.GetDesc().context("reading a display description")?;
                let length = description
                    .DeviceName
                    .iter()
                    .position(|unit| *unit == 0)
                    .unwrap_or(description.DeviceName.len());
                if !String::from_utf16_lossy(&description.DeviceName[..length])
                    .eq_ignore_ascii_case(display_id)
                {
                    continue;
                }
                if description.Rotation != DXGI_MODE_ROTATION_IDENTITY
                    && description.Rotation != DXGI_MODE_ROTATION_UNSPECIFIED
                {
                    return Err(OverlayError::Unsupported(
                        "rotated displays aren't supported".into(),
                    ));
                }
                let output = output
                    .cast::<IDXGIOutput1>()
                    .context("opening the display")?;
                return Ok((adapter, output, description.DesktopCoordinates));
            }
        }
    }
    Err(OverlayError::Unsupported(format!(
        "{display_id} is not an active display"
    )))
}

struct Capture {
    texture: ID3D11Texture2D,
    view: ID3D11ShaderResourceView,
    size: (u32, u32),
}

enum Frame {
    Updated,
    Unchanged,
    Lost,
}

struct Overlay {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    output: IDXGIOutput1,
    duplication: Option<IDXGIOutputDuplication>,
    swap_chain: IDXGISwapChain1,
    target_view: ID3D11RenderTargetView,
    renderer: Renderer,
    capture: Option<Capture>,
    size: (u32, u32),
    // Composition objects must outlive the window content they present.
    _composition: (
        IDCompositionDevice,
        IDCompositionTarget,
        IDCompositionVisual,
    ),
    window: OverlayWindow,
}

impl Overlay {
    fn create(display_id: &str) -> OverlayResult<Self> {
        let (adapter, output, rect) = find_output(display_id)?;
        let size = (
            u32::try_from(rect.right - rect.left).unwrap_or(0),
            u32::try_from(rect.bottom - rect.top).unwrap_or(0),
        );
        if size.0 == 0 || size.1 == 0 {
            return Err(OverlayError::Unsupported("the display has no size".into()));
        }
        let mut device = None;
        let mut context = None;
        // SAFETY: the adapter is live and the output slots are writable.
        unsafe {
            D3D11CreateDevice(
                &adapter,
                D3D_DRIVER_TYPE_UNKNOWN,
                Default::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )
            .context("creating the GPU device")?;
        }
        let missing = || OverlayError::Unsupported("the GPU returned no device".into());
        let device: ID3D11Device = device.ok_or_else(missing)?;
        let context: ID3D11DeviceContext = context.ok_or_else(missing)?;
        let renderer = Renderer::new(&device)?;
        let window = OverlayWindow::create(rect)?;
        let description = DXGI_SWAP_CHAIN_DESC1 {
            Width: size.0,
            Height: size.1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
            BufferCount: 2,
            Scaling: DXGI_SCALING_STRETCH,
            SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
            AlphaMode: DXGI_ALPHA_MODE_PREMULTIPLIED,
            ..Default::default()
        };
        // SAFETY: every object below is created on this thread from the live device
        // and window, and the descriptor matches the window's pixel size.
        let (swap_chain, target_view, composition) = unsafe {
            let factory: IDXGIFactory2 = adapter.GetParent().context("opening the DXGI factory")?;
            let swap_chain = factory
                .CreateSwapChainForComposition(&device, &raw const description, None)
                .context("creating the overlay surface")?;
            let back_buffer: ID3D11Texture2D = swap_chain
                .GetBuffer(0)
                .context("reading the overlay surface")?;
            let mut target_view = None;
            device
                .CreateRenderTargetView(&back_buffer, None, Some(&mut target_view))
                .context("creating the overlay target")?;
            let dxgi_device: IDXGIDevice = device.cast().context("opening the DXGI device")?;
            let composition: IDCompositionDevice =
                DCompositionCreateDevice(&dxgi_device).context("starting composition")?;
            let target = composition
                .CreateTargetForHwnd(window.0, true)
                .context("attaching the overlay window")?;
            let visual = composition
                .CreateVisual()
                .context("creating the overlay visual")?;
            visual
                .SetContent(&swap_chain)
                .context("showing the overlay surface")?;
            target
                .SetRoot(&visual)
                .context("showing the overlay visual")?;
            composition.Commit().context("committing the overlay")?;
            (
                swap_chain,
                target_view.ok_or_else(missing)?,
                (composition, target, visual),
            )
        };
        Ok(Self {
            device,
            context,
            output,
            duplication: None,
            swap_chain,
            target_view,
            renderer,
            capture: None,
            size,
            _composition: composition,
            window,
        })
    }

    fn run(&mut self, shared: &Shared) -> OverlayResult<()> {
        let mut generation = 0;
        let mut last_raise = Instant::now();
        let mut reported_ok = false;
        while !shared.stop.load(Ordering::Relaxed) {
            pump_messages();
            let mut dirty = false;
            {
                let rules = shared.rules.lock().expect("overlay rules mutex poisoned");
                if rules.generation != generation {
                    generation = rules.generation;
                    self.renderer
                        .set_rules(&self.context, &rule_params(&rules.rules));
                    dirty = true;
                }
            }
            match self.next_frame()? {
                Frame::Updated => dirty = true,
                Frame::Unchanged => {}
                Frame::Lost => {
                    // Show nothing stale while capture is unavailable (for example on
                    // the secure desktop), then try again shortly.
                    self.clear()?;
                    thread::sleep(Duration::from_millis(250));
                }
            }
            if dirty && let Some(capture) = &self.capture {
                self.renderer
                    .draw(&self.context, &capture.view, &self.target_view, self.size);
                self.present()?;
                if !reported_ok {
                    *shared.error.lock().expect("overlay error mutex poisoned") = None;
                    reported_ok = true;
                }
            }
            if last_raise.elapsed() >= Duration::from_secs(1) {
                self.window.keep_on_top();
                last_raise = Instant::now();
            }
        }
        Ok(())
    }

    fn next_frame(&mut self) -> OverlayResult<Frame> {
        if self.duplication.is_none() {
            // SAFETY: the output and device are live; failure is reported, not assumed.
            match unsafe { self.output.DuplicateOutput(&self.device) } {
                Ok(duplication) => self.duplication = Some(duplication),
                Err(error) if error.code() == DXGI_ERROR_ACCESS_LOST => return Ok(Frame::Lost),
                Err(source) => {
                    return Err(OverlayError::Windows {
                        context: "capturing the display",
                        source,
                    });
                }
            }
        }
        let Some(duplication) = self.duplication.clone() else {
            return Ok(Frame::Lost);
        };
        let mut info = DXGI_OUTDUPL_FRAME_INFO::default();
        let mut resource = None;
        // SAFETY: both output pointers are writable and live for the call.
        let acquired = unsafe {
            duplication.AcquireNextFrame(FRAME_WAIT_MS, &raw mut info, &raw mut resource)
        };
        match acquired {
            Ok(()) => {}
            Err(error) if error.code() == DXGI_ERROR_WAIT_TIMEOUT => return Ok(Frame::Unchanged),
            Err(error) if error.code() == DXGI_ERROR_ACCESS_LOST => {
                self.duplication = None;
                return Ok(Frame::Lost);
            }
            Err(source) => {
                return Err(OverlayError::Windows {
                    context: "reading the display",
                    source,
                });
            }
        }
        // Pointer-only updates leave the picture unchanged.
        let copied = if info.LastPresentTime == 0 {
            Ok(false)
        } else {
            resource
                .ok_or_else(|| OverlayError::Unsupported("the capture had no image".into()))
                .and_then(|resource| {
                    resource
                        .cast::<ID3D11Texture2D>()
                        .context("reading the captured image")
                })
                .and_then(|texture| self.copy_capture(&texture).map(|()| true))
        };
        // SAFETY: a frame was acquired above, so exactly one release is required.
        if let Err(error) = unsafe { duplication.ReleaseFrame() } {
            tracing::debug!(%error, "failed to release a captured frame");
        }
        Ok(if copied? {
            Frame::Updated
        } else {
            Frame::Unchanged
        })
    }

    fn copy_capture(&mut self, texture: &ID3D11Texture2D) -> OverlayResult<()> {
        let mut description = D3D11_TEXTURE2D_DESC::default();
        // SAFETY: the descriptor is a writable D3D11_TEXTURE2D_DESC.
        unsafe { texture.GetDesc(&raw mut description) };
        if description.Format != DXGI_FORMAT_B8G8R8A8_UNORM {
            return Err(OverlayError::Unsupported(
                "HDR is on for this display; the color equalizer needs SDR".into(),
            ));
        }
        let size = (description.Width, description.Height);
        if size != self.size {
            return Err(OverlayError::Rebuild);
        }
        if self
            .capture
            .as_ref()
            .is_none_or(|capture| capture.size != size)
        {
            let copy = D3D11_TEXTURE2D_DESC {
                Width: size.0,
                Height: size.1,
                MipLevels: 1,
                ArraySize: 1,
                Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                Usage: D3D11_USAGE_DEFAULT,
                BindFlags: D3D11_BIND_SHADER_RESOURCE.0.cast_unsigned(),
                ..Default::default()
            };
            let mut copy_texture = None;
            let mut view = None;
            // SAFETY: the descriptor is fully initialized and the output slots are writable.
            unsafe {
                self.device
                    .CreateTexture2D(&raw const copy, None, Some(&mut copy_texture))
                    .context("creating the capture copy")?;
                let copy_texture = copy_texture.as_ref().ok_or(OverlayError::Rebuild)?;
                self.device
                    .CreateShaderResourceView(copy_texture, None, Some(&mut view))
                    .context("reading the capture copy")?;
            }
            self.capture = Some(Capture {
                texture: copy_texture.ok_or(OverlayError::Rebuild)?,
                view: view.ok_or(OverlayError::Rebuild)?,
                size,
            });
        }
        if let Some(capture) = &self.capture {
            // SAFETY: both textures share size and format and belong to this device.
            unsafe { self.context.CopyResource(&capture.texture, texture) };
        }
        Ok(())
    }

    fn present(&self) -> OverlayResult<()> {
        // SAFETY: the swap chain is live; flip-model presents never block on interval 0.
        unsafe { self.swap_chain.Present(0, DXGI_PRESENT(0)) }
            .ok()
            .context("showing the overlay")
    }

    fn clear(&self) -> OverlayResult<()> {
        // SAFETY: the render target view belongs to this context's device.
        unsafe {
            self.context
                .ClearRenderTargetView(&self.target_view, &[0.0; 4])
        };
        self.present()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color_match::recolor;
    use windows::Win32::Graphics::{
        Direct3D::D3D_DRIVER_TYPE_WARP,
        Direct3D11::{
            D3D11_BIND_RENDER_TARGET, D3D11_CPU_ACCESS_READ, D3D11_MAP_READ,
            D3D11_MAPPED_SUBRESOURCE, D3D11_SUBRESOURCE_DATA, D3D11_USAGE_STAGING,
        },
    };

    fn rule(source: &str, target: &str) -> ColorRule {
        ColorRule {
            id: source.into(),
            enabled: true,
            source: source.into(),
            target: target.into(),
            tolerance: 35.0,
            strength: 80.0,
            saturation: 20.0,
            brightness: -10.0,
        }
    }

    fn texture(
        device: &ID3D11Device,
        width: u32,
        bind: u32,
        usage: windows::Win32::Graphics::Direct3D11::D3D11_USAGE,
        cpu: u32,
        pixels: Option<&[u8]>,
    ) -> ID3D11Texture2D {
        let description = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: 1,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: usage,
            BindFlags: bind,
            CPUAccessFlags: cpu,
            ..Default::default()
        };
        let initial = pixels.map(|pixels| D3D11_SUBRESOURCE_DATA {
            pSysMem: pixels.as_ptr().cast(),
            SysMemPitch: width * 4,
            SysMemSlicePitch: 0,
        });
        let mut texture = None;
        // SAFETY: the descriptor and optional initial data describe `width` BGRA pixels.
        unsafe {
            device
                .CreateTexture2D(
                    &raw const description,
                    initial.as_ref().map(|data| &raw const *data),
                    Some(&mut texture),
                )
                .unwrap();
        }
        texture.unwrap()
    }

    #[test]
    fn shader_matches_the_cpu_reference() {
        let pixels: [[u8; 3]; 8] = [
            [254, 254, 57],
            [250, 248, 70],
            [230, 228, 80],
            [216, 196, 160],
            [255, 255, 255],
            [0, 0, 0],
            [40, 200, 240],
            [60, 220, 255],
        ];
        let rules = [rule("#FEFE39", "#FF3BD4"), rule("#28C8F0", "#28C8F0")];
        let params = rule_params(&rules);
        let mut device = None;
        let mut context = None;
        // SAFETY: WARP needs no adapter; the output slots are writable.
        unsafe {
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_WARP,
                Default::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )
            .unwrap();
        }
        let (device, context): (ID3D11Device, ID3D11DeviceContext) =
            (device.unwrap(), context.unwrap());
        let width = u32::try_from(pixels.len()).unwrap();
        let bgra: Vec<u8> = pixels
            .iter()
            .flat_map(|[red, green, blue]| [*blue, *green, *red, 255])
            .collect();
        let input = texture(
            &device,
            width,
            D3D11_BIND_SHADER_RESOURCE.0.cast_unsigned(),
            D3D11_USAGE_DEFAULT,
            0,
            Some(&bgra),
        );
        let output = texture(
            &device,
            width,
            D3D11_BIND_RENDER_TARGET.0.cast_unsigned(),
            D3D11_USAGE_DEFAULT,
            0,
            None,
        );
        let staging = texture(
            &device,
            width,
            0,
            D3D11_USAGE_STAGING,
            D3D11_CPU_ACCESS_READ.0.cast_unsigned(),
            None,
        );
        let renderer = Renderer::new(&device).unwrap();
        let mut view = None;
        let mut target = None;
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        // SAFETY: all resources belong to the WARP device; the mapped staging texture
        // holds `width` BGRA pixels and is unmapped before it is released.
        let rendered = unsafe {
            device
                .CreateShaderResourceView(&input, None, Some(&mut view))
                .unwrap();
            device
                .CreateRenderTargetView(&output, None, Some(&mut target))
                .unwrap();
            renderer.set_rules(&context, &params);
            renderer.draw(&context, &view.unwrap(), &target.unwrap(), (width, 1));
            context.CopyResource(&staging, &output);
            context
                .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&raw mut mapped))
                .unwrap();
            let bytes =
                std::slice::from_raw_parts(mapped.pData.cast::<u8>(), pixels.len() * 4).to_vec();
            context.Unmap(&staging, 0);
            bytes
        };
        for (index, rgb) in pixels.iter().enumerate() {
            let (color, opacity) = recolor(*rgb, &params);
            let expected = [
                color[2] * opacity,
                color[1] * opacity,
                color[0] * opacity,
                opacity,
            ]
            .map(|value| value * 255.0);
            let actual = &rendered[index * 4..index * 4 + 4];
            for (channel, (want, got)) in expected.iter().zip(actual).enumerate() {
                assert!(
                    (want - f64::from(*got)).abs() <= 2.0,
                    "pixel {index} {rgb:?} channel {channel}: expected {want:.1}, rendered {got}"
                );
            }
        }
        assert!(rendered[3] > 200, "the exact outline color is recolored");
        assert_eq!(rendered[3 * 4 + 3], 0, "beige scenery stays transparent");
    }
}
