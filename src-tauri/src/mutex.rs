//! Windows named mutex — second launch focuses the first window and exits.

use std::sync::Mutex;

use windows::core::w;
use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE, HWND};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowW, IsIconic, SetForegroundWindow, ShowWindow, SW_RESTORE,
};

const MUTEX_NAME: windows::core::PCWSTR = w!("Local\\nocheater.desktop.single-instance");
const WINDOW_TITLE: windows::core::PCWSTR = w!("nocheater NFA Loader");

/// Win32 HANDLE is a raw pointer; ownership stays on this process only.
struct OwnedMutexHandle(HANDLE);
unsafe impl Send for OwnedMutexHandle {}

static OWNED_MUTEX: Mutex<Option<OwnedMutexHandle>> = Mutex::new(None);

/// Returns `true` if this process owns the mutex (first instance).
/// Returns `false` after focusing the existing window (caller should exit).
pub fn ensure_single_instance() -> bool {
    unsafe {
        let handle: HANDLE = match CreateMutexW(None, true, MUTEX_NAME) {
            Ok(h) => h,
            Err(_) => return true,
        };

        if GetLastError() == ERROR_ALREADY_EXISTS {
            let _ = CloseHandle(handle);
            focus_existing();
            return false;
        }

        if let Ok(mut slot) = OWNED_MUTEX.lock() {
            *slot = Some(OwnedMutexHandle(handle));
        }
        true
    }
}

/// Release the single-instance mutex so another process (e.g. elevated relaunch) can take it.
pub fn release() {
    if let Ok(mut slot) = OWNED_MUTEX.lock() {
        if let Some(OwnedMutexHandle(handle)) = slot.take() {
            unsafe {
                let _ = CloseHandle(handle);
            }
        }
    }
}

/// Re-take the mutex after a failed elevation attempt. Returns false if another instance owns it.
pub fn try_reacquire() -> bool {
    unsafe {
        let handle: HANDLE = match CreateMutexW(None, true, MUTEX_NAME) {
            Ok(h) => h,
            Err(_) => return false,
        };

        if GetLastError() == ERROR_ALREADY_EXISTS {
            let _ = CloseHandle(handle);
            return false;
        }

        if let Ok(mut slot) = OWNED_MUTEX.lock() {
            *slot = Some(OwnedMutexHandle(handle));
        }
        true
    }
}

fn focus_existing() {
    unsafe {
        let hwnd: HWND = FindWindowW(None, WINDOW_TITLE).unwrap_or_default();
        if hwnd.0.is_null() {
            return;
        }
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        let _ = SetForegroundWindow(hwnd);
    }
}
