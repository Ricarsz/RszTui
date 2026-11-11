use std::ffi::CString;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::gpu;

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
    pub load1: f32,
    pub load5: f32,
    pub load15: f32,
    pub cpu_mhz: Option<f32>,
    pub cpu_mhz_max: Option<f32>,
    /// CPU 封装功耗（RAPL 能量计数差分，瓦）。
    pub cpu_power_w: Option<f32>,
    pub swap_used: u64,
    pub swap_total: u64,
    pub fans: Vec<(String, u32)>,
    pub gpus: Vec<gpu::Status>,
    pub gpu_load: Option<u8>,
    pub disk_used: u64,
    pub disk_total: u64,
    pub uptime: u64,
    pub net_ifaces: Vec<NetIface>,
}

#[derive(Debug, Clone, Default)]
pub struct NetIface {
    pub name: String,
    pub rx_bps: f64,
    pub tx_bps: f64,
    pub rx_total: u64,
    pub tx_total: u64,
    /// 链路速率（Mbps），无线网卡运行时可能为空。
    pub speed_mbps: Option<u64>,
    pub operstate: String,
    pub physical: bool,
}

#[derive(Debug, Clone, Default)]
struct NetRaw {
    name: String,
    rx: u64,
    tx: u64,
    physical: bool,
    operstate: String,
    speed_mbps: Option<u64>,
}

#[derive(Default)]
pub struct Collector {
    prev_cpu: Option<(u64, u64)>,
    prev_cores: Vec<(u64, u64)>,
    prev_net: Option<(u64, u64)>,
    prev_time: Option<Instant>,
    prev_energy: Option<(u64, Instant)>,
    prev_ifaces: Vec<(String, u64, u64)>,
    gpus: Option<Vec<gpu::Gpu>>,
    /// hwmon 拓扑（chip、标签、文件路径）整个会话不变，只建一次。
    hwmon: Option<HwmonCache>,
    /// 各核 scaling_cur_freq 路径，首次枚举后复用，省掉每秒 read_dir。
    freq_paths: Option<Vec<PathBuf>>,
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
            snap.swap_used = mem.swap_used;
            snap.swap_total = mem.swap_total;
        }

        if self.freq_paths.is_none() {
            self.freq_paths = Some(cpu_freq_paths());
        }
        let mut freqs = self
            .freq_paths
            .as_deref()
            .map(cpu_freqs_from)
            .unwrap_or((None, None));
        if freqs.0.is_none() {
            freqs = cpuinfo_freqs();
        }
        snap.cpu_mhz = freqs.0;
        snap.cpu_mhz_max = freqs.1;
        snap.cpu_power_w = self.cpu_power(now);
        if self.hwmon.is_none() {
            self.hwmon = Some(HwmonCache::build());
        }
        snap.fans = self
            .hwmon
            .as_ref()
            .map(HwmonCache::fans)
            .unwrap_or_default();
        if self.gpus.is_none() {
            self.gpus = Some(gpu::discover());
        }
        let gpus = gpu::sample(self.gpus.as_deref().unwrap_or(&[]));
        snap.gpu_load = gpus.iter().filter_map(|status| status.stats.load).max();
        if let Some((used, total)) = disk_usage("/") {
            snap.disk_used = used;
            snap.disk_total = total;
        }

        let raw = net_stats();
        let secs = self
            .prev_time
            .map(|prev| now.duration_since(prev).as_secs_f64().max(0.001));
        let mut rx_total = 0u64;
        let mut tx_total = 0u64;
        for iface in &raw {
            let prev = self
                .prev_ifaces
                .iter()
                .find(|(name, _, _)| *name == iface.name);
            let (rx_bps, tx_bps) = match (prev, secs) {
                (Some((_, prev_rx, prev_tx)), Some(secs)) => (
                    iface.rx.saturating_sub(*prev_rx) as f64 / secs,
                    iface.tx.saturating_sub(*prev_tx) as f64 / secs,
                ),
                _ => (0.0, 0.0),
            };
            if iface.physical {
                rx_total += iface.rx;
                tx_total += iface.tx;
            }
            if iface.physical || iface.rx + iface.tx > 0 {
                snap.net_ifaces.push(NetIface {
                    name: iface.name.clone(),
                    rx_bps,
                    tx_bps,
                    rx_total: iface.rx,
                    tx_total: iface.tx,
                    speed_mbps: iface.speed_mbps,
                    operstate: iface.operstate.clone(),
                    physical: iface.physical,
                });
            }
        }
        self.prev_ifaces = raw
            .iter()
            .map(|iface| (iface.name.clone(), iface.rx, iface.tx))
            .collect();
        if let (Some(prev), Some(secs)) = (self.prev_net, secs) {
            let (prev_rx, prev_tx) = prev;
            snap.rx_bps = rx_total.saturating_sub(prev_rx) as f64 / secs;
            snap.tx_bps = tx_total.saturating_sub(prev_tx) as f64 / secs;
        }
        self.prev_net = Some((rx_total, tx_total));
        self.prev_time = Some(now);

        let temps = self
            .hwmon
            .as_ref()
            .map(|hwmon| hwmon.temps(&gpus))
            .unwrap_or_default();
        snap.max_temp = temps.iter().map(|(_, v)| *v).reduce(f32::max);
        snap.temps = temps;
        snap.gpus = gpus;

        if let Ok(text) = fs::read_to_string("/proc/loadavg") {
            let mut fields = text.split_whitespace();
            snap.load1 = fields.next().and_then(|v| v.parse().ok()).unwrap_or(0.0);
            snap.load5 = fields.next().and_then(|v| v.parse().ok()).unwrap_or(0.0);
            snap.load15 = fields.next().and_then(|v| v.parse().ok()).unwrap_or(0.0);
        }

        if let Ok(text) = fs::read_to_string("/proc/uptime")
            && let Some(first) = text.split_whitespace().next()
        {
            snap.uptime = first.parse::<f64>().unwrap_or(0.0) as u64;
        }

        snap
    }

    fn cpu_power(&mut self, now: Instant) -> Option<f32> {
        let energy = rapl_energy()?;
        let power = self.prev_energy.and_then(|(prev, at)| {
            let secs = now.duration_since(at).as_secs_f64();
            if secs < 0.05 {
                return None;
            }
            let delta = if energy >= prev {
                energy - prev
            } else {
                rapl_max_energy()?.saturating_sub(prev) + energy
            };
            Some((delta as f64 / 1_000_000.0 / secs) as f32)
        });
        self.prev_energy = Some((energy, now));
        power
    }
}

