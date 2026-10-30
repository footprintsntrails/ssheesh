//! Tauri glue: exposes sshee-core to the UI as commands.

use serde::{Deserialize, Serialize};
use sshee_core::agents::{self, PaneInfo};
use sshee_core::checkup::{self, Checkup};
use sshee_core::disk::{self, DiskReport, Usage};
use sshee_core::containers::{self, ContainerList, Engine};
use sshee_core::files::{self, DirListing, TextFile, Transfer};
use sshee_core::metrics::{self, Metrics};
use sshee_core::secrets::PasswordStore;
use sshee_core::hostkeys::{HostKeyCheck, PendingKeys};
use sshee_core::session::{self, EventSink};
use sshee_core::{ErrorCode, Host, HostSource, HostStore, NewHost, OpError, SessionEvent, SessionManager, SessionSpec, SshEnv, Target};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::ipc::Channel;
use tauri::{Manager, State};

struct AppState {
    sessions: SessionManager,
    hosts: Mutex<HostStore>,
    passwords: PasswordStore,
    host_keys: Arc<PendingKeys>,
    env: SshEnv,
    /// Uploads and downloads in progress, by the id the UI gave them.
    transfers: Mutex<HashMap<String, Arc<Transfer>>>,
}

/// A container a terminal opens into, and what to run there.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ContainerRef {
    engine: Engine,
    id: String,
    run: containers::Run,
}

/// How the UI names a place: this computer, or a server from the list.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
enum TargetSpec {
    Local,
    Host { host_id: String },
}

fn resolve(state: &AppState, spec: &TargetSpec) -> Result<Target, OpError> {
    match spec {
        TargetSpec::Local => Ok(Target::Local),
        TargetSpec::Host { host_id } => {
            let mut hosts = state.hosts.lock().unwrap();
            let h = hosts
                .get(host_id)
                .ok_or_else(|| OpError::new(ErrorCode::NotFound, "That server isn't in your list anymore."))?;
            let mut t = h.target();
            t.password = state.passwords.get(&h.id, h.remembered_password);
            // A server added in the app may have its password in the keychain
            // without a note saying so (0.3.4 and earlier). Find it, note it.
            if t.password.is_none() && !h.remembered_password && h.source == HostSource::Saved {
                t.password = state.passwords.recover(&h.id);
                if t.password.is_some() {
                    let _ = hosts.set_remembered_password(&h.id, true);
                }
            }
            Ok(Target::Ssh(t))
        }
    }
}

/// Runs slow work (network, disk) off the UI thread.
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T, OpError> + Send + 'static) -> Result<T, OpError> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| OpError::failed(e.to_string()))?
}

// ---------------------------------------------------------------- servers

#[tauri::command]
fn list_hosts(state: State<AppState>) -> Vec<Host> {
    state.hosts.lock().unwrap().list()
}

#[tauri::command]
fn add_host(state: State<AppState>, input: NewHost) -> Result<AddedHost, String> {
    // The form keeps its password while the server is saved: that's how the
    // list notes the password is in the keychain.
    let password = input.password.clone().filter(|p| !p.expose().is_empty());
    let remember = input.remember_password;
    let mut host = state.hosts.lock().unwrap().add(input)?;
    let mut warning = None;
    if let Some(pw) = password {
        if let Err(e) = state.passwords.set(&host.id, pw, remember) {
            state.hosts.lock().unwrap().set_remembered_password(&host.id, false)?;
            host.remembered_password = false;
            warning = Some(e);
        }
    }
    Ok(AddedHost { host, warning })
}

#[derive(serde::Serialize)]
struct AddedHost {
    host: Host,
    /// Set when the password couldn't go into the keychain.
    warning: Option<String>,
}

#[tauri::command]
fn remove_host(state: State<AppState>, id: String) -> Result<(), String> {
    state.passwords.forget(&id);
    state.hosts.lock().unwrap().remove(&id)
}

