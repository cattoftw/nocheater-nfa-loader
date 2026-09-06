//! Detect admin elevation and relaunch via ShellExecuteEx `runas` (no console).

use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;
use std::time::Duration;

use tauri::AppHandle;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Shell::{
    IsUserAnAdmin, ShellExecuteExW, SEE_MASK_NO_CONSOLE, SHELLEXECUTEINFOW,
};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

use crate::mutex;

pub fn is_elevated() -> bool {
    unsafe { IsUserAnAdmin().as_bool() }
}

fn wide_nul(path: &PathBuf) -> Vec<u16> {
    path.as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

/// Relaunch this executable elevated via UAC, then exit this instance.
/// Releases the single-instance mutex before `runas` so the new process can start.
pub fn relaunch_as_admin(app: AppHandle) -> Result<(), String> {
    if is_elevated() {
        return Err("Already running as administrator".into());
    }

    let exe = std::env::current_exe().map_err(|e| format!("Could not resolve executable: {e}"))?;
    let exe_wide = wide_nul(&exe);

    // Drop mutex before UAC so the elevated child can acquire it on start.
    mutex::release();

    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        // Prevent the elevated GUI process from inheriting / showing a console.
        fMask: SEE_MASK_NO_CONSOLE,
        hwnd: HWND::default(),
        lpVerb: w!("runas"),
        lpFile: PCWSTR(exe_wide.as_ptr()),
        lpParameters: PCWSTR::null(),
        lpDirectory: PCWSTR::null(),
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };

    let launched = unsafe { ShellExecuteExW(&mut info) };
    if launched.is_err() {
        let _ = mutex::try_reacquire();
        return Err("Elevation cancelled".into());
    }

    // Give the elevated process a moment to start, then quit this instance.
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(400));
        app.exit(0);
    });

    Ok(())
}
