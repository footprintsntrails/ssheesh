//! The cards over a terminal's text about its session (0.10's `TerminalPane.tsx` overlays):
//! the question about a server's key before signing in (a new server, or a key that
//! changed), "Disconnected", "Reconnecting in 3s", "Disconnected after 20 min unused",
//! "Reconnecting…", "Back online" and "Couldn't connect" / "Session ended". Which card shows,
//! and when, is `terminal/session.rs` (`TermPane::card`); the words are `sshee_app::prompts`.
//!
//! ```text
//! term-box
//! ├── session-note   a pill at the top right (under the find bar while it's open):
//! │                  reconnecting, asleep, back online; away while something is open over
//! │                  the window
//! └── session-card   a card in the middle: the key's question, a session that stopped
//! ```
//!
//! The terminal keeps the keyboard: Return gives a card its default answer and Escape the safe
//! one (`TermPane::card_key`), a key wakes a terminal that let go of its connection.

use rune::{col, el, row, text, Align, El};
use sshee_app::prompts::{self, Fingerprint, HostKey};

use crate::icons::{icon, Icon};
use crate::kit::popover::popover;
use crate::kit::push::{push_button, Push};
use crate::terminal::overlays::Wish;
use crate::terminal::session::Card;
use crate::terminal::TermPane;
use crate::theme::{self, Theme};

/// The card over `pane`'s text, if its session has one.
pub fn card<S: 'static>(pane: &TermPane, t: &'static Theme, get: impl Fn(&mut S) -> Option<&mut TermPane> + Copy + 'static) -> Option<El> {
    let card = pane.card()?;
    // A pill under the palette or the agents' list would peek out above it: it waits.
    let pill = !matches!(card, Card::HostKey(_) | Card::Stopped { .. });
    if pill && pane.covered() {
        return None;
    }
    let top = if pane.find_is_open() { NOTE_UNDER_FIND } else { NOTE_TOP };
    Some(match card {
        Card::HostKey(question) => host_key(&question, &pane.over.session.server_name, t, get),
        Card::Disconnected { reason } => {
            let always = text("Always reconnect automatically")
                .key("card-always")
                .font(theme::SMALL)
                .color(t.l2)
                .single_line()
                .role("button")
                .on_press_tx(move |s: &mut S, tx| {
                    if let Some(p) = get(s) {
                        p.over.wish(Wish::AlwaysReconnect);
                        p.reconnect();
                        p.focus_terminal(tx);
                    }
                });
            note(t, top, dot(t.orange), col().min_w(0.0).shrink().child(line(t, &reason, true)).child(always), Some(again(t, "Reconnect", get)))
        }
        Card::Asleep { minutes } => note(
            t,
            top,
            icon(Icon::Terminal, 14.0, t.l2),
            col().min_w(0.0).shrink().child(line(t, &prompts::asleep_title(minutes), true)).child(line(t, prompts::ASLEEP_LINE, false)),
            Some(again(t, "Reconnect", get)),
        ),
        Card::Retrying { reason, line: when } => note(
            t,
            top,
            dot(t.orange),
            col().min_w(0.0).shrink().child(line(t, &reason, true)).child(text(when).key("card-countdown").font(theme::VALUE).color(t.l2).single_line()),
            Some(again(t, "Now", get)),
        ),
        // The small kin of "Connecting to …" (`terminal/connecting.rs`): the text stays seen.
        Card::Rejoining => note(t, top, crate::icons::logo_loader(16.0), line(t, "Reconnecting…", true), None),
        Card::Back { durable } => note(
            t,
            top,
            dot(t.green),
            row().gap(6.0).child(line(t, "Back online", true)).child(text(prompts::back_online_line(durable)).font(theme::NOTE).color(t.l2).single_line()),
            None,
        ),
        Card::Stopped { title, body, again: label, failed, .. } => {
            let mark = if failed { t.red } else { t.l2 };
            let close = push_button(t, "Close", Push::Quiet, true).key("card-close").on_press_tx(move |s: &mut S, _| {
                if let Some(p) = get(s) {
                    p.over.wish(Wish::CloseTab);
                }
            });
            let start = push_button(t, label, Push::Primary, true).key("card-again").on_press_tx(move |s: &mut S, tx| {
                if let Some(p) = get(s) {
                    p.restart();
                    p.focus_terminal(tx);
                }
            });
            centered(
                popover(t, 360.0)
                    .key("session-card")
                    .padding(18.0)
                    .gap(14.0)
                    .label(title)
                    .child(row().gap(10.0).items_start().child(icon(Icon::Warning, 16.0, mark).mt(1.0)).child(
                        col().gap(4.0).grow(1.0).min_w(0.0).child(text(title).font(theme::TITLE).color(t.l1).wrap().w_full()).child(text(body).key("card-reason").font(theme::NOTE).color(t.l2).wrap().w_full()),
                    ))
                    .child(row().gap(8.0).justify_end().child(close).child(start)),
            )
        }
    })
}

