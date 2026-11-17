//! A machine's Stats page without a screen: what it says on each line, and what it asks for
//! beyond the vital signs the status bar already has.
//!
//! A port of 0.10's `src/components/StatsPane.tsx` (one compact table: CPU, memory, network,
//! each disk, then the machine's name, kernel and uptime) with what 1.0 adds: the load on a
//! line of its own, graphs over the last five minutes (`workbench::vitals::History`), and the
//! programs using the most CPU now.
//!
//! The vital signs come from the window's watcher (one look every two seconds at the machine on
//! screen, sshee-core's `metrics::fetch`); the slow facts (name, kernel, cores, every disk, the
//! month's traffic) from the same looks as [`Facts`]. Programs are asked for only while a Stats
//! tab is on screen ([`Ask`]), every [`PROCESSES_EVERY`] seconds.

use crate::files::{Failure, FailureKind};
use crate::workbench::vitals::{bytes, percent, rate, uptime, Vitals, FULL};
use crate::workbench::{ServerKey, TabId};

/// Between two looks at the programs while a Stats tab is on screen, in seconds.
pub const PROCESSES_EVERY: f64 = 5.0;
/// How many programs the page lists.
pub const TOP: usize = 6;

/// What a full look tells beyond the vital signs (sshee-core's `Metrics`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Facts {
    /// The machine's own name for itself (`hostname`).
    pub hostname: String,
    /// "Linux 6.8.0".
    pub kernel: String,
    pub cores: u32,
    /// Over 1, 5 and 15 minutes.
    pub load: [f64; 3],
    /// Bytes.
    pub mem_total: u64,
    pub mem_available: u64,
    pub swap_total: u64,
    pub swap_free: u64,
    /// Bytes since the machine started.
    pub net_rx: u64,
    pub net_tx: u64,
    /// This month's traffic, when vnstat runs there.
    pub month: Option<Month>,
    /// The biggest disks, biggest first.
    pub disks: Vec<Disk>,
    pub uptime_secs: u64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Month {
    /// "2026-10".
    pub month: String,
    pub rx: u64,
    pub tx: u64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Disk {
    pub mount: String,
    pub size: u64,
    pub used: u64,
}

/// A program running on the machine.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Process {
    pub pid: u32,
    /// Its share of one core, in percent, since it started (ps's `%CPU`).
    pub cpu: f64,
    /// Resident memory, in bytes.
    pub memory: u64,
    /// The program's name (ps's `comm`): no arguments, which may hold secrets.
    pub command: String,
}

// ---- what a Stats tab asks and hears

#[derive(Debug, Clone, PartialEq)]
pub struct Ask {
    pub tab: TabId,
    pub server: ServerKey,
    pub ticket: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Reply {
    pub tab: TabId,
    pub server: ServerKey,
    pub ticket: u64,
    pub result: Result<Vec<Process>, Failure>,
}

impl Ask {
    pub fn reply(&self, result: Result<Vec<Process>, Failure>) -> Reply {
        Reply { tab: self.tab, server: self.server.clone(), ticket: self.ticket, result }
    }
}

/// The programs as one Stats tab knows them.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Programs {
    ticket: u64,
    waiting: bool,
    list: Option<Vec<Process>>,
    failure: Option<Failure>,
}

impl Programs {
    /// A look starts: its ticket. None while one is still on its way.
    pub fn ask(&mut self) -> Option<u64> {
        if self.waiting {
            return None;
        }
        self.ticket += 1;
        self.waiting = true;
        Some(self.ticket)
    }

    /// A look came back; an older one is dropped.
    pub fn answered(&mut self, ticket: u64, result: Result<Vec<Process>, Failure>) -> bool {
        if ticket != self.ticket {
            return false;
        }
        self.waiting = false;
        match result {
            Ok(mut list) => {
                list.sort_by(|a, b| b.cpu.total_cmp(&a.cpu).then(b.memory.cmp(&a.memory)));
                list.truncate(TOP);
                self.list = Some(list);
                self.failure = None;
            }
            Err(f) => self.failure = Some(f),
        }
        true
    }

    pub fn list(&self) -> Option<&[Process]> {
        self.list.as_deref()
    }

