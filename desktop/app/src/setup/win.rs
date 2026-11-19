//! Windows' side of the installer: the registry under `HKCU`, the Start menu's shortcuts (with
//! SSHeesh's AppUserModelID, as NSIS's `SetLnkAppUserModelId` made them), the one-installer
//! mutex, and a program deleting itself.

use std::path::{Path, PathBuf};

use windows::core::{Interface, GUID, HSTRING, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE, PROPERTYKEY};
use windows::Win32::Storage::FileSystem::{MoveFileExW, MOVEFILE_DELAY_UNTIL_REBOOT};
use windows::Win32::System::Com::StructuredStorage::{PropVariantChangeType, PROPVARIANT, PVCHF_DEFAULT};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, IPersistFile, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED};
use windows::Win32::System::Threading::{CreateMutexW, OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE};
use windows::Win32::System::Variant::VT_LPWSTR;
use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;
use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
use windows_registry::CURRENT_USER;

use super::names::Value;
use super::work::System;

/// `PKEY_AppUserModel_ID`.
const APP_USER_MODEL_ID: PROPERTYKEY = PROPERTYKEY { fmtid: GUID::from_u128(0x9f4c2855_9f79_4b39_a8d0_e1d42de1d5f3), pid: 5 };

/// This Windows.
pub struct Win;

impl System for Win {
    fn read(&self, key: &str, name: &str) -> Option<String> {
        CURRENT_USER.open(key).ok()?.get_string(name).ok()
    }

    fn write(&self, key: &str, values: &[(&str, Value)]) -> Result<(), String> {
        let k = CURRENT_USER.create(key).map_err(|e| format!("the registry key {key} couldn't be written: {e}"))?;
        for (name, value) in values {
            match value {
                Value::Text(t) => k.set_string(name, t),
                Value::Number(n) => k.set_u32(name, *n),
            }
            .map_err(|e| format!("the registry value {key}\\{name} couldn't be written: {e}"))?;
        }
        Ok(())
    }

    fn remove_value(&self, key: &str, name: &str) {
        if let Ok(k) = CURRENT_USER.options().read().write().open(key) {
            let _ = k.remove_value(name);
        }
    }

    fn remove_key(&self, key: &str) {
        let _ = CURRENT_USER.remove_tree(key);
    }

    fn local_app_data(&self) -> PathBuf {
        dirs::data_local_dir().unwrap_or_else(|| PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_default()))
    }

    fn programs_menu(&self) -> PathBuf {
        let roaming = dirs::data_dir().unwrap_or_else(|| PathBuf::from(std::env::var_os("APPDATA").unwrap_or_default()));
        roaming.join("Microsoft").join("Windows").join("Start Menu").join("Programs")
    }

    fn desktop(&self) -> Option<PathBuf> {
        dirs::desktop_dir()
    }

    fn shortcut(&self, link: &Path, target: &Path, app_id: Option<&str>) -> Result<(), String> {
        if let Some(parent) = link.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        // SAFETY: COM calls on objects made here, on this thread, after CoInitializeEx.
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let made = (|| -> windows::core::Result<()> {
                let shell_link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
                shell_link.SetPath(&HSTRING::from(target.as_os_str()))?;
                if let Some(dir) = target.parent() {
                    shell_link.SetWorkingDirectory(&HSTRING::from(dir.as_os_str()))?;
                }
                shell_link.SetIconLocation(&HSTRING::from(target.as_os_str()), 0)?;
                if let Some(id) = app_id {
                    // The shell reads the id as a VT_LPWSTR; `From<&str>` makes a BSTR.
                    let given = PROPVARIANT::from(id);
                    let mut id_value = PROPVARIANT::default();
                    PropVariantChangeType(&mut id_value, &given, PVCHF_DEFAULT, VT_LPWSTR)?;
                    let props: IPropertyStore = shell_link.cast()?;
                    props.SetValue(&APP_USER_MODEL_ID, &id_value)?;
                    props.Commit()?;
                }
                let file: IPersistFile = shell_link.cast()?;
                file.Save(&HSTRING::from(link.as_os_str()), true)
            })();
            made.map_err(|e| format!("the shortcut {} couldn't be made: {e}", link.display()))
        }
    }
}

/// Holds the installer's mutex for as long as the process runs: a second installer (a second
/// click on the shortcut while 0.10's update runs) sees it and leaves quietly. `None`: one is
/// already running.
pub fn only_installer() -> Option<HANDLE> {
    // SAFETY: a named mutex, never closed: it goes with the process.
    unsafe {
        let handle = CreateMutexW(None, false, &HSTRING::from(r"Local\SSHeesh-setup")).ok()?;
        (GetLastError() != ERROR_ALREADY_EXISTS).then_some(handle)
    }
}

/// Deletes `file` at the next start of Windows (a running program can't delete itself).
pub fn delete_at_restart(file: &Path) {
    // SAFETY: plain strings for the call.
    unsafe {
        let _ = MoveFileExW(&HSTRING::from(file.as_os_str()), PCWSTR::null(), MOVEFILE_DELAY_UNTIL_REBOOT);
    }
}

/// Waits up to `most` for the process `pid` to end (the 1.x that started this installer).
pub fn wait_for_exit(pid: u32, most: std::time::Duration) {
    // SAFETY: a handle opened here, waited on and closed here.
    unsafe {
        if let Ok(process) = OpenProcess(PROCESS_SYNCHRONIZE, false, pid) {
            let _ = WaitForSingleObject(process, u32::try_from(most.as_millis()).unwrap_or(u32::MAX));
            let _ = CloseHandle(process);
        }
    }
}