/// The question about the server's key: the fingerprints, and Trust or not.
fn host_key<S: 'static>(q: &HostKey, name: &str, t: &'static Theme, get: impl Fn(&mut S) -> Option<&mut TermPane> + Copy + 'static) -> El {
    let (yes, no) = q.answers();
    let changed = q.changed();
    let tone = if changed { t.red } else { t.blue };
    let title = q.title(if name.is_empty() { "this server" } else { name });
    let answer = move |trust: bool| {
        move |s: &mut S, tx: &mut rune::Tx| {
            if let Some(p) = get(s) {
                p.answer_host_key(trust, tx);
            }
        }
    };
    // The safe answer is the default (the blue one) when the key changed.
    let (trust, cancel) = if changed {
        (push_button(t, yes, Push::Caution, true), push_button(t, no, Push::Primary, true))
    } else {
        (push_button(t, yes, Push::Primary, true), push_button(t, no, Push::Quiet, true))
    };
    // Wide enough for a SHA256 fingerprint on one line beside its kind, in a 900 point window.
    let mut card = popover(t, 500.0)
        .key("hostkey-card")
        .padding(18.0)
        .gap(12.0)
        .label(title.clone())
        .child(
            row().gap(12.0).items_start().child(
                el().size(30.0, 30.0).radius(15.0).fill(tone.alpha(0.14)).items_center().justify_center().child(icon(if changed { Icon::Warning } else { Icon::Shield }, 15.0, tone)),
            ).child(
                col().gap(4.0).grow(1.0).min_w(0.0)
                    .child(text(title).key("hostkey-title").font(theme::TITLE).color(t.l1).wrap().w_full())
                    .child(text(q.body()).font(theme::NOTE).color(t.l2).wrap().w_full()),
            ),
        );
    card = match q {
        HostKey::New { fingerprints } => card.child(prints(t, "Server fingerprint", fingerprints, false)),
        HostKey::Changed { old, new } => card.child(prints(t, "Before", old, true)).child(prints(t, "Now", new, false)),
    };
    if changed {
        card = card.border(1.0, t.red.alpha(0.5));
    }
    card = card.child(row().gap(8.0).justify_end().mt(4.0).child(cancel.key("hostkey-no").on_press_tx(answer(false))).child(trust.key("hostkey-yes").on_press_tx(answer(true))));
    centered(card)
}

/// A list of fingerprints under a label; the old ones struck through.
fn prints(t: &Theme, label: &str, list: &[Fingerprint], old: bool) -> El {
    let mut c = col().gap(4.0).child(text(label).font(theme::GROUP).color(t.l2).single_line());
    for (i, f) in list.iter().enumerate() {
        let ink = if old { t.l2 } else { t.l1 };
        let mut sha = text(f.sha256.as_str()).font(theme::CODE).color(ink).wrap_anywhere().grow(1.0).min_w(0.0);
        if old {
            sha = sha.strike(0, f.sha256.len(), Some(t.l2));
        }
        c = c.child(
            row().key(format!("{}-{i}", if old { "print-old" } else { "print" })).gap(8.0).px(10.0).py(6.0).radius(theme::RADIUS_ROW).fill(t.hov)
                .child(text(if f.kind.is_empty() { "key" } else { f.kind.as_str() }).font(theme::SMALL).color(t.l2).single_line())
                .child(sha),
        );
    }
    c
}