/// Drops a host's password from memory and the keychain.
#[tauri::command]
fn forget_password(state: State<AppState>, id: String) -> Result<(), String> {
    state.passwords.forget(&id);
    state.hosts.lock().unwrap().set_remembered_password(&id, false)
}

// ---------------------------------------------------------------- terminals

#[tauri::command]
fn open_session(
    state: State<AppState>,
    pane_id: String,
    target: TargetSpec,
    session_key: String,
    cols: u16,
    rows: u16,
    container: Option<ContainerRef>,
    on_event: Channel<SessionEvent>,
) -> Result<(), OpError> {
    let target = resolve(&state, &target)?;
    let run = container.map(|c| containers::argv(c.engine, &c.id, c.run));
    let spec = SessionSpec { target, key: session_key, cols, rows, env: state.env.clone(), run };
    let sink: EventSink = Arc::new(move |event| {
        let _ = on_event.send(event);
    });
    state.sessions.open(&pane_id, spec, sink);
    Ok(())
}

#[tauri::command]
fn write_session(state: State<AppState>, pane_id: String, data: String) {
    state.sessions.write(&pane_id, data.as_bytes());
}

#[tauri::command]
fn resize_session(state: State<AppState>, pane_id: String, cols: u16, rows: u16) {
    state.sessions.resize(&pane_id, cols, rows);
}

/// Reconnect by itself after a drop (true), or wait for the user (false).
#[tauri::command]
fn set_auto_reconnect(state: State<AppState>, enabled: bool) {
    state.sessions.set_auto_reconnect(enabled);
}

#[tauri::command]
fn reconnect_session(state: State<AppState>, pane_id: String) {
    state.sessions.reconnect_now(&pane_id);
}

/// Closes a pane's session. With `end_remote`, also ends its tmux session on
/// the server (the user closed the pane on purpose).
#[tauri::command]
fn close_session(state: State<AppState>, pane_id: String, target: Option<TargetSpec>, session_key: Option<String>, end_remote: bool) {
    state.sessions.close(&pane_id);
    if let (true, Some(target), Some(key)) = (end_remote, target, session_key) {
        if let Ok(target) = resolve(&state, &target) {
            let env = state.env.clone();
            std::thread::spawn(move || {
                let _ = session::end_remote(&target, &env, &key);
            });
        }
    }
}

fn ssh_target(state: &AppState, host_id: &str) -> Result<sshee_core::ssh::SshTarget, OpError> {
    match resolve(state, &TargetSpec::Host { host_id: host_id.to_string() })? {
        Target::Ssh(t) => Ok(t),
        Target::Local => Err(OpError::failed("That's this computer, not a server.")),
    }
}

/// Is the server's key known? If not, fetches it so you can check it first.
#[tauri::command]
async fn check_host_key(state: State<'_, AppState>, host_id: String, changed: bool) -> Result<HostKeyCheck, OpError> {
    let t = ssh_target(&state, &host_id)?;
    let keys = state.host_keys.clone();
    blocking(move || keys.check(&host_id, &t, changed)).await
}

/// Saves the key you were just shown to ~/.ssh/known_hosts.
#[tauri::command]
fn trust_host_key(state: State<AppState>, host_id: String) -> Result<(), OpError> {
    let t = ssh_target(&state, &host_id)?;
    state.host_keys.trust(&host_id, &t)
}

