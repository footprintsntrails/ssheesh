//! Tauri glue: exposes sshee-core to the UI as commands.

use serde::{Deserialize, Serialize};
use sshee_core::agents::{self, PaneInfo};
use sshee_core::checkup::{self, Checkup};
use sshee_core::disk::{self, DiskReport, Usage};
use sshee_core::fixes;
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
use tauri::{AppHandle, Manager, State};

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
            // The keychain may take a while (or ask to be unlocked): the
            // server list isn't held meanwhile.
            let (mut t, remembered, saved) = {
                let mut hosts = state.hosts.lock().unwrap();
                let h = hosts
                    .get(host_id)
                    .ok_or_else(|| OpError::new(ErrorCode::NotFound, "That server isn't in your list anymore."))?;
                (h.target(), h.remembered_password, h.source == HostSource::Saved)
            };
            t.password = state.passwords.get(host_id, remembered);
            // A server added in the app may have its password in the keychain
            // without a note saying so (0.3.4 and earlier). Find it, note it.
            if t.password.is_none() && !remembered && saved {
                t.password = state.passwords.recover(host_id);
                if t.password.is_some() {
                    let _ = state.hosts.lock().unwrap().set_remembered_password(host_id, true);
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

// The commands below read files or the keychain, which can take a while
// (or wait for the keychain to be unlocked): they run off the UI thread.
// Their errors stay plain text, as the UI has always had them.

#[tauri::command]
async fn list_hosts(app: AppHandle) -> Result<Vec<Host>, OpError> {
    blocking(move || Ok(app.state::<AppState>().hosts.lock().unwrap().list())).await
}

#[tauri::command]
async fn add_host(app: AppHandle, input: NewHost) -> Result<AddedHost, String> {
    blocking(move || Ok(add_host_now(&app.state::<AppState>(), input))).await.map_err(|e| e.message)?
}

fn add_host_now(state: &AppState, input: NewHost) -> Result<AddedHost, String> {
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
async fn remove_host(app: AppHandle, id: String) -> Result<(), String> {
    blocking(move || {
        let state = app.state::<AppState>();
        state.passwords.forget(&id);
        let removed = state.hosts.lock().unwrap().remove(&id);
        Ok(removed)
    })
    .await
    .map_err(|e| e.message)?
}

/// Drops a host's password from memory and the keychain.
#[tauri::command]
async fn forget_password(app: AppHandle, id: String) -> Result<(), String> {
    blocking(move || {
        let state = app.state::<AppState>();
        state.passwords.forget(&id);
        let noted = state.hosts.lock().unwrap().set_remembered_password(&id, false);
        Ok(noted)
    })
    .await
    .map_err(|e| e.message)?
}

// ---------------------------------------------------------------- terminals

/// Stays a quick command on the UI thread, so it keeps its place before the
/// keys, resizes and Close that follow it; the server's password is looked
/// up on the session's own thread. A server that's gone ends it as Failed.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
fn open_session(
    app: AppHandle,
    state: State<AppState>,
    pane_id: String,
    target: TargetSpec,
    session_key: String,
    cols: u16,
    rows: u16,
    container: Option<ContainerRef>,
    welcome: Option<sshee_core::welcome::Welcome>,
    on_event: Channel<SessionEvent>,
) {
    let run = container.map(|c| containers::argv(c.engine, &c.id, c.run));
    let prepare = move || {
        let state = app.state::<AppState>();
        let target = resolve(&state, &target).map_err(|e| e.message)?;
        Ok(SessionSpec { target, key: session_key, env: state.env.clone(), run, welcome: welcome.unwrap_or_default() })
    };
    let sink: EventSink = Arc::new(move |event| {
        let _ = on_event.send(event);
    });
    state.sessions.open(&pane_id, cols, rows, prepare, sink);
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
/// the server (the user closed the pane on purpose). The close itself stays
/// on the UI thread, in order with `open_session` for the same pane; the
/// keychain and the server are dealt with on a thread of their own.
#[tauri::command]
fn close_session(app: AppHandle, state: State<AppState>, pane_id: String, target: Option<TargetSpec>, session_key: Option<String>, end_remote: bool) {
    state.sessions.close(&pane_id);
    if let (true, Some(target), Some(key)) = (end_remote, target, session_key) {
        std::thread::spawn(move || {
            let state = app.state::<AppState>();
            if let Ok(target) = resolve(&state, &target) {
                let _ = session::end_remote(&target, &state.env, &key);
            }
        });
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
async fn trust_host_key(app: AppHandle, host_id: String) -> Result<(), OpError> {
    blocking(move || {
        let state = app.state::<AppState>();
        let t = ssh_target(&state, &host_id)?;
        state.host_keys.trust(&host_id, &t)
    })
    .await
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
async fn trust_test_host_key(state: State<'_, AppState>, input: NewHost) -> Result<(), OpError> {
    let host = sshee_core::hosts::preview(&input).map_err(OpError::failed)?;
    let keys = state.host_keys.clone();
    blocking(move || keys.trust(&host.id, &host.target())).await
}

/// Recent scrollback of a durable session, for the "while you were away" summary.
#[tauri::command]
async fn session_history(state: State<'_, AppState>, target: TargetSpec, session_key: String, lines: u32) -> Result<String, OpError> {
    let (t, env) = (resolve(&state, &target)?, state.env.clone());
    blocking(move || session::history(&t, &env, &session_key, lines)).await
}

/// Scrolls a server terminal's tmux history to a position (or just reports it).
#[tauri::command]
async fn tmux_scroll(state: State<'_, AppState>, target: TargetSpec, session_key: String, position: Option<u32>) -> Result<session::TmuxScroll, OpError> {
    let (t, env) = (resolve(&state, &target)?, state.env.clone());
    blocking(move || session::tmux_scroll(&t, &env, &session_key, position)).await
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

/// Bytes moved so far and the total (0 when unknown), for the transfers
/// panel, and whether it's waiting for a dropped connection to come back.
#[derive(Clone, Serialize)]
struct TransferProgress {
    done: u64,
    total: u64,
    waiting: bool,
}

/// Runs an upload or download that the UI can follow and cancel by `id`.
async fn run_transfer<T: Send + 'static>(
    state: &AppState,
    id: String,
    on_progress: Channel<TransferProgress>,
    work: impl FnOnce(Arc<Transfer>) -> Result<T, OpError> + Send + 'static,
) -> Result<T, OpError> {
    let xfer = Transfer::new(move |p| {
        let _ = on_progress.send(TransferProgress { done: p.done, total: p.total, waiting: p.waiting });
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

/// Where downloads go. Without a Downloads folder set up (a minimal Linux
/// desktop) the system names none, or names the home folder: a server's
/// `.bashrc` would then land live in it, so they go to ~/Downloads instead.
fn downloads_dir() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    dirs::download_dir().filter(|d| *d != home).unwrap_or_else(|| home.join("Downloads"))
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
    run_transfer(&state, transfer_id, on_progress, move |xfer| files::download(&t, &env, &path, is_dir, &downloads_dir(), &xfer)).await
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
        // Everything that would be replaced, asked about once, before anything moves.
        if !overwrite {
            files::check_upload(&t, &env, &paths, &remote_dir)?;
        }
        xfer.check()?;
        xfer.add_total(paths.iter().map(|p| files::upload_size(p)).sum());
        for p in &paths {
            files::upload(&t, &env, p, &remote_dir, overwrite, &xfer)?;
        }
        Ok(())
    })
    .await
}

/// Copies files from one folder on a machine into a folder on a server:
/// from another server straight through, from this computer as an upload.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn copy_to_server(
    state: State<'_, AppState>,
    from: TargetSpec,
    to: TargetSpec,
    paths: Vec<String>,
    dest_dir: String,
    overwrite: bool,
    transfer_id: String,
    on_progress: Channel<TransferProgress>,
) -> Result<(), OpError> {
    let (a, b, env) = (resolve(&state, &from)?, resolve(&state, &to)?, state.env.clone());
    run_transfer(&state, transfer_id, on_progress, move |xfer| files::copy_between(&a, &b, &env, &paths, &dest_dir, overwrite, &xfer)).await
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
    // Each step says what it was doing when it went wrong, so a failure can be told apart.
    let failed = |step: &'static str| move |e: tauri_plugin_updater::Error| OpError::failed(format!("{step}: {e}"));
    let update = app
        .updater_builder()
        .target("windows-x86_64-portable")
        .build()
        .map_err(failed("Couldn't get ready to update"))?
        .check()
        .await
        .map_err(failed("Couldn't look for the new version"))?
        .ok_or_else(|| OpError::failed("There's no newer portable version yet."))?;
    let version = update.version.clone();
    let mut done = 0u64;
    let bytes = update
        .download(
            |chunk, total| {
                done += chunk as u64;
                let _ = on_progress.send(TransferProgress { done, total: total.unwrap_or(0), waiting: false });
            },
            || {},
        )
        .await
        .map_err(failed("Couldn't download the new version"))?;
    blocking(move || swap_in_portable(&bytes, &version)).await
}

/// "SSHeesh.exe" -> "SSHeesh.<tag>.exe", next to it.
fn portable_beside(exe: &std::path::Path, tag: &str) -> PathBuf {
    let stem = exe.file_stem().unwrap_or_default().to_string_lossy();
    exe.with_file_name(format!("{stem}.{tag}.exe"))
}

/// Windows won't overwrite a running program but lets it be renamed, so the
/// running copy steps aside as "<name>.old.exe" (removed at the next start)
/// and the new one takes its name, which keeps shortcuts working.
fn swap_in_portable(bytes: &[u8], version: &str) -> Result<(), OpError> {
    let exe = std::env::current_exe().map_err(|e| OpError::failed(format!("Couldn't tell where SSHeesh runs from: {e}")))?;
    swap_in(&exe, bytes, version)
}

/// An antivirus looks at a new program as it's saved: it may hold the file
/// for a moment, or take it away. The swap gives it this long first.
const ANTIVIRUS_LOOK: std::time::Duration = std::time::Duration::from_millis(if cfg!(test) { 0 } else { 1500 });

/// What to say when the saved update disappears: the app isn't signed yet,
/// and Windows Security (or another antivirus) may take a new copy away.
const TAKEN_AWAY: &str = "The new version was taken away right after it was saved, most likely by Windows Security or another antivirus, since SSHeesh isn't signed yet. Allow it under Windows Security → Virus & threat protection → Protection history and try again, or get the new file from the download page.";

fn swap_in(exe: &std::path::Path, bytes: &[u8], version: &str) -> Result<(), OpError> {
    let (new, old) = (portable_beside(exe, "new"), portable_beside(exe, "old"));
    let dir = exe.parent().map(|d| d.display().to_string()).unwrap_or_default();
    std::fs::write(&new, bytes).map_err(|e| OpError::failed(format!("Couldn't save the update in {dir}: {e}")))?;
    std::thread::sleep(ANTIVIRUS_LOOK);
    if !new.exists() {
        return Err(OpError::failed(TAKEN_AWAY));
    }
    let _ = std::fs::remove_file(&old);
    if let Err(e) = retrying(|| std::fs::rename(exe, &old)) {
        // This copy can't step aside: the new one stays next to it under its
        // version's name, to open by hand.
        let kept = exe.with_file_name(format!("SSHeesh-{version}.exe"));
        let _ = std::fs::remove_file(&kept);
        return Err(OpError::failed(match std::fs::rename(&new, &kept) {
            Ok(()) => format!(
                "Couldn't replace the SSHeesh that's running ({e}). The new version is saved next to it as {}: open that one.",
                kept.file_name().unwrap_or_default().to_string_lossy()
            ),
            Err(_) => {
                let _ = std::fs::remove_file(&new);
                format!("Couldn't replace the SSHeesh that's running: {e}")
            }
        }));
    }
    if let Err(e) = retrying(|| std::fs::rename(&new, exe)) {
        let _ = std::fs::rename(&old, exe);
        if !new.exists() {
            return Err(OpError::failed(TAKEN_AWAY));
        }
        let _ = std::fs::remove_file(&new);
        return Err(OpError::failed(format!("Couldn't put the new version in place: {e}")));
    }
    Ok(())
}

/// Runs a file step again for a few seconds while something else holds the
/// file (an antivirus scanning it, a backup or sync app reading it).
fn retrying(mut step: impl FnMut() -> std::io::Result<()>) -> std::io::Result<()> {
    // 32: the file is in use by another program.
    let held = |e: &std::io::Error| e.kind() == std::io::ErrorKind::PermissionDenied || e.raw_os_error() == Some(32);
    let mut tries = 0;
    loop {
        match step() {
            Err(e) if held(&e) && tries < 8 => {
                tries += 1;
                std::thread::sleep(std::time::Duration::from_millis(if cfg!(test) { 1 } else { 500 }));
            }
            done => return done,
        }
    }
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
async fn local_identity() -> Result<sshee_core::identity::LocalIdentity, OpError> {
    blocking(|| Ok(sshee_core::identity::local())).await
}

/// AI coding agents installed on the server ("claude", "codex", …).
#[tauri::command]
async fn detect_agents(state: State<'_, AppState>, target: TargetSpec) -> Result<Vec<String>, OpError> {
    let (t, env) = (resolve(&state, &target)?, state.env.clone());
    blocking(move || agents::detect(&t, &env)).await
}

/// What every SSHeesh terminal on the server is running, in one round trip.
#[tauri::command]
async fn foreground(state: State<'_, AppState>, target: TargetSpec) -> Result<Vec<agents::Foreground>, OpError> {
    let (t, env) = (resolve(&state, &target)?, state.env.clone());
    blocking(move || agents::foreground(&t, &env)).await
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

/// Runs a check-up fix or a cleanup without a terminal, unless it needs you.
#[tauri::command]
async fn run_fix(state: State<'_, AppState>, target: TargetSpec, command: String) -> Result<fixes::RunOutcome, OpError> {
    let (t, env) = (resolve(&state, &target)?, state.env.clone());
    blocking(move || fixes::run_unattended(&t, &env, &command)).await
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

#[tauri::command]
async fn find_path(state: State<'_, AppState>, target: TargetSpec, path: String, cwd: String) -> Result<String, OpError> {
    let (t, env) = (resolve(&state, &target)?, state.env.clone());
    blocking(move || files::find_path(&t, &env, &path, &cwd)).await
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

/// `full` also reads what hardly changes (name, cores, disks); see `metrics::fetch`.
#[tauri::command]
async fn fetch_metrics(state: State<'_, AppState>, target: TargetSpec, full: Option<bool>) -> Result<Metrics, OpError> {
    let (t, env) = (resolve(&state, &target)?, state.env.clone());
    blocking(move || metrics::fetch(&t, &env, full.unwrap_or(true))).await
}

// ---------------------------------------------------------------- tunnels

/// What listens on the server, and which of it answers like a website.
#[tauri::command]
async fn scan_web_apps(state: State<'_, AppState>, target: TargetSpec) -> Result<sshee_core::tunnels::WebAppScan, OpError> {
    let (t, env) = (resolve(&state, &target)?, state.env.clone());
    blocking(move || sshee_core::tunnels::scan(&t, &env)).await
}

/// Starts tunnel `id` (or, if it already runs the same way, reattaches to
/// it). Its state goes to `on_state`; a port here that's taken fails at once.
#[tauri::command]
async fn start_tunnel(
    state: State<'_, AppState>,
    tunnels: State<'_, sshee_core::tunnels::TunnelManager>,
    id: String,
    host_id: String,
    forward: sshee_core::tunnels::Forward,
    on_state: Channel<sshee_core::tunnels::TunnelState>,
) -> Result<(), OpError> {
    let t = ssh_target(&state, &host_id)?;
    let sink: sshee_core::tunnels::StateSink = Arc::new(move |s| {
        let _ = on_state.send(s);
    });
    tunnels.start(&id, &t, &state.env, forward, sink)
}

#[tauri::command]
fn stop_tunnel(tunnels: State<sshee_core::tunnels::TunnelManager>, id: String) {
    tunnels.stop(&id);
}

/// Why a tunnel couldn't listen on `port` on this computer, or null.
#[tauri::command]
fn local_port_problem(port: u16) -> Option<String> {
    sshee_core::tunnels::port_problem(port).map(|e| e.message)
}

/// A free port here for the server's `preferred` one: the same number when it's free.
#[tauri::command]
fn pick_local_port(preferred: u16, avoid: Vec<u16>) -> Result<u16, OpError> {
    sshee_core::tunnels::pick_local_port(preferred, &avoid)
}

/// Opens Windows 11's Snap Layouts for this window, as hovering the
/// maximize button of a system title bar does. With SSHeesh's own title bar
/// the system can't see that button, so it presses Win+Z, which shows the
/// same panel, then Alt, which hides the number hints Windows adds when the
/// panel comes from the keyboard. Elsewhere it does nothing.
#[tauri::command]
async fn show_snap_layouts() -> Result<(), OpError> {
    // It waits a moment between the two key presses: not on the UI thread.
    blocking(|| {
        press_snap_keys();
        Ok(())
    })
    .await
}

fn press_snap_keys() {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::{SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VK_LWIN, VK_MENU};
        let key = |vk: u16, up: bool| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: vk, wScan: 0, dwFlags: if up { KEYEVENTF_KEYUP } else { 0 }, time: 0, dwExtraInfo: 0 } },
        };
        let send = |inputs: &[INPUT]| unsafe {
            SendInput(inputs.len() as u32, inputs.as_ptr(), std::mem::size_of::<INPUT>() as i32);
        };
        const VK_Z: u16 = 0x5A;
        send(&[key(VK_LWIN, false), key(VK_Z, false), key(VK_Z, true), key(VK_LWIN, true)]);
        std::thread::sleep(std::time::Duration::from_millis(50));
        send(&[key(VK_MENU, false), key(VK_MENU, true)]);
    }
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
            app.manage(sshee_core::tunnels::TunnelManager::new());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_hosts,
            add_host,
            remove_host,
            forget_password,
            session_history,
            tmux_scroll,
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
            copy_to_server,
            cancel_transfer,
            detect_agents,
            foreground,
            show_snap_layouts,
            pane_info,
            security_checkup,
            run_fix,
            disk_report,
            disk_biggest,
            session_cwd,
            find_path,
            fetch_metrics,
            local_identity,
            is_portable,
            #[cfg(desktop)]
            install_portable_update,
            list_containers,
            container_action,
            scan_web_apps,
            start_tunnel,
            stop_tunnel,
            local_port_problem,
            pick_local_port,
        ])
        .build(tauri::generate_context!())
        .expect("error while starting SSHeesh")
        .run(|app, event| {
            // Local ssh processes go away with the app; tmux sessions on the
            // servers stay, so reopening the app picks them back up.
            if let tauri::RunEvent::Exit = event {
                app.state::<AppState>().sessions.close_all();
                // Tunnels end with the app too; the ones you left on start again next time.
                app.state::<sshee_core::tunnels::TunnelManager>().stop_all();
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
        swap_in(&exe, b"new", "0.4.1").unwrap();
        assert_eq!(std::fs::read_to_string(&exe).unwrap(), "new");
        assert_eq!(std::fs::read_to_string(dir.join("SSHeesh_0.4.0_x64-portable.old.exe")).unwrap(), "old");
        assert!(!dir.join("SSHeesh_0.4.0_x64-portable.new.exe").exists());
        // A second update replaces the copy the first one set aside.
        swap_in(&exe, b"newer", "0.4.2").unwrap();
        assert_eq!(std::fs::read_to_string(&exe).unwrap(), "newer");
        assert_eq!(std::fs::read_to_string(dir.join("SSHeesh_0.4.0_x64-portable.old.exe")).unwrap(), "new");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_portable_update_that_cant_replace_the_running_copy_keeps_the_new_one() {
        let dir = std::env::temp_dir().join(format!("sshee-swap-gone-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // The running copy isn't where it was started from (moved while open).
        let err = swap_in(&dir.join("SSHeesh.exe"), b"new", "0.7.2").unwrap_err();
        assert!(err.message.contains("saved next to it as SSHeesh-0.7.2.exe"), "{}", err.message);
        assert_eq!(std::fs::read_to_string(dir.join("SSHeesh-0.7.2.exe")).unwrap(), "new");
        assert!(!dir.join("SSHeesh.new.exe").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn retries_a_file_another_program_holds() {
        let mut calls = 0;
        let held = retrying(|| {
            calls += 1;
            if calls < 3 { Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied)) } else { Ok(()) }
        });
        assert!(held.is_ok());
        assert_eq!(calls, 3);
        // A missing file isn't waited on.
        let mut calls = 0;
        let missing = retrying(|| {
            calls += 1;
            Err(std::io::Error::from(std::io::ErrorKind::NotFound))
        });
        assert!(missing.is_err());
        assert_eq!(calls, 1);
    }
}
