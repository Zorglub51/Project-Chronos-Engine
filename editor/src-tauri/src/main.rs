#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Some Linux graphics drivers cannot allocate WebKit's DMA-BUF buffers.
    // Choose its compatible renderer before GTK/WebKit starts any threads,
    // while allowing an explicit environment setting to override the default.
    #[cfg(target_os = "linux")]
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    }
    pce_game_editor::run()
}