/// How far down the pane the pill sits, and how far while the find bar is open (its top, its
/// height and a gap: `terminal/find.rs`).
const NOTE_TOP: f32 = 8.0;
const NOTE_UNDER_FIND: f32 = 8.0 + 36.0 + 8.0;

/// A pill at the top right of the terminal, where the find bar opens: a mark, its words,
/// maybe a button. At the right, a terminal's first line seldom reaches it, and it stays clear
/// of what hangs from the title in the middle.
fn note(t: &Theme, top: f32, mark: El, words: El, button: Option<El>) -> El {
    let mut pill = row().key("session-note").gap(10.0).items_center().py(7.0).pl(12.0).pr(if button.is_some() { 7.0 } else { 14.0 }).max_w(560.0)
        .radius(theme::RADIUS_POPOVER).fill(t.pop).border(1.0, t.rim).shadow(4.0, 16.0, rune::Color::BLACK.alpha(0.22)).role("status")
        .child(mark).child(words.shrink().min_w(0.0));
    if let Some(b) = button {
        pill = pill.child(b);
    }
    // Lined up with the find bar's right edge, clear of the scroll bar.
    row().key("session-note-row").absolute(0.0, top).w_full().justify_end().pl(12.0).pr(20.0).child(pill)
}

fn centered(card: El) -> El {
    col().key("session-center").absolute(0.0, 0.0).w_full().h_full().items_center().justify_center().padding(24.0).child(card)
}

fn line(t: &Theme, words: &str, strong: bool) -> El {
    let style = if strong { theme::TEXT_BOLD } else { theme::NOTE };
    text(words).font(style).color(if strong { t.l1 } else { t.l2 }).single_line().truncate().min_w(0.0).align(Align::Start)
}

fn dot(color: rune::Color) -> El {
    el().size(8.0, 8.0).radius(4.0).fill(color)
}

/// "Reconnect", "Now": connects again at once and gives the keyboard back to the terminal.
fn again<S: 'static>(t: &Theme, label: &str, get: impl Fn(&mut S) -> Option<&mut TermPane> + Copy + 'static) -> El {
    push_button(t, label, Push::Quiet, true).key("card-reconnect").on_press_tx(move |s: &mut S, tx| {
        if let Some(p) = get(s) {
            p.reconnect();
            p.focus_terminal(tx);
        }
    })
}

// ---- what a pane's wishes do in the window (`shell/tabs.rs`)

/// A new terminal on `from`'s machine that types `command` once connected ("New terminal
/// here", an agent): beside `from` when `beside`.
pub(crate) fn new_terminal_with(s: &mut crate::shell::Shell, from: sshee_app::workbench::TabId, command: String, beside: bool, tx: &mut rune::Tx) {
    let Some(server) = s.bench.tab(from).map(|t| t.server.clone()) else { return };
    s.bench.focus(from);
    let new = s.bench.open(sshee_app::workbench::TabKind::Terminal, &server);
    if beside {
        s.bench.pair(from, new);
    }
    s.settle(tx);
    if let Some(pane) = s.panes.get_mut(&new) {
        pane.run_when_connected(command);
    }
}

/// You trusted a server's key in `from`: its other terminals waiting on the same question
/// start too. True when one did.
pub(crate) fn trusted(s: &mut crate::shell::Shell, from: sshee_app::workbench::TabId) -> bool {
    let Some(server) = s.bench.tab(from).map(|t| t.server.clone()) else { return false };
    let waiting: Vec<_> = s.bench.tabs_on(&server).map(|t| t.id).filter(|id| *id != from).collect();
    let mut any = false;
    for id in waiting {
        if let Some(pane) = s.panes.get_mut(&id).filter(|p| matches!(p.status, crate::terminal::LinkStatus::HostKey(_))) {
            pane.restart();
            any = true;
        }
    }
    any
}

/// A clicked path that was found: Files on the same machine shows `dir` beside the terminal,
/// with `name` picked out and opened (`None`: the folder itself), scrolled to `lines` and
/// marked there.
pub(crate) fn show_path(s: &mut crate::shell::Shell, from: sshee_app::workbench::TabId, dir: &str, name: Option<&str>, lines: Option<(u32, u32)>, tx: &mut rune::Tx) {
    let Some(server) = s.bench.tab(from).map(|t| t.server.clone()) else { return };
    let files = s.open_files_on(&server, None, tx);
    match name {
        // Opened at the lines it named, marked (`screens::files::show_path_at`).
        Some(name) => crate::screens::files::show_path_at(s, files, &sshee_app::paths::join(dir, name), lines),
        None => crate::screens::files::go(s, files, dir),
    }
    // Beside the terminal it was clicked in, as 0.10's `placeBeside`.
    if !s.bench.together(from, files) {
        s.bench.pair(from, files);
    }
    s.settle(tx);
}