/// Keys in ~/.ssh and key agents running on this computer.
#[tauri::command]
async fn local_auth() -> Result<sshee_core::keys::LocalAuth, OpError> {
    blocking(|| Ok(sshee_core::keys::discover())).await
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ConnectionTest {
    host_key: HostKeyCheck,
    signed_in: bool,
    /// Whether tmux is installed, so sessions can survive a dropped connection.
    tmux: Option<bool>,
    message: Option<String>,
}

/// Tries a server from the "Add a server" form before it's saved: is it
/// there, is its identity trusted, does the login work, is tmux installed?
#[tauri::command]
async fn test_connection(state: State<'_, AppState>, input: NewHost) -> Result<ConnectionTest, OpError> {
    let host = sshee_core::hosts::preview(&input).map_err(OpError::failed)?;
    let mut t = host.target();
    t.password = input.password.clone().filter(|p| !p.expose().is_empty());
    let (keys, env) = (state.host_keys.clone(), state.env.clone());
    blocking(move || {
        let host_key = keys.check(&host.id, &t, false)?;
        if host_key != HostKeyCheck::Known {
            return Ok(ConnectionTest { host_key, signed_in: false, tmux: None, message: None });
        }
        let probe = "if command -v tmux >/dev/null 2>&1; then echo tmux=yes; else echo tmux=no; fi";
        Ok(match sshee_core::ssh::run_remote(&t, &env, probe, None) {
            Ok(out) => ConnectionTest {
                host_key,
                signed_in: true,
                tmux: Some(String::from_utf8_lossy(&out).contains("tmux=yes")),
                message: None,
            },
            Err(e) => ConnectionTest { host_key, signed_in: false, tmux: None, message: Some(e.message) },
        })
    })
    .await
}

/// Trusts the identity a connection test showed.
#[tauri::command]
fn trust_test_host_key(state: State<AppState>, input: NewHost) -> Result<(), OpError> {
    let host = sshee_core::hosts::preview(&input).map_err(OpError::failed)?;
    state.host_keys.trust(&host.id, &host.target())
}

/// Recent scrollback of a durable session, for the "while you were away" summary.
#[tauri::command]
async fn session_history(state: State<'_, AppState>, target: TargetSpec, session_key: String, lines: u32) -> Result<String, OpError> {
    let (t, env) = (resolve(&state, &target)?, state.env.clone());
    blocking(move || session::history(&t, &env, &session_key, lines)).await
}

// ---------------------------------------------------------------- files

#[tauri::command]
async fn list_dir(state: State<'_, AppState>, target: TargetSpec, path: String) -> Result<DirListing, OpError> {
    let (t, env) = (resolve(&state, &target)?, state.env.clone());
    blocking(move || files::list(&t, &env, &path)).await
}

#[tauri::command]
async fn read_file(state: State<'_, AppState>, target: TargetSpec, path: String) -> Result<TextFile, OpError> {
    let (t, env) = (resolve(&state, &target)?, state.env.clone());
    blocking(move || files::read_text(&t, &env, &path)).await
}

#[tauri::command]
async fn write_file(state: State<'_, AppState>, target: TargetSpec, path: String, content: String) -> Result<(), OpError> {
    let (t, env) = (resolve(&state, &target)?, state.env.clone());
    blocking(move || files::write_text(&t, &env, &path, &content)).await
}

#[tauri::command]
async fn make_dir(state: State<'_, AppState>, target: TargetSpec, path: String) -> Result<(), OpError> {
    let (t, env) = (resolve(&state, &target)?, state.env.clone());
    blocking(move || files::make_dir(&t, &env, &path)).await
}

/// Packs files and folders into a .tar.gz next to them.
#[tauri::command]
async fn archive_paths(state: State<'_, AppState>, target: TargetSpec, dir: String, names: Vec<String>, archive: String) -> Result<(), OpError> {
    let (t, env) = (resolve(&state, &target)?, state.env.clone());
    blocking(move || files::archive(&t, &env, &dir, &names, &archive)).await
}

#[tauri::command]
async fn rename_path(state: State<'_, AppState>, target: TargetSpec, from: String, to: String) -> Result<(), OpError> {
    let (t, env) = (resolve(&state, &target)?, state.env.clone());
    blocking(move || files::rename(&t, &env, &from, &to)).await
}

#[tauri::command]
async fn delete_path(state: State<'_, AppState>, target: TargetSpec, path: String) -> Result<(), OpError> {
    let (t, env) = (resolve(&state, &target)?, state.env.clone());
    blocking(move || files::delete(&t, &env, &path)).await
}

/// Bytes moved so far and the total (0 when unknown), for the transfers panel.
#[derive(Clone, Serialize)]
struct TransferProgress {
    done: u64,
    total: u64,
}

/// Runs an upload or download that the UI can follow and cancel by `id`.
async fn run_transfer<T: Send + 'static>(
    state: &AppState,
    id: String,
    on_progress: Channel<TransferProgress>,
    work: impl FnOnce(Arc<Transfer>) -> Result<T, OpError> + Send + 'static,
) -> Result<T, OpError> {
    let xfer = Transfer::new(move |done, total| {
        let _ = on_progress.send(TransferProgress { done, total });
    });
    state.transfers.lock().unwrap().insert(id.clone(), xfer.clone());
    let result = blocking(move || work(xfer)).await;
    state.transfers.lock().unwrap().remove(&id);
    result
}