    /// Why there's no list: said under the heading.
    pub fn note(&self) -> Option<String> {
        if self.list.as_ref().is_some_and(|l| !l.is_empty()) {
            return None;
        }
        match &self.failure {
            Some(f) if f.kind == FailureKind::Unsupported => Some("This machine doesn't say which programs run.".into()),
            Some(f) => Some(format!("Couldn't list the programs: {}", f.message.trim())),
            None if self.list.is_some() => Some("No programs to show.".into()),
            None => Some("Looking…".into()),
        }
    }
}

// ---- the page's words

/// Which graph goes beside a line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Graph {
    Cpu,
    Memory,
    /// Down and up, on one scale.
    Network,
}

/// One line of the page: what, how much, the detail under it, and what's drawn beside it.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    /// A fixed word for its element key: "cpu", "memory", "disk-0".
    pub key: String,
    pub label: String,
    pub value: String,
    pub detail: Option<String>,
    /// A meter (0 to 1), for what fills up: memory, a disk.
    pub meter: Option<f64>,
    pub graph: Option<Graph>,
    /// Past [`FULL`]: drawn bold with a warning sign, as in the status bar.
    pub warn: bool,
}

/// "2026-10" → "October".
pub fn month_name(yyyy_mm: &str) -> String {
    const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
    yyyy_mm.split_once('-').and_then(|(_, m)| m.parse::<usize>().ok()).and_then(|m| MONTHS.get(m.wrapping_sub(1))).map_or_else(|| yyyy_mm.to_owned(), |m| (*m).to_owned())
}

/// 0.42 → "0.42".
fn load(v: f64) -> String {
    format!("{v:.2}")
}

/// The page's lines, top to bottom, from the vital signs and, once a full look came, the facts.
pub fn lines(v: &Vitals, f: Option<&Facts>) -> Vec<Line> {
    let line = |key: &str, label: &str, value: String| Line { key: key.into(), label: label.into(), value, detail: None, meter: None, graph: None, warn: false };
    let mut out = Vec::new();

    let mut cpu = line("cpu", "CPU", v.cpu.map_or("…".into(), percent));
    cpu.graph = Some(Graph::Cpu);
    cpu.detail = f.filter(|f| f.cores > 0).map(|f| if f.cores == 1 { "1 core".to_owned() } else { format!("{} cores", f.cores) });
    out.push(cpu);

    if let Some(f) = f {
        let mut l = line("load", "Load", format!("{}  {}  {}", load(f.load[0]), load(f.load[1]), load(f.load[2])));
        l.detail = Some("Over 1, 5 and 15 minutes".into());
        out.push(l);
    }

    let mut mem = line("memory", "Memory", percent(v.memory));
    mem.meter = Some(v.memory);
    mem.graph = Some(Graph::Memory);
    mem.warn = v.memory >= FULL;
    if let Some(f) = f.filter(|f| f.mem_total > 0) {
        let used = f.mem_total.saturating_sub(f.mem_available);
        let mut d = format!("{} of {}", bytes(used as f64), bytes(f.mem_total as f64));
        if f.swap_total > 0 {
            d.push_str(&format!(", swap {} of {}", bytes(f.swap_total.saturating_sub(f.swap_free) as f64), bytes(f.swap_total as f64)));
        }
        mem.detail = Some(d);
    }
    out.push(mem);

    let speed = match (v.down, v.up) {
        (Some(d), Some(u)) => format!("↓ {}   ↑ {}", rate(d), rate(u)),
        _ => "…".into(),
    };
    let mut net = line("network", "Network", speed);
    net.graph = Some(Graph::Network);
    net.detail = f.map(|f| match &f.month {
        Some(m) => format!("{}: ↓ {}  ↑ {}", month_name(&m.month), bytes(m.rx as f64), bytes(m.tx as f64)),
        None => format!("Since it started: ↓ {}  ↑ {}", bytes(f.net_rx as f64), bytes(f.net_tx as f64)),
    });
    out.push(net);

    match f {
        Some(f) if !f.disks.is_empty() => {
            for (i, d) in f.disks.iter().enumerate() {
                let used = if d.size == 0 { 0.0 } else { (d.used as f64 / d.size as f64).clamp(0.0, 1.0) };
                let mut l = line(&format!("disk-{i}"), "Disk", percent(used));
                l.detail = Some(format!("{}: {} of {}", d.mount, bytes(d.used as f64), bytes(d.size as f64)));
                l.meter = Some(used);
                l.warn = used >= FULL;
                out.push(l);
            }
        }
        // Before a full look: the fullest disk the status bar knows.
        _ => {
            if let Some((used, mount)) = &v.disk {
                let mut l = line("disk-0", "Disk", percent(*used));
                l.detail = mount.clone();
                l.meter = Some(*used);
                l.warn = *used >= FULL;
                out.push(l);
            }
        }
    }
    out
}

