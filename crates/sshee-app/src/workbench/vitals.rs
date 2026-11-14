//! A machine's vital signs for the status bar: CPU, memory, disk, uptime and network speed,
//! worked out from two looks at its counters, with the values that need a look marked.
//!
//! A port of the numbers in 0.10's `src/lib/metrics.ts` (CPU and network from the difference
//! between two looks), `src/components/ServerLoad.tsx` (the fullest disk; "full" is 90%) and
//! `src/lib/format.ts` (how they read).

/// Past this, a disk or memory is marked (0.10's `FULL`).
pub const FULL: f64 = 0.9;

/// One look at a machine's counters (sshee-core's `Metrics`, the fields the status bar reads).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Reading {
    /// When it was taken, in milliseconds since 1970.
    pub at_ms: u64,
    /// Jiffies since boot, all CPUs: every state, and idle.
    pub cpu_total: u64,
    pub cpu_idle: u64,
    /// Bytes.
    pub mem_total: u64,
    pub mem_available: u64,
    pub uptime_secs: u64,
    /// Bytes since boot, over the machine's own network cards.
    pub net_rx: u64,
    pub net_tx: u64,
    /// Every disk: where it's mounted, its size and what's used, in bytes. Empty in a quick
    /// look; [`Vitals::between`] then keeps the last full look's.
    pub disks: Vec<Disk>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Disk {
    pub mount: String,
    pub size: u64,
    pub used: u64,
}

/// What the status bar shows of the machine on screen. Fractions are 0 to 1.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Vitals {
    /// None until a second look: it's the share of time not idle between two.
    pub cpu: Option<f64>,
    pub memory: f64,
    /// The fullest disk: how full, and its mount unless it's `/`.
    pub disk: Option<(f64, Option<String>)>,
    pub uptime_secs: u64,
    /// Bytes a second, between two looks.
    pub down: Option<f64>,
    pub up: Option<f64>,
}

/// One value in the bar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Vital {
    pub label: String,
    pub value: String,
    /// Past its threshold: drawn bold with a warning sign.
    pub warn: bool,
}

impl Vitals {
    /// The vitals at `now`, with `before` (the look before it, if there was one) for what is a
    /// rate.
    pub fn between(before: Option<&Reading>, now: &Reading) -> Vitals {
        let memory = if now.mem_total == 0 { 0.0 } else { now.mem_total.saturating_sub(now.mem_available) as f64 / now.mem_total as f64 };
        let disks = if now.disks.is_empty() { before.map_or(&now.disks, |b| &b.disks) } else { &now.disks };
        let disk = disks
            .iter()
            .filter(|d| d.size > 0)
            .map(|d| (d.used as f64 / d.size as f64, d))
            .max_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(used, d)| (used.clamp(0.0, 1.0), (d.mount != "/").then(|| d.mount.clone())));
        let mut v = Vitals { cpu: None, memory: memory.clamp(0.0, 1.0), disk, uptime_secs: now.uptime_secs, down: None, up: None };
        if let Some(b) = before.filter(|b| now.at_ms > b.at_ms) {
            let dt = ((now.at_ms - b.at_ms) as f64 / 1000.0).max(0.001);
            let total = now.cpu_total.saturating_sub(b.cpu_total);
            if total > 0 {
                let idle = now.cpu_idle.saturating_sub(b.cpu_idle);
                v.cpu = Some((1.0 - idle as f64 / total as f64).clamp(0.0, 1.0));
            }
            v.down = Some(now.net_rx.saturating_sub(b.net_rx) as f64 / dt);
            v.up = Some(now.net_tx.saturating_sub(b.net_tx) as f64 / dt);
        }
        v
    }

    /// The bar's values, left to right. One that isn't known yet is left out.
    pub fn items(&self) -> Vec<Vital> {
        let mut out = Vec::new();
        let mut put = |label: &str, value: String, warn: bool| out.push(Vital { label: label.to_owned(), value, warn });
        if let Some(cpu) = self.cpu {
            put("CPU", percent(cpu), false);
        }
        put("Memory", percent(self.memory), self.memory >= FULL);
        if let Some((used, mount)) = &self.disk {
            let label = mount.as_ref().map_or("Disk".to_owned(), |m| format!("Disk {m}"));
            put(&label, percent(*used), *used >= FULL);
        }
        put("Uptime", uptime(self.uptime_secs), false);
        if let Some(down) = self.down {
            put("Down", rate(down), false);
        }
        if let Some(up) = self.up {
            put("Up", rate(up), false);
        }
        out
    }
}

/// 0.376 → "38%".
pub fn percent(fraction: f64) -> String {
    format!("{}%", (fraction.clamp(0.0, 1.0) * 100.0).round() as u32)
}

/// 3,215,100 s → "37 d 5 h"; under a day "5 h 12 min"; under an hour "12 min".
pub fn uptime(secs: u64) -> String {
    let (d, h, m) = (secs / 86_400, secs % 86_400 / 3_600, secs % 3_600 / 60);
    if d > 0 {
        format!("{d} d {h} h")
    } else if h > 0 {
        format!("{h} h {m} min")
    } else {
        format!("{m} min")
    }
}

const UNITS: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];

