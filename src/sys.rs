use std::fs;
use std::path::Path;
use std::time::Instant;

#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub cpu_pct: f32,
    pub mem_used: u64,
    pub mem_total: u64,
    pub rx_bps: f64,
    pub tx_bps: f64,
    pub temps: Vec<(String, f32)>,
    pub max_temp: Option<f32>,
    pub gpu_temp: Option<f32>,
    pub load1: f32,
    pub uptime: u64,
}

#[derive(Default)]
pub struct Collector {
    prev_cpu: Option<(u64, u64)>,
    prev_net: Option<(u64, u64)>,
    prev_time: Option<Instant>,
}

impl Collector {
    pub fn sample(&mut self) -> Snapshot {
        let now = Instant::now();
        let mut snap = Snapshot::default();

        if let Some((total, idle)) = cpu_times() {
            if let Some((prev_total, prev_idle)) = self.prev_cpu {
                let d_total = total.saturating_sub(prev_total) as f64;
                let d_idle = idle.saturating_sub(prev_idle) as f64;
                if d_total > 0.0 {
                    snap.cpu_pct = ((1.0 - d_idle / d_total) * 100.0).clamp(0.0, 100.0) as f32;
                }
            }
            self.prev_cpu = Some((total, idle));
        }

        if let Some((used, total)) = mem_info() {
            snap.mem_used = used;
            snap.mem_total = total;
        }

        if let Some((rx, tx)) = net_bytes() {
            if let (Some((prev_rx, prev_tx)), Some(prev)) = (self.prev_net, self.prev_time) {
                let secs = now.duration_since(prev).as_secs_f64().max(0.001);
                snap.rx_bps = rx.saturating_sub(prev_rx) as f64 / secs;
                snap.tx_bps = tx.saturating_sub(prev_tx) as f64 / secs;
            }
            self.prev_net = Some((rx, tx));
        }
        self.prev_time = Some(now);

        let temps = temperatures();
        snap.gpu_temp = temps
            .iter()
            .find(|(label, _)| {
                let lower = label.to_ascii_lowercase();
                lower.contains("gpu") || lower.contains("nvidia") || lower.contains("edge")
            })
            .map(|(_, value)| *value);
        snap.max_temp = temps.iter().map(|(_, v)| *v).reduce(f32::max);
        snap.temps = temps;

        if let Ok(text) = fs::read_to_string("/proc/loadavg")
            && let Some(first) = text.split_whitespace().next()
        {
            snap.load1 = first.parse().unwrap_or(0.0);
        }

        if let Ok(text) = fs::read_to_string("/proc/uptime")
            && let Some(first) = text.split_whitespace().next()
        {
            snap.uptime = first.parse::<f64>().unwrap_or(0.0) as u64;
        }

        snap
    }
}

fn cpu_times() -> Option<(u64, u64)> {
    let text = fs::read_to_string("/proc/stat").ok()?;
    let line = text.lines().find(|l| l.starts_with("cpu "))?;
    let fields: Vec<u64> = line
        .split_whitespace()
        .skip(1)
        .filter_map(|v| v.parse().ok())
        .collect();
    if fields.len() < 4 {
        return None;
    }
    let idle = fields[3] + fields.get(4).copied().unwrap_or(0);
    let total = fields.iter().sum();
    Some((total, idle))
}

fn mem_info() -> Option<(u64, u64)> {
    let text = fs::read_to_string("/proc/meminfo").ok()?;
    let mut total = None;
    let mut available = None;
    for line in text.lines() {
        if let Some(v) = line.strip_prefix("MemTotal:") {
            total = v.split_whitespace().next()?.parse::<u64>().ok();
        } else if let Some(v) = line.strip_prefix("MemAvailable:") {
            available = v.split_whitespace().next()?.parse::<u64>().ok();
        }
    }
    let total = total? * 1024;
    let available = available? * 1024;
    Some((total.saturating_sub(available), total))
}

fn net_bytes() -> Option<(u64, u64)> {
    let entries = fs::read_dir("/sys/class/net").ok()?;
    let mut rx_total = 0u64;
    let mut tx_total = 0u64;
    for entry in entries.filter_map(|e| e.ok()) {
        let name = entry.file_name();
        if name == "lo" {
            continue;
        }
        // 只统计物理网卡，避免 veth/bridge 的流量被重复累加。
        if !entry.path().join("device").exists() {
            continue;
        }
        let base = entry.path().join("statistics");
        rx_total += read_u64(&base.join("rx_bytes")).unwrap_or(0);
        tx_total += read_u64(&base.join("tx_bytes")).unwrap_or(0);
    }
    Some((rx_total, tx_total))
}

fn read_u64(path: &Path) -> Option<u64> {
    fs::read_to_string(path).ok()?.trim().parse().ok()
}

fn temperatures() -> Vec<(String, f32)> {
    let mut result: Vec<(String, f32)> = Vec::new();
    let Ok(entries) = fs::read_dir("/sys/class/hwmon") else {
        return result;
    };

    for entry in entries.filter_map(|e| e.ok()) {
        let dir = entry.path();
        let chip = fs::read_to_string(dir.join("name"))
            .map(|v| v.trim().to_string())
            .unwrap_or_else(|_| "sensor".into());
        let Ok(files) = fs::read_dir(&dir) else {
            continue;
        };
        for file in files.filter_map(|e| e.ok()) {
            let name = file.file_name().to_string_lossy().into_owned();
            let Some(stem) = name.strip_suffix("_input") else {
                continue;
            };
            if !stem.starts_with("temp") {
                continue;
            }
            let Ok(raw) = fs::read_to_string(file.path()) else {
                continue;
            };
            let Ok(milli) = raw.trim().parse::<f32>() else {
                continue;
            };
            let celsius = milli / 1000.0;
            if !(0.0..150.0).contains(&celsius) {
                continue;
            }
            let label = fs::read_to_string(dir.join(format!("{stem}_label")))
                .map(|v| v.trim().to_string())
                .unwrap_or_else(|_| chip.clone());
            let label = if label.is_empty() {
                chip.clone()
            } else {
                label
            };
            let label = format!("{label} ({chip})");
            if result.iter().any(|(existing, _)| *existing == label) {
                continue;
            }
            result.push((label, celsius));
        }
    }

    result.sort_by(|a, b| b.1.total_cmp(&a.1));
    result.truncate(8);
    result
}
