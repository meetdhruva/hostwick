//! Server metrics for the status bar: CPU, memory, disk and load, read
//! with one small `sh` script over the shared SSH connection (never on
//! telnet or serial), plus the smoothing used for the latency figure.
//!
//! Linux reads `/proc`; macOS uses `sysctl`, `vm_stat` and `ps`; other
//! systems report what they can (load, disk) and "n/a" for the rest.

use std::collections::HashMap;

/// The script, run through `hostwick_sftp::shell::portable` so it behaves
/// the same whatever the account's login shell is. Output is split into
/// `@section` blocks.
pub const SCRIPT: &str = r#"os=$(uname -s 2>/dev/null); echo "@os"; echo "$os"
case "$os" in
Linux) echo @stat; head -n 1 /proc/stat; echo @mem; grep -E '^(MemTotal|MemAvailable|MemFree|Buffers|Cached|SReclaimable):' /proc/meminfo; echo @load; cat /proc/loadavg;;
Darwin) echo @ncpu; sysctl -n hw.ncpu; echo @cpups; ps -A -o %cpu | awk 'NR>1{s+=$1} END{print s+0}'; echo @memsize; sysctl -n hw.memsize; echo @vmstat; vm_stat; echo @load; sysctl -n vm.loadavg;;
*) echo @load; sysctl -n vm.loadavg 2>/dev/null || uptime;;
esac
echo @df; df -Pk / 2>/dev/null | tail -n 1"#;

/// One reading. `None` = not available on this system.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Metrics {
    /// 0..=100.
    pub cpu: Option<f32>,
    /// (used, total) in bytes.
    pub mem: Option<(u64, u64)>,
    /// Root file system, 0..=100.
    pub disk_pct: Option<u8>,
    /// 1, 5 and 15 minute load averages.
    pub load: Option<[f32; 3]>,
}

impl Metrics {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// Turns raw outputs into [`Metrics`]; remembers the previous CPU counters
/// (Linux reports time since boot, the percentage is the change between two
/// readings).
#[derive(Debug, Default)]
pub struct Sampler {
    prev_cpu: Option<(u64, u64)>,
}

impl Sampler {
    pub fn sample(&mut self, output: &str) -> Metrics {
        let sec = sections(output);
        let get = |k: &str| sec.get(k).copied().unwrap_or("");
        let os = get("os").trim();
        let mut m = Metrics {
            disk_pct: parse_df(get("df")),
            ..Metrics::default()
        };
        match os {
            "Linux" => {
                if let Some(now) = parse_proc_stat(get("stat")) {
                    if let Some(prev) = self.prev_cpu.replace(now) {
                        m.cpu = cpu_percent(prev, now);
                    }
                }
                m.mem = parse_meminfo(get("mem"));
                m.load = parse_loadavg(get("load"));
            }
            "Darwin" => {
                let ncpu = get("ncpu").trim().parse::<f32>().ok().filter(|n| *n >= 1.0);
                m.cpu = get("cpups")
                    .trim()
                    .parse::<f32>()
                    .ok()
                    .zip(ncpu)
                    .map(|(sum, n)| (sum / n).clamp(0.0, 100.0));
                let total = get("memsize").trim().parse::<u64>().ok();
                m.mem = total.and_then(|t| parse_vm_stat(get("vmstat"), t).map(|u| (u, t)));
                m.load = parse_loadavg(&get("load").replace(['{', '}'], ""));
            }
            _ => {
                let l = get("load");
                // `uptime`: "... load averages: 0.52, 0.58, 0.59".
                let l = match l.find("load average") {
                    Some(i) => l[i..].split_once(':').map_or("", |(_, r)| r),
                    None => l,
                };
                m.load = parse_loadavg(&l.replace(['{', '}', ','], " "));
            }
        }
        m
    }

