//! Tauri glue: exposes sshee-core to the UI as commands.

use serde::Deserialize;
use sshee_core::containers::{self, ContainerList, Engine};
use sshee_core::files::{self, DirListing, TextFile};
use sshee_core::metrics::{self, Metrics};
use sshee_core::session::{self, EventSink};
use sshee_core::{ErrorCode, Host, HostStore, NewHost, OpError, SessionEvent, SessionManager, SessionSpec, SshEnv, Target};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::ipc::Channel;
use tauri::{Manager, State};

struct AppState {
    sessions: SessionManager,
    hosts: Mutex<HostStore>,
    env: SshEnv,
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
        TargetSpec::Host { host_id } => state
            .hosts
            .lock()
            .unwrap()
            .get(host_id)
            .map(|h| Target::Ssh(h.target()))
            .ok_or_else(|| OpError::new(ErrorCode::NotFound, "That server isn't in your list anymore.")),
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
fn add_host(state: State<AppState>, input: NewHost) -> Result<Host, String> {
    state.hosts.lock().unwrap().add(input)
}

#[tauri::command]
fn remove_host(state: State<AppState>, id: String) -> Result<(), String> {
    state.hosts.lock().unwrap().remove(&id)
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

/// Saves a server file or folder into Downloads; returns the local path.
#[tauri::command]
async fn download_path(state: State<'_, AppState>, target: TargetSpec, path: String, is_dir: bool) -> Result<String, OpError> {
    let (t, env) = (resolve(&state, &target)?, state.env.clone());
    let dest = dirs::download_dir().or_else(dirs::home_dir).unwrap_or_else(|| PathBuf::from("."));
    blocking(move || files::download(&t, &env, &path, is_dir, &dest)).await
}

#[tauri::command]
async fn upload_paths(
    state: State<'_, AppState>,
    target: TargetSpec,
    paths: Vec<String>,
    remote_dir: String,
    overwrite: bool,
) -> Result<(), OpError> {
    let (t, env) = (resolve(&state, &target)?, state.env.clone());
    blocking(move || {
        for p in paths {
            files::upload(&t, &env, &PathBuf::from(p), &remote_dir, overwrite)?;
        }
        Ok(())
    })
    .await
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
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            #[cfg(desktop)]
            {
                app.handle().plugin(tauri_plugin_updater::Builder::new().build())?;
                app.handle().plugin(tauri_plugin_process::init())?;
            }
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            app.manage(AppState {
                sessions: SessionManager::new(),
                hosts: Mutex::new(HostStore::new(&data_dir)),
                env: SshEnv::with_control_dir(sshee_core::ssh::default_control_dir()),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_hosts,
            add_host,
            remove_host,
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
            delete_path,
            download_path,
            upload_paths,
            session_cwd,
            fetch_metrics,
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