// ---- the agents' buttons

/// The agents the terminal's server has, where 0.10 showed them: in the terminal's header,
/// which is the top row for the tab on screen and a pane's own header beside another. Once
/// found they stay in sight, one quiet button each in the agent's own mark: a press starts it
/// here when the shell waits at its prompt, else in a new terminal beside, in the same folder.
/// `size`: each button's side, the header's own.
pub(crate) fn agent_starters(s: &crate::shell::Shell, id: sshee_app::workbench::TabId, size: f32, t: &'static Theme) -> Option<El> {
    let pane = s.panes.get(&id)?;
    let agents = pane.over.session.agents.as_ref().filter(|a| !a.is_empty())?;
    if !matches!(pane.status, crate::terminal::LinkStatus::Connected { .. }) {
        return None;
    }
    let mut bar = row().key(format!("agents-{}", id.0)).gap(1.0).items_center().role("group").label_static("AI agents on this server");
    for agent in agents.iter().copied() {
        let name = format!("Start {} here", agent.name());
        bar = bar.child(
            el().key(format!("agent-{}-{}", agent.command(), id.0))
                .size(size, size)
                .radius(if size < theme::CONTROL { theme::RADIUS_ROW } else { theme::RADIUS })
                .items_center()
                .justify_center()
                .hover(t.hov2)
                .press(t.down)
                .focusable(true)
                .role("button")
                .label(name.clone())
                .help(name)
                .on_press_tx(move |s: &mut crate::shell::Shell, tx| {
                    if let Some(p) = s.panes.get_mut(&id) {
                        p.start_agent(agent, tx);
                    }
                    tx.instant();
                })
                .child(agent_mark(agent, (size * 0.57).round(), t)),
        );
    }
    Some(bar)
}

/// An agent's mark, as 0.10 drew them: Claude's from Simple Icons (CC0), OpenCode's and
/// oh-my-pi's from their own repositories (MIT), Codex's after LobeHub Icons (MIT), in one
/// colour here.
fn agent_mark(agent: sshee_app::agents::AgentKind, size: f32, t: &Theme) -> El {
    use crate::svg::{path, Fit};
    use sshee_app::agents::AgentKind;
    let hex = |h: &str| rune::color::hex(h).unwrap_or(t.l1);
    let shape = match agent {
        AgentKind::Claude => rune::Shape::new().fill_path(&path(CLAUDE, Fit::square(0.0, 0.0, 24.0, size)), hex("#D97757")),
        AgentKind::OpenCode => {
            let fit = Fit::square(64.0, 64.0, 384.0, size);
            rune::Shape::new().fill_path(&path("M320 224V352H192V224H320Z", fit), t.l1.alpha(0.45)).fill_path(&path("M384 416H128V96H384V416ZM320 160H192V352H320V160Z", fit), t.l1)
        }
        AgentKind::Omp => {
            let fit = Fit::square(5.0, -10.0, 110.0, size);
            rune::Shape::new()
                .fill_path(&path("M12 8h96a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H12a2 2 0 0 1-2-2v-8a2 2 0 0 1 2-2z", fit), t.l1)
                .fill_path(&path("M27 20h8a2 2 0 0 1 2 2v58a2 2 0 0 1-2 2h-8a2 2 0 0 1-2-2V22a2 2 0 0 1 2-2z", fit), t.l1)
                .fill_path(&path("M77 20h8a2 2 0 0 1 2 2v41a2 2 0 0 1-2 2h-8a2 2 0 0 1-2-2V22a2 2 0 0 1 2-2z", fit), t.l1)
                .fill_path(&path("M74 55h14a3 3 0 0 1 3 3v10a3 3 0 0 1-3 3H74a3 3 0 0 1-3-3V58a3 3 0 0 1 3-3z", fit), hex("#f97316"))
        }
        AgentKind::Codex => {
            let fit = Fit::square(0.0, 0.0, 24.0, size);
            rune::Shape::new()
                .fill_path(&path("M4.5 0h15A4.5 4.5 0 0 1 24 4.5v15a4.5 4.5 0 0 1-4.5 4.5h-15A4.5 4.5 0 0 1 0 19.5v-15A4.5 4.5 0 0 1 4.5 0z", fit), hex("#7A9DFF"))
                .fill_path(&path("M12.5 13.9h3.7a.65.65 0 1 1 0 1.3h-3.7a.65.65 0 1 1 0-1.3zM8.5 9.2a.64.64 0 0 1 .88.23l1.27 2.22a.64.64 0 0 1 0 .64l-1.45 2.46a.64.64 0 1 1-1.1-.65l1.27-2.14-1.1-1.92a.64.64 0 0 1 .23-.84z", fit), hex("#FFFFFF"))
        }
    };
    rune::canvas(shape).size(size, size).a11y_hidden()
}