    /// Forget the CPU counters (after a reconnect).
    pub fn reset(&mut self) {
        self.prev_cpu = None;
    }
}

/// `@name` lines start a section.
fn sections(output: &str) -> HashMap<&str, &str> {
    let mut map = HashMap::new();
    let mut name: Option<&str> = None;
    let mut start = 0;
    let mut pos = 0;
    for line in output.split_inclusive('\n') {
        let t = line.trim_end();
        if let Some(n) = t.strip_prefix('@')
            && !n.is_empty()
            && n.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        {
            if let Some(prev) = name {
                map.insert(prev, &output[start..pos]);
            }
            name = Some(n);
            start = pos + line.len();
        }
        pos += line.len();
    }
    if let Some(prev) = name {
        map.insert(prev, &output[start..]);
    }
    map
}

/// `cpu  user nice system idle iowait irq softirq steal ...` → (total, idle).
pub fn parse_proc_stat(text: &str) -> Option<(u64, u64)> {
    let line = text.lines().find(|l| l.starts_with("cpu"))?;
    let n: Vec<u64> = line
        .split_whitespace()
        .skip(1)
        .map_while(|x| x.parse().ok())
        .collect();
    if n.len() < 4 {
        return None;
    }
    // guest time is already inside user/nice: leave the last two out.
    let total: u64 = n.iter().take(8).sum();
    let idle = n[3] + n.get(4).copied().unwrap_or(0);
    Some((total, idle))
}

/// Busy percentage between two `(total, idle)` readings.
pub fn cpu_percent(prev: (u64, u64), now: (u64, u64)) -> Option<f32> {
    let dt = now.0.checked_sub(prev.0)?;
    let di = now.1.checked_sub(prev.1)?;
    if dt == 0 {
        return None;
    }
    Some((100.0 * (dt.saturating_sub(di)) as f32 / dt as f32).clamp(0.0, 100.0))
}

/// (used, total) bytes from `/proc/meminfo` lines. Used = total minus what
/// is available (the kernel's own estimate, or free + buffers + cache on
/// kernels older than 3.14).
pub fn parse_meminfo(text: &str) -> Option<(u64, u64)> {
    let mut v: HashMap<&str, u64> = HashMap::new();
    for l in text.lines() {
        let (k, rest) = l.split_once(':')?;
        let kib: u64 = rest.split_whitespace().next()?.parse().ok()?;
        v.insert(k.trim(), kib);
    }
    let total = *v.get("MemTotal")?;
    let avail = match v.get("MemAvailable") {
        Some(a) => *a,
        None => {
            v.get("MemFree").copied().unwrap_or(0)
                + v.get("Buffers").copied().unwrap_or(0)
                + v.get("Cached").copied().unwrap_or(0)
                + v.get("SReclaimable").copied().unwrap_or(0)
        }
    };
    if total == 0 {
        return None;
    }
    Some((total.saturating_sub(avail) * 1024, total * 1024))
}

/// `0.52 0.58 0.59 1/467 12345`.
pub fn parse_loadavg(text: &str) -> Option<[f32; 3]> {
    let mut it = text.split_whitespace().filter_map(|x| x.parse::<f32>().ok());
    Some([it.next()?, it.next()?, it.next()?])
}

/// The last line of `df -Pk /`: `fs 1024-blocks used avail capacity mount`.
/// Parsed from the end, since the file system name may contain spaces.
pub fn parse_df(text: &str) -> Option<u8> {
    let line = text.lines().rev().find(|l| !l.trim().is_empty())?;
    let f: Vec<&str> = line.split_whitespace().collect();
    if f.len() < 5 {
        return None;
    }
    let cap = f[f.len() - 2];
    if let Some(p) = cap.strip_suffix('%') {
        return p.parse::<u8>().ok().map(|p| p.min(100));
    }
    // Some systems print no percentage: compute from used and available.
    let avail: f64 = f[f.len() - 3].parse().ok()?;
    let used: f64 = f[f.len() - 4].parse().ok()?;
    if used + avail <= 0.0 {
        return None;
    }
    Some((100.0 * used / (used + avail)).round().clamp(0.0, 100.0) as u8)
}

/// macOS `vm_stat`: used = active + wired + compressed pages.
pub fn parse_vm_stat(text: &str, total: u64) -> Option<u64> {
    let page: u64 = text
        .lines()
        .next()?
        .split("page size of")
        .nth(1)?
        .split_whitespace()
        .next()?
        .parse()
        .ok()?;
    let pages = |label: &str| -> u64 {
        text.lines()
            .find_map(|l| l.strip_prefix(label))
            .and_then(|r| r.trim().trim_end_matches('.').parse::<u64>().ok())
            .unwrap_or(0)
    };
    let used = pages("Pages active:")
        + pages("Pages wired down:")
        + pages("Pages occupied by compressor:");
    if used == 0 {
        return None;
    }
    Some((used * page).min(total))
}

/// `3.2 GB`, `512 MB`: binary units, one decimal under 10.
pub fn format_bytes(b: u64) -> String {
    const U: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = b as f64;
    let mut i = 0;
    while v >= 1024.0 && i < U.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 || v >= 10.0 {
        format!("{v:.0} {}", U[i])
    } else {
        format!("{v:.1} {}", U[i])
    }
}

/// Exponential moving average for the latency figure, so one slow
/// round trip does not make the number jump.
#[derive(Debug, Clone)]
pub struct Smoother {
    alpha: f32,
    value: Option<f32>,
}

impl Smoother {
    pub fn new(alpha: f32) -> Self {
        Self {
            alpha: alpha.clamp(0.01, 1.0),
            value: None,
        }
    }