#[tauri::command]
fn cancel_transfer(state: State<AppState>, id: String) {
    if let Some(xfer) = state.transfers.lock().unwrap().get(&id) {
        xfer.cancel();
    }
}

/// Saves a server file or folder into Downloads; returns the local path.
#[tauri::command]
async fn download_path(
    state: State<'_, AppState>,
    target: TargetSpec,
    path: String,
    is_dir: bool,
    transfer_id: String,
    on_progress: Channel<TransferProgress>,
) -> Result<String, OpError> {
    let (t, env) = (resolve(&state, &target)?, state.env.clone());
    let dest = dirs::download_dir().or_else(dirs::home_dir).unwrap_or_else(|| PathBuf::from("."));
    run_transfer(&state, transfer_id, on_progress, move |xfer| files::download(&t, &env, &path, is_dir, &dest, &xfer)).await
}

#[tauri::command]
async fn upload_paths(
    state: State<'_, AppState>,
    target: TargetSpec,
    paths: Vec<String>,
    remote_dir: String,
    overwrite: bool,
    transfer_id: String,
    on_progress: Channel<TransferProgress>,
) -> Result<(), OpError> {
    let (t, env) = (resolve(&state, &target)?, state.env.clone());
    run_transfer(&state, transfer_id, on_progress, move |xfer| {
        let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
        xfer.add_total(paths.iter().map(|p| files::upload_size(p)).sum());
        for p in &paths {
            files::upload(&t, &env, p, &remote_dir, overwrite, &xfer)?;
        }
        Ok(())
    })
    .await
}

/// The portable Windows .exe: run straight from a download, not installed.
/// It updates itself with `install_portable_update`.
#[tauri::command]
fn is_portable() -> bool {
    cfg!(windows)
        && std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(|dir| !dir.join("uninstall.exe").exists()))
            .unwrap_or(false)
}

/// The portable .exe's update: downloaded and checked against the release's
/// signature like any update, then swapped in for the running one. A
/// restart runs it.
#[cfg(desktop)]
#[tauri::command]
async fn install_portable_update(app: tauri::AppHandle, on_progress: Channel<TransferProgress>) -> Result<(), OpError> {
    use tauri_plugin_updater::UpdaterExt;
    let failed = |e: tauri_plugin_updater::Error| OpError::failed(e.to_string());
    let update = app
        .updater_builder()
        .target("windows-x86_64-portable")
        .build()
        .map_err(failed)?
        .check()
        .await
        .map_err(failed)?
        .ok_or_else(|| OpError::failed("There's no newer portable version yet."))?;
    let mut done = 0u64;
    let bytes = update
        .download(
            |chunk, total| {
                done += chunk as u64;
                let _ = on_progress.send(TransferProgress { done, total: total.unwrap_or(0) });
            },
            || {},
        )
        .await
        .map_err(failed)?;
    blocking(move || swap_in_portable(&bytes)).await
}

