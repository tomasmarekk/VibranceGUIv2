//! Embeds the Tauri configuration and Windows application resources.
//! The same resources are used by the installer and the standalone executable.
fn main() {
    tauri_build::build();
}