/// The line under the table: "web-7731, Linux 6.8.0, up 37 d 5 h". `name` is how the window
/// may show the machine (its alias while streaming): the machine's own name is used only when
/// `own_name` allows it.
pub fn about(v: &Vitals, f: Option<&Facts>, name: &str, own_name: bool) -> String {
    let mut parts = Vec::new();
    match f.filter(|f| own_name && !f.hostname.is_empty()) {
        Some(f) => parts.push(f.hostname.clone()),
        None => parts.push(name.to_owned()),
    }
    if let Some(kernel) = f.map(|f| f.kernel.as_str()).filter(|k| !k.is_empty()) {
        parts.push(kernel.to_owned());
    }
    parts.push(format!("up {}", uptime(f.map_or(v.uptime_secs, |f| f.uptime_secs.max(v.uptime_secs)))));
    let mut s = parts.join(", ");
    if f.is_some_and(|f| f.month.is_none()) {
        s.push_str(". Install vnstat on the server for monthly traffic.");
    }
    s
}

/// A program's line: its name, CPU and memory.
pub fn program(p: &Process) -> (String, String, String) {
    (p.command.clone(), format!("{:.1}%", p.cpu), bytes(p.memory as f64))
}

/// What the sidebar's card shows of a machine's load: three meters, a warning, the uptime.
#[derive(Debug, Clone, PartialEq)]
pub struct Load {
    /// ("CPU", 0.38), ("Memory", 0.6), ("Disk /var", 0.92).
    pub meters: Vec<(String, f64)>,
    pub warning: Option<String>,
    /// "Up 37 d 5 h".
    pub uptime: String,
}

