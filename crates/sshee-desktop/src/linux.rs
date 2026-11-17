//! Notifications on Linux: `org.freedesktop.Notifications` on the session bus, from a thread of
//! its own with a small tokio runtime (as Rune's tray and file dialogs do). The connection that
//! shows a notification hears its `ActionInvoked`, so a click comes back here; `routes.rs`
//! keeps which number is which tab.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::StreamExt;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use zbus::{Connection, Proxy, Variant};

use crate::escape;
use crate::routes::{Routes, ACTIONS};
use crate::{AppInfo, Note, Notifier, Sink, Support};

const DEST: &str = "org.freedesktop.Notifications";
const PATH: &str = "/org/freedesktop/Notifications";
/// How long the bus may take to answer a call.
const RPC_TIMEOUT: Duration = Duration::from_secs(5);
/// A generic terminal icon the icon themes have: the preview isn't installed with one of its own.
const ICON: &str = "utilities-terminal";
/// Longest title and body handed over: servers cut them anyway.
const TITLE_MAX: usize = 120;
const BODY_MAX: usize = 400;

enum Command {
    Show(Note),
    Withdraw(u64),
}

pub(crate) struct Freedesktop {
    commands: UnboundedSender<Command>,
    support: Arc<Mutex<Support>>,
}

impl Freedesktop {
    pub(crate) fn start(app: &AppInfo, sink: Sink) -> Freedesktop {
        let (commands, receiver) = unbounded_channel();
        let support = Arc::new(Mutex::new(Support::Full));
        let (name, said) = (app.name.clone(), support.clone());
        let spawned = std::thread::Builder::new().name("notifications".into()).spawn(move || {
            match tokio::runtime::Builder::new_current_thread().enable_all().build() {
                Ok(runtime) => runtime.block_on(serve(name, receiver, sink, said)),
                Err(_) => *said.lock().unwrap() = Support::None("The notifications' thread couldn't start."),
            }
        });
        if spawned.is_err() {
            *support.lock().unwrap() = Support::None("The notifications' thread couldn't start.");
        }
        Freedesktop { commands, support }
    }
}

impl Notifier for Freedesktop {
    fn show(&mut self, note: &Note) {
        let _ = self.commands.send(Command::Show(note.clone()));
    }

    fn withdraw(&mut self, tab: u64) {
        let _ = self.commands.send(Command::Withdraw(tab));
    }

    fn support(&self) -> Support {
        *self.support.lock().unwrap()
    }
}

async fn serve(app: String, mut commands: UnboundedReceiver<Command>, sink: Sink, support: Arc<Mutex<Support>>) {
    let setup = async {
        let conn = Connection::session().await?;
        let proxy = Proxy::new(&conn, DEST, PATH, DEST).await?;
        // Listen before anything is shown: a click can come at once.
        let invoked = proxy.receive_signal("ActionInvoked").await?;
        let tokens = proxy.receive_signal("ActivationToken").await?;
        let closed = proxy.receive_signal("NotificationClosed").await?;
        let caps: Vec<String> = proxy.call("GetCapabilities", &()).await?;
        Ok::<_, zbus::Error>((conn, proxy, invoked, tokens, closed, caps))
    };
    let (conn, proxy, mut invoked, mut tokens, mut closed, caps) = match tokio::time::timeout(RPC_TIMEOUT, setup).await {
        Ok(Ok(v)) => v,
        _ => {
            *support.lock().unwrap() = Support::None("No notification service answers on this desktop's session bus.");
            return;
        }
    };
    let markup = caps.iter().any(|c| c == "body-markup");
    let actions = caps.iter().any(|c| c == "actions");
    if !actions {
        *support.lock().unwrap() = Support::NoClick;
    }
    let mut routes = Routes::default();
    loop {
        tokio::select! {
            command = commands.recv() => match command {
                None => break,
                Some(Command::Show(note)) => {
                    let title = escape::clip(&note.title, TITLE_MAX);
                    let body = escape::clip(&note.body, BODY_MAX);
                    let body = if markup { escape::freedesktop_body(&body) } else { body };
                    let offered: Vec<&str> = if actions { ACTIONS.to_vec() } else { Vec::new() };
                    let mut hints: HashMap<&str, Variant<'_>> = HashMap::new();
                    hints.insert("urgency", Variant::U8(1));
                    let args = (app.as_str(), routes.replaces(note.tab), ICON, title.as_str(), body.as_str(), offered, hints, -1i32);
                    if let Ok(Ok(id)) = tokio::time::timeout(RPC_TIMEOUT, proxy.call::<_, _, u32>("Notify", &args)).await {
                        routes.shown(note.tab, id);
                    }
                }
                Some(Command::Withdraw(tab)) => {
                    if let Some(id) = routes.withdraw(tab) {
                        let _ = tokio::time::timeout(RPC_TIMEOUT, proxy.call::<_, _, ()>("CloseNotification", &(id,))).await;
                    }
                }
            },
            Some(message) = tokens.next() => {
                if let Ok((id, token)) = message.body().deserialize::<(u32, String)>() {
                    routes.token(id, token);
                }
            }
            Some(message) = invoked.next() => {
                if let Ok((id, action)) = message.body().deserialize::<(u32, String)>() {
                    if let Some(event) = routes.invoked(id, &action) {
                        sink(event);
                    }
                }
            }
            Some(message) = closed.next() => {
                if let Ok((id, reason)) = message.body().deserialize::<(u32, u32)>() {
                    routes.closed(id, reason);
                }
            }
        }
    }
    let _ = conn.close().await;
}