/// "SSHeesh.exe" -> "SSHeesh.<tag>.exe", next to it.
fn portable_beside(exe: &std::path::Path, tag: &str) -> PathBuf {
    let stem = exe.file_stem().unwrap_or_default().to_string_lossy();
    exe.with_file_name(format!("{stem}.{tag}.exe"))
}

/// Windows won't overwrite a running program but lets it be renamed, so the
/// running copy steps aside as "<name>.old.exe" (removed at the next start)
/// and the new one takes its name, which keeps shortcuts working.
fn swap_in_portable(bytes: &[u8]) -> Result<(), OpError> {
    swap_in(&std::env::current_exe()?, bytes)
}

fn swap_in(exe: &std::path::Path, bytes: &[u8]) -> Result<(), OpError> {
    let (new, old) = (portable_beside(exe, "new"), portable_beside(exe, "old"));
    std::fs::write(&new, bytes).map_err(|e| OpError::failed(format!("Couldn't save the update next to SSHeesh: {e}")))?;
    let _ = std::fs::remove_file(&old);
    if let Err(e) = std::fs::rename(exe, &old) {
        let _ = std::fs::remove_file(&new);
        return Err(e.into());
    }
    if let Err(e) = std::fs::rename(&new, exe) {
        let _ = std::fs::rename(&old, exe);
        let _ = std::fs::remove_file(&new);
        return Err(e.into());
    }
    Ok(())
}

/// Removes the copy the last portable update set aside, once the old
/// process has had time to exit.
fn remove_old_portable() {
    let Ok(exe) = std::env::current_exe() else { return };
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(5));
        let _ = std::fs::remove_file(portable_beside(&exe, "old"));
    });
}

/// This computer's user and machine name, for streamer mode.
#[tauri::command]
fn local_identity() -> sshee_core::identity::LocalIdentity {
    sshee_core::identity::local()
}

/// AI coding agents installed on the server ("claude", "codex", …).
#[tauri::command]
async fn detect_agents(state: State<'_, AppState>, target: TargetSpec) -> Result<Vec<String>, OpError> {
    let (t, env) = (resolve(&state, &target)?, state.env.clone());
    blocking(move || agents::detect(&t, &env)).await
}

/// A terminal's foreground program and folder, from tmux on the server.
#[tauri::command]
async fn pane_info(state: State<'_, AppState>, target: TargetSpec, session_key: String) -> Result<PaneInfo, OpError> {
    let (t, env) = (resolve(&state, &target)?, state.env.clone());
    blocking(move || agents::pane_info(&t, &env, &session_key)).await
}

/// A read-only security check of the server.
#[tauri::command]
async fn security_checkup(state: State<'_, AppState>, target: TargetSpec) -> Result<Checkup, OpError> {
    let (t, env) = (resolve(&state, &target)?, state.env.clone());
    blocking(move || checkup::run(&t, &env)).await
}

/// Disks, Docker, the journal and safe cleanups.
#[tauri::command]
async fn disk_report(state: State<'_, AppState>, target: TargetSpec) -> Result<DiskReport, OpError> {
    let (t, env) = (resolve(&state, &target)?, state.env.clone());
    blocking(move || disk::report(&t, &env)).await
}

/// The biggest files and folders directly inside `path`.
#[tauri::command]
async fn disk_biggest(state: State<'_, AppState>, target: TargetSpec, path: String) -> Result<Vec<Usage>, OpError> {
    let (t, env) = (resolve(&state, &target)?, state.env.clone());
    blocking(move || disk::biggest(&t, &env, &path)).await
}

#[tauri::command]
async fn session_cwd(state: State<'_, AppState>, target: TargetSpec, session_key: String) -> Result<String, OpError> {
    let (t, env) = (resolve(&state, &target)?, state.env.clone());
    blocking(move || files::session_cwd(&t, &env, &session_key)).await
}

// ---------------------------------------------------------------- containers

