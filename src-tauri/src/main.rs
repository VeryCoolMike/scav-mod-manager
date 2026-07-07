// Prevents an additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Work around a common WebKitGTK bug where the window renders solid
    // white (usually the DMA-BUF renderer misbehaving with certain GPU
    // drivers) - most prevalent in AppImage builds. Must be set before the
    // webview initializes.
    #[cfg(target_os = "linux")]
    {
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        std::env::set_var("WEBKIT_DISABLE_COMPOSITING_MODE", "1");
    }
    scav_mod_manager_lib::run()
}