    pub fn push(&mut self, sample: f32) -> f32 {
        let v = match self.value {
            None => sample,
            Some(v) => v + self.alpha * (sample - v),
        };
        self.value = Some(v);
        v
    }

    pub fn get(&self) -> Option<f32> {
        self.value
    }

    pub fn reset(&mut self) {
        self.value = None;
    }
}

impl Default for Smoother {
    fn default() -> Self {
        Self::new(0.3)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINUX_1: &str = "@os\nLinux\n@stat\ncpu  1000 10 500 8000 400 0 90 0 0 0\n@mem\nMemTotal:       16384000 kB\nMemFree:         1000000 kB\nMemAvailable:    8192000 kB\nBuffers:          200000 kB\nCached:          5000000 kB\nSReclaimable:     300000 kB\n@load\n0.52 0.58 0.59 1/467 12345\n@df\n/dev/vda1       41151808 20575904 18462720      53% /\n";
    const LINUX_2: &str = "@os\nLinux\n@stat\ncpu  1500 10 700 8400 500 0 90 0 0 0\n@mem\nMemTotal:       16384000 kB\nMemFree:         1000000 kB\nMemAvailable:    8192000 kB\n@load\n1.00 0.60 0.60 1/467 12346\n@df\n/dev/vda1       41151808 20575904 18462720      53% /\n";

    #[test]
    fn linux_sample() {
        let mut s = Sampler::default();
        let a = s.sample(LINUX_1);
        // The first reading has no CPU figure yet (needs a delta).
        assert_eq!(a.cpu, None);
        assert_eq!(a.mem, Some((8_192_000 * 1024, 16_384_000 * 1024)));
        assert_eq!(a.disk_pct, Some(53));
        assert_eq!(a.load, Some([0.52, 0.58, 0.59]));
        let b = s.sample(LINUX_2);
        let (p, n) = (
            parse_proc_stat("cpu  1000 10 500 8000 400 0 90 0 0 0"),
            parse_proc_stat("cpu  1500 10 700 8400 500 0 90 0 0 0"),
        );
        let (p, n) = (p.unwrap_or_default(), n.unwrap_or_default());
        assert_eq!(p, (10000, 8400));
        assert_eq!(n, (11200, 8900));
        // busy = (1200 - 500) / 1200
        let want = 100.0 * 700.0 / 1200.0;
        assert!((b.cpu.unwrap_or(-1.0) - want).abs() < 0.01, "{:?}", b.cpu);
        s.reset();
        assert_eq!(s.sample(LINUX_2).cpu, None);
    }

    #[test]
    fn meminfo_old_kernel_without_available() {
        let t = "MemTotal: 1000 kB\nMemFree: 100 kB\nBuffers: 50 kB\nCached: 150 kB\n";
        assert_eq!(parse_meminfo(t), Some((700 * 1024, 1000 * 1024)));
        assert_eq!(parse_meminfo(""), None);
        assert_eq!(parse_meminfo("MemTotal: 0 kB\n"), None);
    }

    #[test]
    fn counters_going_backwards_give_nothing() {
        assert_eq!(cpu_percent((100, 50), (100, 50)), None);
        assert_eq!(cpu_percent((100, 50), (90, 60)), None);
        assert_eq!(cpu_percent((0, 0), (100, 100)), Some(0.0));
        assert_eq!(cpu_percent((0, 0), (100, 0)), Some(100.0));
    }

    #[test]
    fn df_variants() {
        assert_eq!(parse_df("/dev/vda1 100 53 47 53% /\n"), Some(53));
        assert_eq!(parse_df("my fs name 100 53 47 53% /"), Some(53));
        assert_eq!(parse_df("fs 100 25 75 - /"), Some(25));
        assert_eq!(parse_df(""), None);
        assert_eq!(parse_df("df: /: no such\n"), None);
    }

    #[test]
    fn macos_sample() {
        let out = "@os\nDarwin\n@ncpu\n8\n@cpups\n120.5\n@memsize\n17179869184\n@vmstat\nMach Virtual Memory Statistics: (page size of 16384 bytes)\nPages free:                               20000.\nPages active:                            300000.\nPages inactive:                          280000.\nPages speculative:                         5000.\nPages wired down:                        150000.\nPages occupied by compressor:             50000.\n@load\n{ 2.10 1.90 1.50 }\n@df\n/dev/disk3s1s1 488245288 15000000 300000000 5% /\n";
        let m = Sampler::default().sample(out);
        assert!((m.cpu.unwrap_or(0.0) - 15.0625).abs() < 0.01);
        assert_eq!(m.mem, Some(((300_000 + 150_000 + 50_000) * 16384, 17_179_869_184)));
        assert_eq!(m.load, Some([2.10, 1.90, 1.50]));
        assert_eq!(m.disk_pct, Some(5));
    }

    #[test]
    fn other_systems_report_what_they_can() {
        let out = "@os\nFreeBSD\n@load\n{ 0.10 0.20 0.30 }\n@df\n/dev/ada0p2 100 10 90 10% /\n";
        let m = Sampler::default().sample(out);
        assert_eq!((m.cpu, m.mem), (None, None));
        assert_eq!(m.load, Some([0.10, 0.20, 0.30]));
        assert_eq!(m.disk_pct, Some(10));
        let up = "@os\nSunOS\n@load\n 10:20  up 3 days,  1 user,  load averages: 0.52, 0.58, 0.59\n@df\n";
        assert_eq!(Sampler::default().sample(up).load, Some([0.52, 0.58, 0.59]));
        assert!(Sampler::default().sample("garbage").is_empty());
        assert!(Sampler::default().sample("").is_empty());
    }

    #[test]
    fn byte_formatting() {
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1536), "1.5 KB");
        assert_eq!(format_bytes(8_192_000 * 1024), "7.8 GB");
        assert_eq!(format_bytes(16 * 1024 * 1024 * 1024), "16 GB");
    }