fn rapl_energy() -> Option<u64> {
    let text = fs::read_to_string("/sys/class/powercap/intel-rapl:0/energy_uj").ok()?;
    text.trim().parse().ok()
}

fn rapl_max_energy() -> Option<u64> {
    let text = fs::read_to_string("/sys/class/powercap/intel-rapl:0/max_energy_range_uj").ok()?;
    text.trim().parse().ok()
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
    swap_used: u64,
    swap_total: u64,
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
    let swap_total = value("SwapTotal:").unwrap_or(0) * 1024;
    let swap_free = value("SwapFree:").unwrap_or(0) * 1024;
    Some(MemInfo {
        used,
        total,
        cached,
        free,
        swap_used: swap_total.saturating_sub(swap_free),
        swap_total,
    })
}

/// 首次枚举各核 scaling_cur_freq 路径；之后每周期只读这些文件（值会变，
/// 路径不变），省掉每秒一次 read_dir 与文件名解析。
fn cpu_freq_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let Ok(entries) = fs::read_dir("/sys/devices/system/cpu") else {
        return paths;
    };
    for entry in entries.filter_map(|entry| entry.ok()) {
        let name = entry.file_name();
        let Some(index) = name.to_str().and_then(|name| name.strip_prefix("cpu")) else {
            continue;
        };
        if index.is_empty() || !index.bytes().all(|byte| byte.is_ascii_digit()) {
            continue;
        }
        paths.push(entry.path().join("cpufreq/scaling_cur_freq"));
    }
    paths
}

/// 按缓存路径读每核频率，返回（平均值、最大值）GHz。
fn cpu_freqs_from(paths: &[PathBuf]) -> (Option<f32>, Option<f32>) {
    let mut sum = 0.0f64;
    let mut max = 0.0f64;
    let mut count = 0u32;
    for path in paths {
        let Ok(text) = fs::read_to_string(path) else {
            continue;
        };
        let Ok(khz) = text.trim().parse::<f64>() else {
            continue;
        };
        sum += khz;
        max = max.max(khz);
        count += 1;
    }
    if count == 0 {
        return (None, None);
    }
    (
        Some((sum / count as f64 / 1_000_000.0) as f32),
        Some((max / 1_000_000.0) as f32),
    )
}

