#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Tauri uses WebKitGTK on Linux.  Its accelerated compositor performs an
    // EGL/GBM probe during startup, which can fail before a display exists in
    // AppImages, SSH sessions, and software-rendered desktops.  The launcher
    // is a 2D UI, so use the reliable software path by default.  Both settings
    // remain user-overridable for systems with a verified graphics stack.
    #[cfg(target_os = "linux")]
    {
        if std::env::var_os("WEBKIT_DISABLE_COMPOSITING_MODE").is_none() {
            std::env::set_var("WEBKIT_DISABLE_COMPOSITING_MODE", "1");
        }
        if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
            std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        }
    }
    flint_lib::run();
}
