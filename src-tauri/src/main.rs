//! Windows application entry point; native lifecycle is owned by the library.
//! Release builds avoid opening a console alongside the desktop window.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() -> std::process::ExitCode {
    if let Err(error) = vibrance_gui_v2::run() {
        tracing::error!(%error, "application terminated");
        eprintln!("VibranceGUI v2: {error:#}");
        #[cfg(windows)]
        if !std::env::args().any(|arg| arg == "--diagnostics") {
            let title: Vec<_> = "VibranceGUI v2\0".encode_utf16().collect();
            let message: Vec<_> = format!("Unable to start VibranceGUI v2.\n\n{error:#}\0")
                .encode_utf16()
                .collect();
            // SAFETY: both buffers are NUL-terminated and remain alive until the modal returns.
            unsafe {
                windows_sys::Win32::UI::WindowsAndMessaging::MessageBoxW(
                    std::ptr::null_mut(),
                    message.as_ptr(),
                    title.as_ptr(),
                    windows_sys::Win32::UI::WindowsAndMessaging::MB_OK
                        | windows_sys::Win32::UI::WindowsAndMessaging::MB_ICONERROR,
                );
            }
        }
        return std::process::ExitCode::FAILURE;
    }
    std::process::ExitCode::SUCCESS
}