/// 没有 cpufreq 时的兜底：读 /proc/cpuinfo 的 MHz 估算。
fn cpuinfo_freqs() -> (Option<f32>, Option<f32>) {
    let Ok(text) = fs::read_to_string("/proc/cpuinfo") else {
        return (None, None);
    };
    let mut sum = 0.0f64;
    let mut max = 0.0f64;
    let mut count = 0u32;
    for line in text.lines() {
        if let Some(value) = line.strip_prefix("cpu MHz")
            && let Some(value) = value.split(':').nth(1)
            && let Ok(mhz) = value.trim().parse::<f64>()
        {
            sum += mhz;
            max = max.max(mhz);
            count += 1;
        }
    }
    if count == 0 {
        return (None, None);
    }
    (
        Some((sum / count as f64 / 1000.0) as f32),
        Some((max / 1000.0) as f32),
    )
}

fn net_stats() -> Vec<NetRaw> {
    let mut result = Vec::new();
    let Ok(entries) = fs::read_dir("/sys/class/net") else {
        return result;
    };
    for entry in entries.filter_map(|entry| entry.ok()) {
        let name = entry.file_name().to_string_lossy().to_string();
        if name == "lo" {
            continue;
        }
        let dir = entry.path();
        let physical = dir.join("device").exists();
        let operstate = fs::read_to_string(dir.join("operstate"))
            .map(|value| value.trim().to_string())
            .unwrap_or_default();
        if !physical && is_virtual_iface(&name) && operstate != "up" {
            continue;
        }
        let stats = dir.join("statistics");
        result.push(NetRaw {
            name,
            rx: read_u64(&stats.join("rx_bytes")).unwrap_or(0),
            tx: read_u64(&stats.join("tx_bytes")).unwrap_or(0),
            physical,
            operstate,
            speed_mbps: fs::read_to_string(dir.join("speed"))
                .ok()
                .and_then(|value| value.trim().parse::<i64>().ok())
                .filter(|speed| *speed > 0)
                .map(|speed| speed as u64),
        });
    }
    // 物理网卡、已启用的排前面，其余按名字稳定排序，避免每秒跳动。
    result.sort_by(|a, b| {
        b.physical
            .cmp(&a.physical)
            .then_with(|| (b.operstate == "up").cmp(&(a.operstate == "up")))
            .then_with(|| a.name.cmp(&b.name))
    });
    result
}

fn is_virtual_iface(name: &str) -> bool {
    const PREFIXES: [&str; 10] = [
        "veth",
        "docker",
        "br-",
        "virbr",
        "tun",
        "tap",
        "wg",
        "tailscale",
        "vnet",
        "vmnet",
    ];
    PREFIXES.iter().any(|prefix| name.starts_with(prefix))
}

/// hwmon 拓扑缓存：chip 名、标签与 `*_input` 路径整个会话不变，首次采样时
/// 建好；之后每周期只读数值文件，省掉重复的 read_dir、`name`/`_label` 读取，
/// 以及对内存 SPD 的 canonicalize。
#[derive(Default)]
struct HwmonCache {
    temps: Vec<TempProbe>,
    fans: Vec<FanProbe>,
}

struct TempProbe {
    /// 一般只有 1 个；nvme 存该盘全部 temp 文件，composite 时第一个是 temp1。
    paths: Vec<PathBuf>,
    label: String,
    rank: u8,
    /// nvme：优先取 temp1（Composite），取不到再取组内最大。
    composite: bool,
}

struct FanProbe {
    path: PathBuf,
    label: String,
}

