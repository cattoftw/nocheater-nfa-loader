use tauri::{AppHandle, Manager};

/// No system tray — close / OS close quits the app fully.
pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    if let Some(window) = app.get_webview_window("main") {
        let handle = app.clone();
        window.on_window_event(move |event| {
            if let tauri::WindowEvent::CloseRequested { .. } = event {
                handle.exit(0);
            }
        });
    }
    Ok(())
}