/// Claude's mark (Simple Icons, CC0), on a 24-unit grid.
const CLAUDE: &str = "m4.7144 15.9555 4.7174-2.6471.079-.2307-.079-.1275h-.2307l-.7893-.0486-2.6956-.0729-2.3375-.0971-2.2646-.1214-.5707-.1215-.5343-.7042.0546-.3522.4797-.3218.686.0608 1.5179.1032 2.2767.1578 1.6514.0972 2.4468.255h.3886l.0546-.1579-.1336-.0971-.1032-.0972L6.973 9.8356l-2.55-1.6879-1.3356-.9714-.7225-.4918-.3643-.4614-.1578-1.0078.6557-.7225.8803.0607.2246.0607.8925.686 1.9064 1.4754 2.4893 1.8336.3643.3035.1457-.1032.0182-.0728-.164-.2733-1.3539-2.4467-1.445-2.4893-.6435-1.032-.17-.6194c-.0607-.255-.1032-.4674-.1032-.7285L6.287.1335 6.6997 0l.9957.1336.419.3642.6192 1.4147 1.0018 2.2282 1.5543 3.0296.4553.8985.2429.8318.091.255h.1579v-.1457l.1275-1.706.2368-2.0947.2307-2.6957.0789-.7589.3764-.9107.7468-.4918.5828.2793.4797.686-.0668.4433-.2853 1.8517-.5586 2.9021-.3643 1.9429h.2125l.2429-.2429.9835-1.3053 1.6514-2.0643.7286-.8196.85-.9046.5464-.4311h1.0321l.759 1.1293-.34 1.1657-1.0625 1.3478-.8804 1.1414-1.2628 1.7-.7893 1.36.0729.1093.1882-.0183 2.8535-.607 1.5421-.2794 1.8396-.3157.8318.3886.091.3946-.3278.8075-1.967.4857-2.3072.4614-3.4364.8136-.0425.0304.0486.0607 1.5482.1457.6618.0364h1.621l3.0175.2247.7892.522.4736.6376-.079.4857-1.2142.6193-1.6393-.3886-3.825-.9107-1.3113-.3279h-.1822v.1093l1.0929 1.0686 2.0035 1.8092 2.5075 2.3314.1275.5768-.3218.4554-.34-.0486-2.2039-1.6575-.85-.7468-1.9246-1.621h-.1275v.17l.4432.6496 2.3436 3.5214.1214 1.0807-.17.3521-.6071.2125-.6679-.1214-1.3721-1.9246L14.38 17.959l-1.1414-1.9428-.1397.079-.674 7.2552-.3156.3703-.7286.2793-.6071-.4614-.3218-.7468.3218-1.4753.3886-1.9246.3157-1.53.2853-1.9004.17-.6314-.0121-.0425-.1397.0182-1.4328 1.9672-2.1796 2.9446-1.7243 1.8456-.4128.164-.7164-.3704.0667-.6618.4008-.5889 2.386-3.0357 1.4389-1.882.929-1.0868-.0062-.1579h-.0546l-6.3385 4.1164-1.1293.1457-.4857-.4554.0608-.7467.2307-.2429 1.9064-1.3114Z";

#[cfg(test)]
mod tests;
