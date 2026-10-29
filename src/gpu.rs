//! GPU 枚举与实时采样：AMD/Intel 走 DRM + hwmon，NVIDIA 走 nvidia-smi。
//!
//! 温度刻意不走「标签里含 gpu」的模糊匹配 —— 那样在多显卡机器上会拿错设备。
//! 这里先按 PCI 设备枚举出每块 GPU，再把传感器/驱动读数归到对应 GPU 上。

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const SMI_TTL: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Vendor {
    Nvidia,
    Amd,
    Intel,
    Other,
}

impl Vendor {
    pub fn from_id(id: &str) -> Self {
        match id.trim().to_ascii_lowercase().as_str() {
            "0x10de" => Self::Nvidia,
            "0x1002" => Self::Amd,
            "0x8086" => Self::Intel,
            _ => Self::Other,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Nvidia => "NVIDIA",
            Self::Amd => "AMD",
            Self::Intel => "Intel",
            Self::Other => "GPU",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Gpu {
    pub vendor: Vendor,
    /// 完整型号，例如 `GeForce RTX 4060 Laptop GPU`。
    pub name: String,
    /// 紧凑型号，例如 `RTX 4060`。
    pub short: String,
    pub driver: String,
    /// PCI 地址，例如 `0000:01:00.0`。
    pub pci: String,
    /// DRM 节点名，例如 `card0`。
    pub card: String,
    pub hwmon: Option<PathBuf>,
}

#[derive(Debug, Clone, Default)]
pub struct Stats {
    pub temp: Option<f32>,
    pub hotspot: Option<f32>,
    pub mem_temp: Option<f32>,
    pub load: Option<u8>,
    pub mem_used: Option<u64>,
    pub mem_total: Option<u64>,
    /// AMD/Intel 核显从系统内存划走的 GTT 用量。
    pub shared_used: Option<u64>,
    pub power: Option<f32>,
    pub clock_mhz: Option<u32>,
    pub pcie: String,
    pub driver_version: String,
    pub runtime: String,
}

#[derive(Debug, Clone)]
pub struct Status {
    pub gpu: Gpu,
    pub stats: Stats,
}

/// 枚举 `/sys/class/drm` 下的真实 GPU（跳过连接器节点）。
pub fn discover() -> Vec<Gpu> {
    let mut cards: Vec<PathBuf> = Vec::new();
    let Ok(entries) = fs::read_dir("/sys/class/drm") else {
        return Vec::new();
    };
    for entry in entries.filter_map(|entry| entry.ok()) {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(rest) = name.strip_prefix("card") else {
            continue;
        };
        if rest.is_empty() || !rest.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        cards.push(entry.path());
    }
    cards.sort_by(|a, b| a.file_name().cmp(&b.file_name()));

    let mut pending = Vec::new();
    for card in &cards {
        let device = card.join("device");
        let Some(vendor_raw) = read_trim(device.join("vendor")) else {
            continue;
        };
        let vendor = Vendor::from_id(&vendor_raw);
        if !device.join("uevent").exists() {
            continue;
        }
        let pci = fs::canonicalize(&device)
            .ok()
            .and_then(|path| {
                path.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
            })
            .unwrap_or_default();
        let device_id = read_trim(device.join("device"))
            .map(|value| value.to_ascii_lowercase())
            .unwrap_or_default();
        let driver = fs::read_link(device.join("driver"))
            .ok()
            .and_then(|path| {
                path.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
            })
            .unwrap_or_default();
        let hwmon = fs::read_dir(device.join("hwmon"))
            .ok()
            .and_then(|mut entries| entries.find_map(|entry| entry.ok().map(|entry| entry.path())));
        let card_name = card
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        pending.push((card_name, vendor, device_id, driver, pci, hwmon));
    }

    // vulkaninfo 能给出厂商营销名（如 Radeon 780M）；NVIDIA 休眠时不去唤醒它。
    let nvidia_asleep = pending.iter().any(|(card, vendor, ..)| {
        *vendor == Vendor::Nvidia
            && read_trim(
                Path::new("/sys/class/drm")
                    .join(card)
                    .join("device/power/runtime_status"),
            )
            .as_deref()
                == Some("suspended")
    });
    let vulkan = if nvidia_asleep {
        HashMap::new()
    } else {
        vulkan_names()
    };
    let lspci = lspci_names();

    let mut gpus: Vec<Gpu> = Vec::new();
    for (card, vendor, device_id, driver, pci, hwmon) in pending {
        if gpus.iter().any(|gpu| gpu.pci == pci) {
            continue;
        }
        let name = vulkan
            .get(&(vendor, device_id))
            .map(|raw| display_name(raw))
            .or_else(|| lspci.get(&pci).cloned())
            .unwrap_or_else(|| format!("{} GPU", vendor.label()));
        let short = short_name(&name);
        gpus.push(Gpu {
            vendor,
            name,
            short,
            driver,
            pci,
            card,
            hwmon,
        });
    }
    gpus
}

fn display_name(raw: &str) -> String {
    let mut name = raw.trim().to_string();
    if let Some(pos) = name.find(" (RADV") {
        name.truncate(pos);
    }
    for prefix in ["NVIDIA ", "AMD ", "Intel "] {
        if let Some(rest) = name.strip_prefix(prefix) {
            name = rest.to_string();
        }
    }
    if name.contains("Radeon") {
        name = name.replace(" Graphics", "");
    }
    name.trim().to_string()
}

/// 用于温度列表的紧凑名字：`GeForce RTX 4060 Laptop GPU` -> `RTX 4060`。
pub fn short_name(name: &str) -> String {
    let mut short = name.to_string();
    for marker in ["RTX ", "GTX ", "RX ", "Arc "] {
        if let Some(pos) = short.find(marker) {
            short = short[pos..].to_string();
            break;
        }
    }
    for suffix in [" Laptop GPU", " Max-Q / Mobile", " Graphics", " Mobile"] {
        short = short.replace(suffix, "");
    }
    short.trim().to_string()
}

fn vulkan_names() -> HashMap<(Vendor, String), String> {
    let mut map = HashMap::new();
    // 以 root 经 sudo 运行时 DISPLAY 仍指向用户会话，但拿不到 X cookie，
    // vulkaninfo 连接 X 会失败；清掉显示相关变量，让它走无头枚举。
    let Ok(output) = Command::new("vulkaninfo")
        .arg("--summary")
        .env("LC_ALL", "C")
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .env_remove("XAUTHORITY")
        .output()
    else {
        return map;
    };
    if !output.status.success() {
        return map;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut vendor: Option<Vendor> = None;
    let mut device: Option<String> = None;
    let mut name: Option<String> = None;
    for line in text.lines() {
        let line = line.trim();
        let is_header = line
            .strip_prefix("GPU")
            .and_then(|rest| rest.strip_suffix(':'))
            .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()));
        if is_header {
            flush_vulkan(&mut map, &mut vendor, &mut device, &mut name);
            continue;
        }
        if let Some(value) = line.strip_prefix("vendorID") {
            if let Some(value) = value.split_once('=').map(|(_, value)| value.trim()) {
                vendor = Some(Vendor::from_id(value));
            }
        } else if let Some(value) = line.strip_prefix("deviceID") {
            if let Some(value) = value.split_once('=').map(|(_, value)| value.trim()) {
                device = Some(value.to_ascii_lowercase());
            }
        } else if let Some(value) = line.strip_prefix("deviceName")
            && let Some(value) = value.split_once('=').map(|(_, value)| value.trim())
        {
            name = Some(value.to_string());
        }
    }
    flush_vulkan(&mut map, &mut vendor, &mut device, &mut name);
    map
}

fn flush_vulkan(
    map: &mut HashMap<(Vendor, String), String>,
    vendor: &mut Option<Vendor>,
    device: &mut Option<String>,
    name: &mut Option<String>,
) {
    if let (Some(vendor), Some(device), Some(name)) = (vendor.take(), device.take(), name.take()) {
        map.insert((vendor, device), name);
    }
}

fn lspci_names() -> HashMap<String, String> {
    let mut map = HashMap::new();
    let Ok(output) = Command::new("lspci").arg("-D").env("LC_ALL", "C").output() else {
        return map;
    };
    if !output.status.success() {
        return map;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    for line in text.lines() {
        let Some((slot, rest)) = line.split_once(' ') else {
            continue;
        };
        for marker in [
            "VGA compatible controller: ",
            "3D controller: ",
            "Display controller: ",
        ] {
            if let Some(pos) = rest.find(marker) {
                map.insert(
                    slot.to_string(),
                    crate::hw::clean_gpu(&rest[pos + marker.len()..]),
                );
                break;
            }
        }
    }
    map
}

/// 采样所有 GPU。NVIDIA 处于 runtime suspend 时不调用 nvidia-smi，避免把它唤醒。
pub fn sample(gpus: &[Gpu]) -> Vec<Status> {
    let any_active_nvidia = gpus
        .iter()
        .any(|gpu| gpu.vendor == Vendor::Nvidia && runtime_of(gpu) == "active");
    let rows = if any_active_nvidia {
        nvidia_rows()
    } else {
        Vec::new()
    };
    gpus.iter()
        .map(|gpu| Status {
            gpu: gpu.clone(),
            stats: match gpu.vendor {
                Vendor::Nvidia => nvidia_stats(gpu, &rows),
                Vendor::Amd | Vendor::Intel => drm_stats(gpu),
                Vendor::Other => Stats {
                    runtime: runtime_of(gpu),
                    ..Stats::default()
                },
            },
        })
        .collect()
}

fn drm_stats(gpu: &Gpu) -> Stats {
    let device = device_path(gpu);
    let mut stats = Stats {
        runtime: runtime_of(gpu),
        ..Stats::default()
    };
    if let Some(hwmon) = &gpu.hwmon {
        stats.temp = temp_at(hwmon, 1);
        stats.hotspot = temp_at(hwmon, 2);
        stats.mem_temp = temp_at(hwmon, 3);
        stats.power = power_w(hwmon);
    }
    stats.load = read_u64(device.join("gpu_busy_percent")).map(|value| value.min(100) as u8);
    stats.mem_used = read_u64(device.join("mem_info_vram_used"));
    stats.mem_total = read_u64(device.join("mem_info_vram_total"));
    stats.shared_used = read_u64(device.join("mem_info_gtt_used"));
    stats.clock_mhz = amd_clock(&device.join("pp_dpm_sclk"));
    stats.pcie = drm_pcie(&device);
    stats
}

fn nvidia_stats(gpu: &Gpu, rows: &[SmiRow]) -> Stats {
    let mut stats = Stats {
        runtime: runtime_of(gpu),
        ..Stats::default()
    };
    let Some(row) = rows.iter().find(|row| same_bus(&row.bus, &gpu.pci)) else {
        return stats;
    };
    stats.temp = row.temp;
    stats.load = row.load;
    stats.mem_used = row.mem_used;
    stats.mem_total = row.mem_total;
    stats.power = row.power;
    stats.clock_mhz = row.clock;
    stats.driver_version = row.driver.clone();
    if !row.pcie_gen.is_empty() {
        stats.pcie = format!("PCIe {}.0 x{}", row.pcie_gen, row.width);
    }
    stats
}

#[derive(Debug, Clone, Default)]
struct SmiRow {
    bus: String,
    temp: Option<f32>,
    load: Option<u8>,
    mem_used: Option<u64>,
    mem_total: Option<u64>,
    power: Option<f32>,
    clock: Option<u32>,
    pcie_gen: String,
    width: String,
    driver: String,
}

fn nvidia_rows() -> Vec<SmiRow> {
    static CACHE: OnceLock<Mutex<Option<SmiCache>>> = OnceLock::new();
    type SmiCache = (Instant, Vec<SmiRow>);
    let cache = CACHE.get_or_init(|| Mutex::new(None));
    {
        let guard = cache.lock().unwrap_or_else(|err| err.into_inner());
        if let Some((at, rows)) = guard.as_ref()
            && at.elapsed() < SMI_TTL
        {
            return rows.clone();
        }
    }
    let rows = query_nvidia_smi();
    let mut guard = cache.lock().unwrap_or_else(|err| err.into_inner());
    if rows.is_empty()
        && let Some((_, old)) = guard.as_ref()
    {
        return old.clone();
    }
    *guard = Some((Instant::now(), rows.clone()));
    rows
}

fn query_nvidia_smi() -> Vec<SmiRow> {
    let Ok(output) = Command::new("nvidia-smi")
        .args([
            "--query-gpu=pci.bus_id,temperature.gpu,utilization.gpu,memory.used,memory.total,power.draw,clocks.current.graphics,pcie.link.gen.current,pcie.link.width.current,driver_version",
            "--format=csv,noheader,nounits",
        ])
        .env("LC_ALL", "C")
        .output()
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(parse_smi_row)
        .collect()
}

fn parse_smi_row(line: &str) -> Option<SmiRow> {
    let fields: Vec<&str> = line.split(',').map(str::trim).collect();
    if fields.len() < 10 {
        return None;
    }
    let mib = 1024.0 * 1024.0;
    Some(SmiRow {
        bus: fields[0].to_string(),
        temp: parse_num::<f32>(fields[1]),
        load: parse_num::<u8>(fields[2]),
        mem_used: parse_num::<f64>(fields[3]).map(|value| (value * mib) as u64),
        mem_total: parse_num::<f64>(fields[4]).map(|value| (value * mib) as u64),
        power: parse_num::<f32>(fields[5]),
        clock: parse_num::<u32>(fields[6]),
        pcie_gen: fields[7].to_string(),
        width: fields[8].to_string(),
        driver: fields[9].to_string(),
    })
}

fn parse_num<T: std::str::FromStr>(raw: &str) -> Option<T> {
    let raw = raw.trim();
    if raw.is_empty() || raw.contains("N/A") {
        return None;
    }
    raw.parse().ok()
}

fn same_bus(a: &str, b: &str) -> bool {
    let tail = |value: &str| {
        let mut parts = value.rsplitn(3, ':');
        let device = parts.next().unwrap_or_default();
        let bus = parts.next().unwrap_or_default();
        format!("{bus}:{device}")
    };
    tail(a) == tail(b)
}

fn device_path(gpu: &Gpu) -> PathBuf {
    Path::new("/sys/class/drm").join(&gpu.card).join("device")
}

fn runtime_of(gpu: &Gpu) -> String {
    read_trim(device_path(gpu).join("power/runtime_status")).unwrap_or_default()
}

fn temp_at(hwmon: &Path, index: u8) -> Option<f32> {
    let raw = read_trim(hwmon.join(format!("temp{index}_input")))?;
    let celsius = raw.parse::<f32>().ok()? / 1000.0;
    (0.0..150.0).contains(&celsius).then_some(celsius)
}

fn power_w(hwmon: &Path) -> Option<f32> {
    let average = read_u64(hwmon.join("power1_average")).map(|value| value as f32 / 1_000_000.0);
    average.or_else(|| read_u64(hwmon.join("power1_input")).map(|value| value as f32 / 1_000_000.0))
}

fn amd_clock(path: &Path) -> Option<u32> {
    let text = fs::read_to_string(path).ok()?;
    let line = text.lines().find(|line| line.trim_end().ends_with('*'))?;
    let token = line.split_whitespace().nth(1)?;
    let digits: String = token.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

fn drm_pcie(device: &Path) -> String {
    let speed = read_trim(device.join("current_link_speed"));
    let width = read_trim(device.join("current_link_width"));
    match (speed, width) {
        (Some(speed), Some(width)) => match pcie_gen(&speed) {
            Some(link_gen) => format!("PCIe {link_gen}.0 x{width}"),
            None => format!("PCIe {} x{}", speed.replace(" PCIe", ""), width),
        },
        _ => String::new(),
    }
}

/// `current_link_speed` 是 GT/s 字符串（2.5/5.0/8.0/16.0 GT/s），换算成代际。
fn pcie_gen(speed: &str) -> Option<u32> {
    let value = speed.split_whitespace().next()?.parse::<f32>().ok()?;
    let link_gen = match value {
        v if (v - 2.5).abs() < 0.1 => 1,
        v if (v - 5.0).abs() < 0.1 => 2,
        v if (v - 8.0).abs() < 0.1 => 3,
        v if (v - 16.0).abs() < 0.1 => 4,
        v if (v - 32.0).abs() < 0.1 => 5,
        v if (v - 64.0).abs() < 0.1 => 6,
        _ => return None,
    };
    Some(link_gen)
}

fn read_trim(path: impl AsRef<Path>) -> Option<String> {
    let text = fs::read_to_string(path).ok()?;
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_string())
}

fn read_u64(path: impl AsRef<Path>) -> Option<u64> {
    read_trim(path)?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_pcie_speed_to_generation() {
        assert_eq!(pcie_gen("16.0 GT/s PCIe"), Some(4));
        assert_eq!(pcie_gen("8.0 GT/s PCIe"), Some(3));
        assert_eq!(pcie_gen("2.5 GT/s PCIe"), Some(1));
        assert_eq!(pcie_gen("unknown"), None);
    }

    #[test]
    fn parses_nvidia_smi_row() {
        let row = parse_smi_row("00000000:01:00.0, 38, 0, 2, 8188, 2.19, 210, 4, 8, 615.78.08")
            .expect("row");
        assert_eq!(row.bus, "00000000:01:00.0");
        assert_eq!(row.temp, Some(38.0));
        assert_eq!(row.load, Some(0));
        assert_eq!(row.mem_used, Some(2 * 1024 * 1024));
        assert_eq!(row.mem_total, Some(8188 * 1024 * 1024));
        assert_eq!(row.power, Some(2.19));
        assert_eq!(row.pcie_gen, "4");
        assert_eq!(row.driver, "615.78.08");
    }

    #[test]
    fn handles_na_fields() {
        let row = parse_smi_row("00000000:01:00.0, [N/A], 0, 2, 8188, [N/A], 210, 4, 8, 615.78.08")
            .expect("row");
        assert_eq!(row.temp, None);
        assert_eq!(row.power, None);
        assert_eq!(row.load, Some(0));
    }

    #[test]
    fn matches_pci_bus_with_short_domain() {
        assert!(same_bus("00000000:01:00.0", "0000:01:00.0"));
        assert!(!same_bus("00000000:01:00.0", "0000:66:00.0"));
    }

    #[test]
    fn shortens_gpu_names() {
        assert_eq!(short_name("GeForce RTX 4060 Laptop GPU"), "RTX 4060");
        assert_eq!(short_name("Radeon 780M"), "Radeon 780M");
        assert_eq!(short_name("AMD Radeon 780M Graphics"), "AMD Radeon 780M");
        assert_eq!(
            short_name("NVIDIA GeForce RTX 4060 Max-Q / Mobile"),
            "RTX 4060"
        );
    }

    #[test]
    fn parses_amd_clock_line() {
        let path = std::env::temp_dir().join("rsztui-test-sclk");
        fs::write(&path, "0: 800Mhz *\n1: 1100Mhz \n2: 2700Mhz \n").unwrap();
        assert_eq!(amd_clock(&path), Some(800));
        let _ = fs::remove_file(path);
    }
}