impl HwmonCache {
    fn build() -> Self {
        let mut cache = HwmonCache::default();
        let Ok(entries) = fs::read_dir("/sys/class/hwmon") else {
            return cache;
        };
        for entry in entries.filter_map(|e| e.ok()) {
            let dir = entry.path();
            let chip = fs::read_to_string(dir.join("name"))
                .map(|v| v.trim().to_string())
                .unwrap_or_default();
            if chip.is_empty() {
                continue;
            }

            // asus 提供 cpu_fan / gpu_fan 两路实际转速。
            if chip == "asus"
                && let Ok(files) = fs::read_dir(&dir)
            {
                for file in files.filter_map(|e| e.ok()) {
                    let name = file.file_name().to_string_lossy().into_owned();
                    let Some(stem) = name.strip_suffix("_input") else {
                        continue;
                    };
                    if !stem.starts_with("fan") {
                        continue;
                    }
                    let label = fs::read_to_string(dir.join(format!("{stem}_label")))
                        .map(|v| v.trim().to_string())
                        .unwrap_or_default();
                    let label = match label.as_str() {
                        "cpu_fan" => "CPU".to_string(),
                        "gpu_fan" => "GPU".to_string(),
                        "" => format!("风扇{}", &stem[3..]),
                        other => other.to_string(),
                    };
                    cache.fans.push(FanProbe {
                        path: file.path(),
                        label,
                    });
                }
            }

            // GPU 温度由 gpu 模块按设备归集，避免把核显/独显的传感器混在一起。
            if chip == "amdgpu" || chip == "nvidia" {
                continue;
            }

            let mut sensors: Vec<(String, PathBuf)> = Vec::new();
            if let Ok(files) = fs::read_dir(&dir) {
                for file in files.filter_map(|e| e.ok()) {
                    let name = file.file_name().to_string_lossy().into_owned();
                    let Some(stem) = name.strip_suffix("_input") else {
                        continue;
                    };
                    if !stem.starts_with("temp") {
                        continue;
                    }
                    sensors.push((stem.to_string(), file.path()));
                }
            }
            if sensors.is_empty() {
                continue;
            }

            if chip == "nvme" {
                // Composite（temp1）是 Mission Center 等工具显示的那一路；
                // Sensor 1/2 是闪存颗粒，个别情况下会高出 10–15°C。
                let composite = sensors.iter().any(|(stem, _)| stem == "temp1");
                let mut paths = Vec::with_capacity(sensors.len());
                if let Some((_, path)) = sensors.iter().find(|(stem, _)| stem == "temp1") {
                    paths.push(path.clone());
                }
                for (stem, path) in &sensors {
                    if stem != "temp1" {
                        paths.push(path.clone());
                    }
                }
                cache.temps.push(TempProbe {
                    paths,
                    label: nvme_label(&dir),
                    rank: 3,
                    composite,
                });
                continue;
            }

            for (stem, path) in sensors {
                let label = fs::read_to_string(dir.join(format!("{stem}_label")))
                    .map(|v| v.trim().to_string())
                    .unwrap_or_default();
                let (rank, label) = match chip.as_str() {
                    "k10temp" => (
                        0,
                        if label.is_empty() {
                            "CPU".to_string()
                        } else {
                            format!("CPU {label}")
                        },
                    ),
                    "spd5118" => (4, dimm_label(&dir)),
                    "acpitz" => (5, "ACPI 温区".to_string()),
                    _ => (6, temperature_label(&chip, &label)),
                };
                cache.temps.push(TempProbe {
                    paths: vec![path],
                    label,
                    rank,
                    composite: false,
                });
            }
        }
        cache
    }

    fn fans(&self) -> Vec<(String, u32)> {
        let mut result: Vec<(String, u32)> = self
            .fans
            .iter()
            .filter_map(|probe| {
                let raw = fs::read_to_string(&probe.path).ok()?;
                let rpm = raw.trim().parse::<u32>().ok()?;
                Some((probe.label.clone(), rpm))
            })
            .collect();
        result.sort_by(|a, b| a.0.cmp(&b.0));
        result
    }

    /// 温度条目：优先级只决定显示顺序（CPU → GPU → 磁盘 → 内存 → ACPI），
    /// 不再单纯按数值排序，避免每个采样周期列表整体跳动。
    fn temps(&self, gpus: &[gpu::Status]) -> Vec<(String, f32)> {
        let mut rated: Vec<(u8, String, f32)> = Vec::new();
        for probe in &self.temps {
            let celsius = if probe.composite {
                probe
                    .paths
                    .first()
                    .and_then(|path| read_celsius(path))
                    .or_else(|| {
                        probe
                            .paths
                            .iter()
                            .skip(1)
                            .filter_map(|path| read_celsius(path))
                            .max_by(f32::total_cmp)
                    })
            } else {
                probe
                    .paths
                    .iter()
                    .filter_map(|path| read_celsius(path))
                    .max_by(f32::total_cmp)
            };
            if let Some(celsius) = celsius {
                push_temp(&mut rated, probe.rank, probe.label.clone(), celsius);
            }
        }

        for status in gpus {
            let name = status.gpu.short.clone();
            let rank = match status.gpu.vendor {
                gpu::Vendor::Nvidia => 1,
                _ => 2,
            };
            if let Some(celsius) = status.stats.temp {
                push_temp(&mut rated, rank, name.clone(), celsius);
            }
            if let Some(celsius) = status.stats.hotspot
                && status.stats.temp != Some(celsius)
            {
                push_temp(&mut rated, rank, format!("{name} 热点"), celsius);
            }
            if let Some(celsius) = status.stats.mem_temp {
                push_temp(&mut rated, rank, format!("{name} 显存"), celsius);
            }
        }

        rated.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
        rated.truncate(10);
        rated
            .into_iter()
            .map(|(_, label, value)| (label, value))
            .collect()
    }
}

