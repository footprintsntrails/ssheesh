//! The folder under the pointer on this computer's screen, outside SSHeesh:
//! the desktop, or a file manager's window. Files dragged out of the window
//! download there, as letting go over a folder means.
//!
//! Windows only so far: elsewhere there's no answer, and the download goes to
//! Downloads.

use std::path::PathBuf;

/// The folder the pointer is over, if it's the desktop or an Explorer window
/// showing a real folder (not "This PC" or a search).
pub fn folder_under_pointer() -> Option<PathBuf> {
    imp::folder_under_pointer()
}

#[cfg(not(windows))]
mod imp {
    pub fn folder_under_pointer() -> Option<std::path::PathBuf> {
        None
    }
}

#[cfg(windows)]
mod imp {
    use std::path::PathBuf;
    use windows::core::Interface;
    use windows::Win32::Foundation::{HWND, POINT};
    use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, IServiceProvider, CLSCTX_ALL, COINIT_APARTMENTTHREADED};
    use windows::Win32::System::Variant::VARIANT;
    use windows::Win32::UI::Shell::{
        IFolderView, IPersistFolder2, IShellBrowser, IShellWindows, IWebBrowserApp, SHGetPathFromIDListEx, ShellWindows, GPFIDL_DEFAULT, SID_STopLevelBrowser,
    };
    use windows::Win32::UI::WindowsAndMessaging::{GetAncestor, GetClassNameW, GetCursorPos, IsWindowVisible, WindowFromPoint, GA_ROOT};

    pub fn folder_under_pointer() -> Option<PathBuf> {
        // SAFETY: plain Win32 calls on values they return; the buffers are ours.
        unsafe {
            let mut at = POINT::default();
            GetCursorPos(&mut at).ok()?;
            let hit = WindowFromPoint(at);
            if hit.is_invalid() {
                return None;
            }
            let window = GetAncestor(hit, GA_ROOT);
            match class_of(window).as_str() {
                // The desktop's icons live in one of these two.
                "Progman" | "WorkerW" => dirs::desktop_dir(),
                "CabinetWClass" | "ExploreWClass" => explorer_folder(window),
                _ => None,
            }
        }
    }

    unsafe fn class_of(window: HWND) -> String {
        let mut name = [0u16; 64];
        let n = unsafe { GetClassNameW(window, &mut name) }.max(0) as usize;
        String::from_utf16_lossy(&name[..n])
    }

    /// The folder open in the Explorer window `window`: the one of its tabs
    /// that's showing.
    unsafe fn explorer_folder(window: HWND) -> Option<PathBuf> {
        unsafe {
            // Ours to undo only if this call is what set COM up on this thread.
            let ours = CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok();
            let found = explorer_folder_in_com(window);
            if ours {
                CoUninitialize();
            }
            found
        }
    }

    unsafe fn explorer_folder_in_com(window: HWND) -> Option<PathBuf> {
        unsafe {
            let all: IShellWindows = CoCreateInstance(&ShellWindows, None, CLSCTX_ALL).ok()?;
            for i in 0..all.Count().ok()? {
                let Ok(item) = all.Item(&VARIANT::from(i)) else { continue };
                let Ok(app) = item.cast::<IWebBrowserApp>() else { continue };
                if app.HWND().ok().map(|h| h.0 as isize) != Some(window.0 as isize) {
                    continue;
                }
                // Every tab of a window answers with the window's handle; the
                // tab on screen is the one whose view is visible.
                let Ok(services) = item.cast::<IServiceProvider>() else { continue };
                let Ok(browser) = services.QueryService::<IShellBrowser>(&SID_STopLevelBrowser) else { continue };
                let Ok(view) = browser.QueryActiveShellView() else { continue };
                if !view.GetWindow().is_ok_and(|w| IsWindowVisible(w).as_bool()) {
                    continue;
                }
                let Ok(folder) = view.cast::<IFolderView>().and_then(|v| v.GetFolder::<IPersistFolder2>()) else { continue };
                let Ok(id) = folder.GetCurFolder() else { continue };
                let mut path = [0u16; 1024];
                let ok = SHGetPathFromIDListEx(id, &mut path, GPFIDL_DEFAULT).as_bool();
                CoTaskMemFree(Some(id as *const _));
                if !ok {
                    // "This PC", a library, a search: not a folder on disk.
                    return None;
                }
                let len = path.iter().position(|&c| c == 0).unwrap_or(path.len());
                return Some(PathBuf::from(String::from_utf16_lossy(&path[..len])));
            }
            None
        }
    }
}
