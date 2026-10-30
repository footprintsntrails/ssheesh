//! Server health: CPU, memory, load, network traffic and disks.
//!
//! Read straight from Linux's /proc over the existing ssh connection, so
//! nothing has to be installed on the server. Returns raw counters; the UI
//! turns two snapshots into rates (CPU %, MB/s).

use crate::error::{ErrorCode, OpError};
use crate::ssh::{run_local, run_remote, SshEnv, Target};
use serde::Serialize;

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Metrics {
    pub at_ms: u64,
    pub hostname: String,
    pub kernel: String,
    pub cores: u32,
    pub uptime_secs: u64,
    pub load: [f64; 3],
    /// Jiffies since boot, all CPUs.
    pub cpu_total: u64,
    pub cpu_idle: u64,
    /// Bytes.
    pub mem_total: u64,
    pub mem_available: u64,
    pub swap_total: u64,
    pub swap_free: u64,
    /// Bytes since boot, summed over physical interfaces.
    pub net_rx: u64,
    pub net_tx: u64,
    pub net_ifaces: Vec<String>,
    /// This month's traffic, when vnstat runs on the server.
    pub month: Option<MonthTraffic>,
    pub disks: Vec<Disk>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonthTraffic {
    pub month: String,
    pub rx: u64,
    pub tx: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Disk {
    pub mount: String,
    pub size: u64,
    pub used: u64,
}

const SCRIPT: &str = r#"export LC_ALL=C
echo @load; cat /proc/loadavg 2>/dev/null
echo @cpu; head -n1 /proc/stat 2>/dev/null
echo @mem; grep -E '^(MemTotal|MemAvailable|SwapTotal|SwapFree):' /proc/meminfo 2>/dev/null
echo @net; cat /proc/net/dev 2>/dev/null
echo @disk; df -kP 2>/dev/null
echo @uptime; cat /proc/uptime 2>/dev/null
echo @cores; nproc 2>/dev/null || grep -c ^processor /proc/cpuinfo 2>/dev/null
echo @host; hostname 2>/dev/null; uname -sr 2>/dev/null
echo @vnstat; command -v vnstat >/dev/null 2>&1 && vnstat --oneline b 2>/dev/null
exit 0"#;

pub fn fetch(target: &Target, env: &SshEnv) -> Result<Metrics, OpError> {
    let out = match target {
        Target::Ssh(t) => run_remote(t, env, SCRIPT, None)?,
        Target::Local => run_local(SCRIPT)?,
    };
    let mut m = parse(&String::from_utf8_lossy(&out));
    if m.cpu_total == 0 && m.mem_total == 0 {
        return Err(OpError::new(ErrorCode::Unsupported, "Stats work on Linux machines for now."));
    }
    m.at_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    Ok(m)
}

pub fn parse(text: &str) -> Metrics {
    let mut m = Metrics::default();
    let mut section = "";
    let mut host_lines = 0;
    let mut net_all: Vec<(String, u64, u64)> = Vec::new();
    for line in text.lines() {
        if let Some(name) = line.strip_prefix('@') {
            section = match name.trim() {
                "load" => "load",
                "cpu" => "cpu",
                "mem" => "mem",
                "net" => "net",
                "disk" => "disk",
                "uptime" => "uptime",
                "cores" => "cores",
                "host" => "host",
                "vnstat" => "vnstat",
                _ => "",
            };
            continue;
        }
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.is_empty() {
            continue;
        }
        match section {
            "load" if f.len() >= 3 => {
                for (i, v) in f.iter().take(3).enumerate() {
                    m.load[i] = v.parse().unwrap_or(0.0);
                }
            }
            "cpu" if f[0] == "cpu" => {
                let n: Vec<u64> = f[1..].iter().map(|v| v.parse().unwrap_or(0)).collect();
                // user nice system idle iowait irq softirq steal (guest is already in user)
                m.cpu_total = n.iter().take(8).sum();
                m.cpu_idle = n.get(3).copied().unwrap_or(0) + n.get(4).copied().unwrap_or(0);
            }
            "mem" if f.len() >= 2 => {
                let kb: u64 = f[1].parse().unwrap_or(0);
                match f[0] {
                    "MemTotal:" => m.mem_total = kb * 1024,
                    "MemAvailable:" => m.mem_available = kb * 1024,
                    "SwapTotal:" => m.swap_total = kb * 1024,
                    "SwapFree:" => m.swap_free = kb * 1024,
                    _ => {}
                }
            }
            "net" => parse_net_line(line, &mut net_all),
            "disk" if f.len() >= 6 && f[0] != "Filesystem" => {
                let fs_type_like = f[0];
                let mount = f[5..].join(" ");
                if is_real_disk(fs_type_like, &mount) {
                    let size = f[1].parse::<u64>().unwrap_or(0) * 1024;
                    let used = f[2].parse::<u64>().unwrap_or(0) * 1024;
                    if size > 0 {
                        m.disks.push(Disk { mount, size, used });
                    }
                }
            }
            "uptime" => m.uptime_secs = f[0].parse::<f64>().unwrap_or(0.0) as u64,
            "cores" => m.cores = f[0].parse().unwrap_or(0),
            "host" => {
                if host_lines == 0 {
                    m.hostname = line.trim().to_string();
                } else if host_lines == 1 {
                    m.kernel = line.trim().to_string();
                }
                host_lines += 1;
            }
            "vnstat" => m.month = parse_vnstat(line),
            _ => {}
        }
    }
    finish_net(&mut m, net_all);
    m.disks.sort_by(|a, b| b.size.cmp(&a.size));
    m.disks.truncate(4);
    m
}

fn is_virtual_iface(name: &str) -> bool {
    const VIRTUAL: [&str; 12] =
        ["docker", "veth", "br-", "virbr", "cni", "flannel", "cali", "kube", "tailscale", "wg", "tun", "zt"];
    // IPv6-in-IPv4 tunnels (sit0, he-ipv6, ...) carry traffic that also
    // shows up on the real interface.
    const TUNNELS: [&str; 5] = ["sit", "ip6tnl", "ipip", "gre", "6in4"];
    name == "lo"
        || VIRTUAL.iter().any(|p| name.starts_with(p))
        || TUNNELS.iter().any(|p| name.starts_with(p))
        || name.contains("ipv6")
}

fn parse_net_line(line: &str, all: &mut Vec<(String, u64, u64)>) {
    let Some((name, rest)) = line.split_once(':') else { return };
    let n: Vec<u64> = rest.split_whitespace().map(|v| v.parse().unwrap_or(0)).collect();
    if n.len() < 9 {
        return;
    }
    all.push((name.trim().to_string(), n[0], n[8]));
}

/// Counts only physical interfaces, so container and VPN traffic isn't
/// counted twice.
fn finish_net(m: &mut Metrics, all: Vec<(String, u64, u64)>) {
    let mut counted: Vec<&(String, u64, u64)> = all.iter().filter(|(n, _, _)| !is_virtual_iface(n)).collect();
    if counted.is_empty() {
        counted = all.iter().filter(|(n, _, _)| n != "lo").collect();
    }
    for (name, rx, tx) in counted {
        m.net_rx += rx;
        m.net_tx += tx;
        m.net_ifaces.push(name.clone());
    }
}

fn is_real_disk(fs: &str, mount: &str) -> bool {
    const SKIP_FS: [&str; 9] = ["tmpfs", "devtmpfs", "overlay", "squashfs", "udev", "none", "efivarfs", "shm", "run"];
    mount.starts_with('/')
        && !SKIP_FS.contains(&fs)
        && !mount.starts_with("/snap/")
        && !mount.starts_with("/run")
        && !mount.starts_with("/dev")
        && !mount.starts_with("/sys")
        && !mount.starts_with("/proc")
        && !mount.starts_with("/boot/efi")
        && !mount.starts_with("/var/lib/docker")
}

/// `vnstat --oneline b`: version;iface;today;rx;tx;total;rate;month;rx;tx;...
fn parse_vnstat(line: &str) -> Option<MonthTraffic> {
    let f: Vec<&str> = line.split(';').collect();
    if f.len() < 10 {
        return None;
    }
    Some(MonthTraffic { month: f[7].to_string(), rx: f[8].trim().parse().ok()?, tx: f[9].trim().parse().ok()? })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "@load
0.42 0.38 0.30 1/234 5678
@cpu
cpu  100 0 50 800 50 0 0 0 0 0
@mem
MemTotal:       16000000 kB
MemAvailable:   12000000 kB
SwapTotal:       2000000 kB
SwapFree:        2000000 kB
@net
Inter-|   Receive                                                |  Transmit
 face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets errs drop fifo colls carrier compressed
    lo: 999 1 0 0 0 0 0 0 999 1 0 0 0 0 0 0
  eth0: 1000 10 0 0 0 0 0 0 2000 20 0 0 0 0 0 0
docker0: 500 5 0 0 0 0 0 0 500 5 0 0 0 0 0 0
  sit0: 7 1 0 0 0 0 0 0 7 1 0 0 0 0 0 0
he-ipv6: 300 3 0 0 0 0 0 0 300 3 0 0 0 0 0 0
@disk
Filesystem     1024-blocks      Used Available Capacity Mounted on
/dev/sda1        100000000  30000000  70000000      30% /
tmpfs              1000000         0   1000000       0% /run/user/0
/dev/sdb1         50000000  10000000  40000000      20% /mnt/my data
@uptime
123456.78 999.00
@cores
8
@host
srv-1
Linux 6.8.0
@vnstat
1;eth0;2026-10-29;100;200;300;1 bit/s;2026-10;4000;5000;9000;2 bit/s;1;2;3
";

    #[test]
    fn parses_a_snapshot() {
        let m = parse(SAMPLE);
        assert_eq!(m.load, [0.42, 0.38, 0.30]);
        assert_eq!(m.cpu_total, 1000);
        assert_eq!(m.cpu_idle, 850);
        assert_eq!(m.mem_total, 16000000 * 1024);
        assert_eq!((m.net_rx, m.net_tx), (1000, 2000));
        assert_eq!(m.net_ifaces, vec!["eth0"]);
        assert_eq!(m.disks.len(), 2);
        assert_eq!(m.disks[1].mount, "/mnt/my data");
        assert_eq!(m.uptime_secs, 123456);
        assert_eq!(m.cores, 8);
        assert_eq!(m.hostname, "srv-1");
        assert_eq!(m.kernel, "Linux 6.8.0");
        let month = m.month.unwrap();
        assert_eq!((month.month.as_str(), month.rx, month.tx), ("2026-10", 4000, 5000));
    }
}