fn read_celsius(path: &Path) -> Option<f32> {
    let raw = fs::read_to_string(path).ok()?;
    let milli = raw.trim().parse::<f32>().ok()?;
    let celsius = milli / 1000.0;
    (0.0..150.0).contains(&celsius).then_some(celsius)
}

fn disk_usage(path: &str) -> Option<(u64, u64)> {
    let path = CString::new(path).ok()?;
    // SAFETY: statvfs 只读填充调用方提供的结构体，指针来自有效的 CString。
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(path.as_ptr(), &mut stat) } != 0 {
        return None;
    }
    let block = stat.f_frsize as u64;
    let total = stat.f_blocks as u64 * block;
    let free = stat.f_bavail as u64 * block;
    Some((total.saturating_sub(free), total))
}

fn read_u64(path: &Path) -> Option<u64> {
    fs::read_to_string(path).ok()?.trim().parse().ok()
}

fn push_temp(list: &mut Vec<(u8, String, f32)>, rank: u8, label: String, celsius: f32) {
    if list.iter().any(|(_, existing, _)| *existing == label) {
        return;
    }
    list.push((rank, label, celsius));
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

fn temperature_label(chip: &str, sensor: &str) -> String {
    if sensor.is_empty() || sensor == chip {
        chip.into()
    } else {
        format!("{sensor} ({chip})")
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

#[cfg(test)]
mod net_tests {
    #[test]
    fn classifies_virtual_interfaces() {
        assert!(super::is_virtual_iface("veth1234"));
        assert!(super::is_virtual_iface("br-9a1b"));
        assert!(super::is_virtual_iface("tun0"));
        assert!(!super::is_virtual_iface("enp3s0"));
        assert!(!super::is_virtual_iface("wlan0"));
    }
}

#[cfg(test)]
mod hwmon_tests {
    use super::*;

    #[test]
    fn celsius_filter_rejects_out_of_range() {
        let path =
            std::env::temp_dir().join(format!("rogtui-celsius-{}.txt", std::process::id()));
        std::fs::write(&path, "45000\n").unwrap();
        assert_eq!(read_celsius(&path), Some(45.0));
        std::fs::write(&path, "200000\n").unwrap();
        assert_eq!(read_celsius(&path), None, "超出 0–150°C 的读数应被丢弃");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn hwmon_cache_builds_and_caps_temps() {
        let cache = HwmonCache::build();
        let temps = cache.temps(&[]);
        assert!(temps.len() <= 10, "温度条目应被截断到 10 个");
        let fans = cache.fans();
        let mut sorted = fans.clone();
        sorted.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(fans, sorted, "风扇条目应按名字稳定排序");
    }
}

/// CPU 调频联动状态：`asusd` 切性能模式时会同时改 ACPI platform_profile 与
/// EPP（amd-pstate-epp），governor 保持 powersave 不变。这里只读，不做任何写入。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Pstate {
    pub driver: String,
    pub governor: String,
    pub platform_profile: String,
    pub epp: String,
    pub epp_choices: Vec<String>,
}

impl Pstate {
    pub fn summary(&self) -> String {
        format!(
            "{} · {} · EPP {} · {}",
            short(&self.platform_profile),
            short(&self.governor),
            short(&self.epp),
            short(&self.driver)
        )
    }

    /// EPP 是否跟着性能模式走：cpu0 的候选值里至少要包含 asusd 会写的三档。
    pub fn linked(&self) -> bool {
        ["performance", "balance_performance", "power"]
            .iter()
            .all(|want| self.epp_choices.iter().any(|have| have == want))
    }
}

fn short(text: &str) -> &str {
    if text.is_empty() { "未知" } else { text }
}

pub fn pstate() -> Pstate {
    let cpu0 = "/sys/devices/system/cpu/cpu0/cpufreq";
    let epp_choices =
        fs::read_to_string(format!("{cpu0}/energy_performance_available_preferences"))
            .map(|text| {
                text.split_whitespace()
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
    Pstate {
        driver: read_trim(format!("{cpu0}/scaling_driver")),
        governor: read_trim(format!("{cpu0}/scaling_governor")),
        platform_profile: read_trim("/sys/firmware/acpi/platform_profile".to_string()),
        epp: read_trim(format!("{cpu0}/energy_performance_preference")),
        epp_choices,
    }
}

fn read_trim(path: String) -> String {
    fs::read_to_string(path)
        .map(|text| text.trim().to_string())
        .unwrap_or_default()
}
