use std::fs;
use std::path::Path;
use std::time::Instant;

#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub cpu_pct: f32,
    pub cpu_cores: Vec<f32>,
    pub mem_used: u64,
    pub mem_total: u64,
    pub mem_cached: u64,
    pub mem_free: u64,
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
    prev_cores: Vec<(u64, u64)>,
    prev_net: Option<(u64, u64)>,
    prev_time: Option<Instant>,
}

impl Collector {
    pub fn sample(&mut self) -> Snapshot {
        let now = Instant::now();
        let mut snap = Snapshot::default();

        let (overall, cores) = cpu_times_all();
        if let Some((total, idle)) = overall {
            if let Some((prev_total, prev_idle)) = self.prev_cpu {
                let d_total = total.saturating_sub(prev_total) as f64;
                let d_idle = idle.saturating_sub(prev_idle) as f64;
                if d_total > 0.0 {
                    snap.cpu_pct = ((1.0 - d_idle / d_total) * 100.0).clamp(0.0, 100.0) as f32;
                }
            }
            self.prev_cpu = Some((total, idle));
        }
        if self.prev_cores.len() == cores.len() {
            snap.cpu_cores = cores
                .iter()
                .zip(self.prev_cores.iter())
                .map(|((total, idle), (prev_total, prev_idle))| {
                    let d_total = total.saturating_sub(*prev_total) as f64;
                    let d_idle = idle.saturating_sub(*prev_idle) as f64;
                    if d_total > 0.0 {
                        ((1.0 - d_idle / d_total) * 100.0).clamp(0.0, 100.0) as f32
                    } else {
                        0.0
                    }
                })
                .collect();
        } else {
            snap.cpu_cores = vec![0.0; cores.len()];
        }
        self.prev_cores = cores;

        if let Some(mem) = mem_info() {
            snap.mem_used = mem.used;
            snap.mem_total = mem.total;
            snap.mem_cached = mem.cached;
            snap.mem_free = mem.free;
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

type CpuTime = (u64, u64);

fn cpu_times_all() -> (Option<CpuTime>, Vec<CpuTime>) {
    let Ok(text) = fs::read_to_string("/proc/stat") else {
        return (None, Vec::new());
    };
    let mut overall = None;
    let mut cores = Vec::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("cpu ") {
            overall = parse_cpu_line(rest);
        } else if let Some(rest) = line.strip_prefix("cpu")
            && rest.chars().next().is_some_and(|c| c.is_ascii_digit())
            && let Some(value) =
                parse_cpu_line(rest.trim_start_matches(|c: char| c.is_ascii_digit()))
        {
            cores.push(value);
        }
    }
    (overall, cores)
}

fn parse_cpu_line(line: &str) -> Option<(u64, u64)> {
    let fields: Vec<u64> = line
        .split_whitespace()
        .filter_map(|v| v.parse().ok())
        .collect();
    if fields.len() < 4 {
        return None;
    }
    let idle = fields[3] + fields.get(4).copied().unwrap_or(0);
    let total = fields.iter().sum();
    Some((total, idle))
}

struct MemInfo {
    used: u64,
    total: u64,
    cached: u64,
    free: u64,
}

fn mem_info() -> Option<MemInfo> {
    let text = fs::read_to_string("/proc/meminfo").ok()?;
    let value = |key: &str| {
        text.lines().find_map(|line| {
            line.strip_prefix(key)
                .and_then(|v| v.split_whitespace().next())
                .and_then(|v| v.parse::<u64>().ok())
        })
    };
    let total = value("MemTotal:")? * 1024;
    let available = value("MemAvailable:")? * 1024;
    let cached = value("Cached:").unwrap_or(0) * 1024
        + value("SReclaimable:").unwrap_or(0) * 1024
        + value("Buffers:").unwrap_or(0) * 1024;
    let used = total.saturating_sub(available);
    let free = total.saturating_sub(used + cached);
    Some(MemInfo {
        used,
        total,
        cached,
        free,
    })
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
        let mut sensors: Vec<(String, f32)> = Vec::new();
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
            sensors.push((label, celsius));
        }
        if sensors.is_empty() {
            continue;
        }

        // 一块 NVMe 通常有 Composite / Sensor 1 / Sensor 2 三个读数。
        // 仪表盘只保留该盘的最高温度，避免一块盘占满整个列表。
        if chip == "nvme" {
            if let Some((_, celsius)) = sensors.iter().max_by(|a, b| a.1.total_cmp(&b.1)) {
                result.push((nvme_label(&dir), *celsius));
            }
            continue;
        }

        for (sensor, celsius) in sensors {
            let label = temperature_label(&dir, &chip, &sensor);
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

fn nvme_label(dir: &Path) -> String {
    let model = fs::read_to_string(dir.join("device/model"))
        .map(|v| v.trim().to_string())
        .unwrap_or_default();
    if model.is_empty() {
        "NVMe".into()
    } else {
        format!("{model} (NVMe)")
    }
}

fn temperature_label(dir: &Path, chip: &str, sensor: &str) -> String {
    match chip {
        "k10temp" => format!("CPU ({sensor})"),
        "amdgpu" => "核显 (amdgpu)".into(),
        "spd5118" => dimm_label(dir),
        "acpitz" => "ACPI 温区".into(),
        _ if sensor.is_empty() || sensor == chip => chip.into(),
        _ => format!("{sensor} ({chip})"),
    }
}

fn dimm_label(dir: &Path) -> String {
    let address = fs::canonicalize(dir.join("device"))
        .ok()
        .and_then(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .unwrap_or_default();
    if address.ends_with("0050") {
        "内存 1 (SPD)".into()
    } else if address.ends_with("0051") {
        "内存 2 (SPD)".into()
    } else {
        "内存 (SPD)".into()
    }
}