/// 1536 → "1.5 KB": one decimal under 10, none above (0.10's `bytes`).
pub fn bytes(n: f64) -> String {
    if !n.is_finite() || n < 1.0 {
        return "0 B".into();
    }
    let mut i = ((n.ln() / 1024f64.ln()).floor() as usize).min(UNITS.len() - 1);
    // "1020 KB" reads worse than "1.0 MB".
    if n / 1024f64.powi(i as i32) >= 1000.0 && i < UNITS.len() - 1 {
        i += 1;
    }
    let v = n / 1024f64.powi(i as i32);
    if v < 10.0 && i > 0 {
        format!("{v:.1} {}", UNITS[i])
    } else {
        format!("{} {}", v.round() as u64, UNITS[i])
    }
}

/// Bytes a second: "1.6 MB/s".
pub fn rate(bytes_per_sec: f64) -> String {
    format!("{}/s", bytes(bytes_per_sec))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reading(at_ms: u64) -> Reading {
        Reading {
            at_ms,
            cpu_total: 1_000,
            cpu_idle: 800,
            mem_total: 8 << 30,
            mem_available: (8u64 << 30) * 2 / 5,
            uptime_secs: 37 * 86_400 + 5 * 3_600 + 120,
            net_rx: 10_000_000,
            net_tx: 1_000_000,
            disks: vec![Disk { mount: "/".into(), size: 100, used: 44 }, Disk { mount: "/boot".into(), size: 100, used: 12 }],
        }
    }

    fn shown(v: &Vitals) -> Vec<(String, String, bool)> {
        v.items().into_iter().map(|i| (i.label, i.value, i.warn)).collect()
    }

    #[test]
    fn one_look_has_no_rates_yet() {
        let v = Vitals::between(None, &reading(1_000));
        assert_eq!(
            shown(&v),
            [("Memory", "60%", false), ("Disk", "44%", false), ("Uptime", "37 d 5 h", false)].map(|(a, b, c)| (a.to_owned(), b.to_owned(), c))
        );
    }

    #[test]
    fn two_looks_give_cpu_and_network_speed() {
        let before = reading(1_000);
        let mut now = reading(3_000);
        now.cpu_total += 400;
        now.cpu_idle += 248;
        now.net_rx += 2 * 1_677_722;
        now.net_tx += 2 * 322_560;
        // A quick look leaves the disks out: the last full look's stay.
        now.disks.clear();
        let v = Vitals::between(Some(&before), &now);
        assert_eq!(
            shown(&v),
            [
                ("CPU", "38%", false),
                ("Memory", "60%", false),
                ("Disk", "44%", false),
                ("Uptime", "37 d 5 h", false),
                ("Down", "1.6 MB/s", false),
                ("Up", "315 KB/s", false),
            ]
            .map(|(a, b, c)| (a.to_owned(), b.to_owned(), c))
        );
    }

    #[test]
    fn a_full_disk_or_memory_is_marked_and_the_fullest_disk_is_named() {
        let mut now = reading(1_000);
        now.disks.push(Disk { mount: "/var".into(), size: 1_000, used: 920 });
        now.mem_available = now.mem_total / 20;
        let v = Vitals::between(None, &now);
        let items = shown(&v);
        assert!(items.contains(&("Disk /var".into(), "92%".into(), true)), "{items:?}");
        assert!(items.contains(&("Memory".into(), "95%".into(), true)), "{items:?}");
        // Just under the line: not marked.
        now.disks = vec![Disk { mount: "/".into(), size: 1_000, used: 899 }];
        assert!(!Vitals::between(None, &now).items().iter().any(|i| i.label == "Disk" && i.warn));
    }

    #[test]
    fn counters_that_went_back_or_a_clock_that_stood_still_give_nothing_odd() {
        let before = reading(5_000);
        // The machine restarted between the looks: every counter is smaller.
        let mut now = reading(7_000);
        now.cpu_total = 10;
        now.cpu_idle = 5;
        now.net_rx = 0;
        now.net_tx = 0;
        let v = Vitals::between(Some(&before), &now);
        assert_eq!((v.cpu, v.down, v.up), (None, Some(0.0), Some(0.0)));
        // The same instant twice: no rates.
        let v = Vitals::between(Some(&before), &reading(5_000));
        assert_eq!((v.cpu, v.down, v.up), (None, None, None));
        // No memory or disks reported at all.
        let v = Vitals::between(None, &Reading::default());
        assert_eq!((v.memory, v.disk.clone()), (0.0, None));
    }

    #[test]
    fn sizes_and_times_read_as_in_0_10() {
        assert_eq!(bytes(0.0), "0 B");
        assert_eq!(bytes(1536.0), "1.5 KB");
        assert_eq!(bytes(1020.0 * 1024.0), "1.0 MB");
        assert_eq!(bytes(315.0 * 1024.0), "315 KB");
        assert_eq!(bytes(f64::NAN), "0 B");
        assert_eq!(rate(12.0), "12 B/s");
        assert_eq!(uptime(59), "0 min");
        assert_eq!(uptime(3 * 3_600 + 60), "3 h 1 min");
        assert_eq!(uptime(2 * 86_400), "2 d 0 h");
        assert_eq!(percent(1.7), "100%");
    }
}