    #[test]
    fn smoothing() {
        let mut s = Smoother::new(0.5);
        assert_eq!(s.get(), None);
        assert_eq!(s.push(100.0), 100.0);
        assert_eq!(s.push(200.0), 150.0);
        assert_eq!(s.push(150.0), 150.0);
        // One outlier moves the figure by alpha only.
        let mut s = Smoother::default();
        for _ in 0..20 {
            s.push(20.0);
        }
        let v = s.push(1020.0);
        assert!((v - 320.0).abs() < 0.5, "{v}");
        s.reset();
        assert_eq!(s.get(), None);
    }

    /// The script, run through the portable wrapper in a real shell, yields
    /// output the parser understands.
    #[cfg(unix)]
    #[test]
    fn script_runs_locally() {
        let cmd = hostwick_sftp::shell::portable(SCRIPT);
        let out = std::process::Command::new("sh")
            .arg("-c")
            .arg(&cmd)
            .output()
            .unwrap_or_else(|e| panic!("sh: {e}"));
        let text = String::from_utf8_lossy(&out.stdout);
        let mut s = Sampler::default();
        s.sample(&text);
        let m = s.sample(&text);
        // Whatever this machine is, the disk and (on Linux) load are there.
        assert!(m.disk_pct.is_some(), "{text}");
        if cfg!(target_os = "linux") {
            assert!(m.load.is_some() && m.mem.is_some(), "{text}");
        }
    }
}