impl Load {
    pub fn of(v: &Vitals) -> Load {
        let mut meters = vec![("CPU".to_owned(), v.cpu.unwrap_or(0.0)), ("Memory".to_owned(), v.memory)];
        if let Some((used, mount)) = &v.disk {
            meters.push((mount.as_ref().map_or("Disk".to_owned(), |m| format!("Disk {m}")), *used));
        }
        Load { meters, warning: crate::workbench::vitals::warning(v), uptime: format!("Up {}", uptime(v.uptime_secs)) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vitals() -> Vitals {
        Vitals { cpu: Some(0.38), memory: 0.6, disk: Some((0.44, None)), uptime_secs: 37 * 86_400 + 5 * 3_600, down: Some(1.6 * 1_048_576.0), up: Some(315.0 * 1024.0) }
    }

    fn facts() -> Facts {
        Facts {
            hostname: "web-7731".into(),
            kernel: "Linux 6.8.0".into(),
            cores: 8,
            load: [0.42, 0.38, 0.3],
            mem_total: 8 << 30,
            mem_available: (8u64 << 30) * 2 / 5,
            swap_total: 2 << 30,
            swap_free: 2 << 30,
            net_rx: 120 << 30,
            net_tx: 30 << 30,
            month: None,
            disks: vec![Disk { mount: "/".into(), size: 100 << 30, used: 44 << 30 }, Disk { mount: "/var".into(), size: 50 << 30, used: 46 << 30 }],
            uptime_secs: 37 * 86_400 + 5 * 3_600,
        }
    }

    fn shown(lines: &[Line]) -> Vec<(String, String, Option<String>, bool)> {
        lines.iter().map(|l| (l.label.clone(), l.value.clone(), l.detail.clone(), l.warn)).collect()
    }

    #[test]
    fn the_page_reads_as_a_compact_table() {
        let f = facts();
        let lines = lines(&vitals(), Some(&f));
        let s = |a: &str, b: &str, c: Option<&str>, w: bool| (a.to_owned(), b.to_owned(), c.map(str::to_owned), w);
        assert_eq!(
            shown(&lines),
            [
                s("CPU", "38%", Some("8 cores"), false),
                s("Load", "0.42  0.38  0.30", Some("Over 1, 5 and 15 minutes"), false),
                s("Memory", "60%", Some("4.8 GB of 8.0 GB, swap 0 B of 2.0 GB"), false),
                s("Network", "↓ 1.6 MB/s   ↑ 315 KB/s", Some("Since it started: ↓ 120 GB  ↑ 30 GB"), false),
                s("Disk", "44%", Some("/: 44 GB of 100 GB"), false),
                s("Disk", "92%", Some("/var: 46 GB of 50 GB"), true),
            ]
        );
        assert_eq!(lines.iter().map(|l| l.key.as_str()).collect::<Vec<_>>(), ["cpu", "load", "memory", "network", "disk-0", "disk-1"]);
        assert_eq!(lines.iter().filter_map(|l| l.graph).collect::<Vec<_>>(), [Graph::Cpu, Graph::Memory, Graph::Network]);
    }

    #[test]
    fn before_a_full_look_it_says_what_the_status_bar_knows() {
        let mut v = vitals();
        v.cpu = None;
        v.down = None;
        let lines = lines(&v, None);
        assert_eq!(lines.iter().map(|l| (l.label.as_str(), l.value.as_str())).collect::<Vec<_>>(), [("CPU", "…"), ("Memory", "60%"), ("Network", "…"), ("Disk", "44%")]);
        assert!(lines.iter().all(|l| l.label != "Load"));
    }

    #[test]
    fn the_month_and_the_machine_read_in_words() {
        let mut f = facts();
        f.month = Some(Month { month: "2026-10".into(), rx: 4 << 30, tx: 5 << 30 });
        let net = lines(&vitals(), Some(&f)).into_iter().find(|l| l.key == "network").unwrap();
        assert_eq!(net.detail.as_deref(), Some("October: ↓ 4.0 GB  ↑ 5.0 GB"));
        assert_eq!(month_name("2026-13"), "2026-13");
        assert_eq!(month_name("odd"), "odd");
        assert_eq!(about(&vitals(), Some(&f), "Server 1", true), "web-7731, Linux 6.8.0, up 37 d 5 h");
        // While streaming, the machine's own name never shows: the window's alias does.
        assert_eq!(about(&vitals(), Some(&f), "Server 1", false), "Server 1, Linux 6.8.0, up 37 d 5 h");
        f.month = None;
        assert!(about(&vitals(), Some(&f), "web-7731", true).ends_with("Install vnstat on the server for monthly traffic."));
        assert_eq!(about(&vitals(), None, "web-7731", true), "web-7731, up 37 d 5 h");
    }

    #[test]
    fn programs_are_the_busiest_and_trouble_is_said() {
        let mut p = Programs::default();
        assert_eq!(p.note().as_deref(), Some("Looking…"));
        let t = p.ask().unwrap();
        // One look at a time.
        assert_eq!(p.ask(), None);
        let procs: Vec<Process> = (0..10).map(|i| Process { pid: i, cpu: i as f64, memory: 1 << 20, command: format!("p{i}") }).collect();
        assert!(p.answered(t, Ok(procs)));
        let names: Vec<&str> = p.list().unwrap().iter().map(|p| p.command.as_str()).collect();
        assert_eq!(names, ["p9", "p8", "p7", "p6", "p5", "p4"]);
        assert_eq!(p.note(), None);
        assert_eq!(program(&p.list().unwrap()[0]), ("p9".into(), "9.0%".into(), "1.0 MB".into()));
        let t = p.ask().unwrap();
        assert!(!p.answered(t + 1, Ok(vec![])));
        assert!(p.answered(t, Err(Failure::new(FailureKind::Unsupported, "no ps"))));
        // A list seen before stays; with none, the note says why.
        assert!(p.list().is_some());
        let mut q = Programs::default();
        let t = q.ask().unwrap();
        q.answered(t, Err(Failure::new(FailureKind::Unsupported, "no ps")));
        assert_eq!(q.note().as_deref(), Some("This machine doesn't say which programs run."));
    }

    #[test]
    fn the_card_shows_three_meters_and_a_warning() {
        let mut v = vitals();
        v.disk = Some((0.92, Some("/var".into())));
        let load = Load::of(&v);
        assert_eq!(load.meters, [("CPU".to_owned(), 0.38), ("Memory".to_owned(), 0.6), ("Disk /var".to_owned(), 0.92)]);
        assert_eq!(load.warning.as_deref(), Some("Disk /var 92% full"));
        assert_eq!(load.uptime, "Up 37 d 5 h");
    }
}
