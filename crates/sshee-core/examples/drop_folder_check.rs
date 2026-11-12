//! On real Windows: opens Explorer at a new folder, puts the pointer over that
//! window, and checks SSHeesh names the folder; then the same for the desktop
//! where it shows. Elsewhere it has nothing to check.
//!
//!   cargo run -p sshee-core --example drop_folder_check

#[cfg(not(windows))]
fn main() {
    println!("Only Windows tells which folder is under the pointer: nothing to check here.");
}

#[cfg(windows)]
fn main() {
    use sshee_core::drop_folder::folder_under_pointer;
    use std::time::{Duration, Instant};
    use windows::core::w;
    use windows::Win32::Foundation::{HWND, POINT, RECT};
    use windows::Win32::UI::WindowsAndMessaging::{
        FindWindowExW, GetAncestor, GetClassNameW, GetSystemMetrics, GetWindowRect, SetCursorPos, SetWindowPos, ShowWindow, WindowFromPoint, GA_ROOT, HWND_TOPMOST, SM_CXSCREEN,
        SM_CYSCREEN, SWP_NOMOVE, SWP_NOSIZE, SW_MINIMIZE,
    };

    let dir = std::env::temp_dir().join(format!("ssheesh drop check {}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let dir = dir.canonicalize().unwrap();
    // Without the \\?\ canonicalize adds, as Explorer says it.
    let want = dir.to_string_lossy().trim_start_matches(r"\\?\").to_lowercase();
    std::process::Command::new("explorer.exe").arg(&dir).spawn().expect("explorer starts");

    let explorers = || {
        let mut found = Vec::new();
        let mut prev: Option<HWND> = None;
        // SAFETY: a window handle or none, and a class name literal.
        while let Ok(w) = unsafe { FindWindowExW(None, prev, w!("CabinetWClass"), None) } {
            if w.is_invalid() {
                break;
            }
            found.push(w);
            prev = Some(w);
        }
        found
    };
    let point_at = |x: i32, y: i32| {
        // SAFETY: moves the pointer.
        unsafe { SetCursorPos(x, y).unwrap() };
        std::thread::sleep(Duration::from_millis(250));
    };

    let start = Instant::now();
    let mut seen = Vec::new();
    let mut window = None;
    'wait: while start.elapsed() < Duration::from_secs(40) {
        for w in explorers() {
            let mut r = RECT::default();
            // SAFETY: our own RECT, and a handle Windows just gave.
            unsafe {
                let _ = SetWindowPos(w, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE);
                if GetWindowRect(w, &mut r).is_err() {
                    continue;
                }
            }
            point_at((r.left + r.right) / 2, (r.top + r.bottom) / 2);
            let got = folder_under_pointer().map(|p| p.to_string_lossy().to_lowercase());
            if got.as_deref() == Some(want.as_str()) {
                println!("ok: Explorer window -> {}", got.unwrap());
                window = Some(w);
                break 'wait;
            }
            seen.push(got);
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    let Some(window) = window else {
        eprintln!("FAILED: no Explorer window answered {want}; saw {seen:?}");
        std::process::exit(1);
    };

    // Out of the way, then the desktop wherever it shows through.
    // SAFETY: a handle from above; screen sizes.
    let (w, h) = unsafe {
        let _ = ShowWindow(window, SW_MINIMIZE);
        (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN))
    };
    std::thread::sleep(Duration::from_millis(800));
    let desktop_at = [(6, h / 2), (w - 6, h / 2), (w / 2, 6), (6, 6), (w - 6, 6), (w / 2, h / 3)].into_iter().find(|&(x, y)| {
        let mut name = [0u16; 64];
        // SAFETY: a point on screen and our own buffer.
        let n = unsafe { GetClassNameW(GetAncestor(WindowFromPoint(POINT { x, y }), GA_ROOT), &mut name) }.max(0) as usize;
        matches!(String::from_utf16_lossy(&name[..n]).as_str(), "Progman" | "WorkerW")
    });
    match desktop_at {
        Some((x, y)) => {
            point_at(x, y);
            let got = folder_under_pointer();
            if got.is_none() || got != dirs::desktop_dir() {
                eprintln!("FAILED: over the desktop it said {got:?}, not {:?}", dirs::desktop_dir());
                std::process::exit(1);
            }
            println!("ok: desktop -> {}", got.unwrap().display());
        }
        None => println!("The desktop shows nowhere on this screen: not checked."),
    }

    let _ = std::fs::remove_dir(&dir);
    println!("drop folder check passed");
}