#[tauri::command]
async fn list_containers(state: State<'_, AppState>, target: TargetSpec) -> Result<ContainerList, OpError> {
    let (t, env) = (resolve(&state, &target)?, state.env.clone());
    blocking(move || containers::list(&t, &env)).await
}

#[tauri::command]
async fn container_action(
    state: State<'_, AppState>,
    target: TargetSpec,
    engine: Engine,
    id: String,
    action: containers::Action,
) -> Result<(), OpError> {
    let (t, env) = (resolve(&state, &target)?, state.env.clone());
    blocking(move || containers::act(&t, &env, engine, &id, action)).await
}

// ---------------------------------------------------------------- metrics

#[tauri::command]
async fn fetch_metrics(state: State<'_, AppState>, target: TargetSpec) -> Result<Metrics, OpError> {
    let (t, env) = (resolve(&state, &target)?, state.env.clone());
    blocking(move || metrics::fetch(&t, &env)).await
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .setup(|app| {
            #[cfg(desktop)]
            {
                app.handle().plugin(tauri_plugin_updater::Builder::new().build())?;
                app.handle().plugin(tauri_plugin_process::init())?;
                // The window opens at the size and place you left it.
                app.handle().plugin(tauri_plugin_window_state::Builder::default().build())?;
            }
            if is_portable() {
                remove_old_portable();
            }
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            app.manage(AppState {
                sessions: SessionManager::new(),
                hosts: Mutex::new(HostStore::new(&data_dir)),
                passwords: PasswordStore::default(),
                host_keys: Arc::new(PendingKeys::default()),
                transfers: Mutex::default(),
                env: SshEnv {
                    // ssh runs this same executable to ask for a password; see main.rs.
                    askpass: std::env::current_exe().ok(),
                    ..SshEnv::with_control_dir(sshee_core::ssh::default_control_dir())
                },
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_hosts,
            add_host,
            remove_host,
            forget_password,
            session_history,
            check_host_key,
            trust_host_key,
            local_auth,
            test_connection,
            trust_test_host_key,
            open_session,
            write_session,
            resize_session,
            reconnect_session,
            set_auto_reconnect,
            close_session,
            list_dir,
            read_file,
            write_file,
            make_dir,
            rename_path,
            archive_paths,
            delete_path,
            download_path,
            upload_paths,
            cancel_transfer,
            detect_agents,
            pane_info,
            security_checkup,
            disk_report,
            disk_biggest,
            session_cwd,
            fetch_metrics,
            local_identity,
            is_portable,
            #[cfg(desktop)]
            install_portable_update,
            list_containers,
            container_action,
        ])
        .build(tauri::generate_context!())
        .expect("error while starting SSHeesh")
        .run(|app, event| {
            // Local ssh processes go away with the app; tmux sessions on the
            // servers stay, so reopening the app picks them back up.
            if let tauri::RunEvent::Exit = event {
                app.state::<AppState>().sessions.close_all();
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portable_update_takes_the_old_files_place() {
        let dir = std::env::temp_dir().join(format!("sshee-swap-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("SSHeesh_0.4.0_x64-portable.exe");
        std::fs::write(&exe, "old").unwrap();
        swap_in(&exe, b"new").unwrap();
        assert_eq!(std::fs::read_to_string(&exe).unwrap(), "new");
        assert_eq!(std::fs::read_to_string(dir.join("SSHeesh_0.4.0_x64-portable.old.exe")).unwrap(), "old");
        assert!(!dir.join("SSHeesh_0.4.0_x64-portable.new.exe").exists());
        // A second update replaces the copy the first one set aside.
        swap_in(&exe, b"newer").unwrap();
        assert_eq!(std::fs::read_to_string(&exe).unwrap(), "newer");
        assert_eq!(std::fs::read_to_string(dir.join("SSHeesh_0.4.0_x64-portable.old.exe")).unwrap(), "new");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
