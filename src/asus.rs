use std::collections::HashMap;
use std::fs;
use std::path::Path;

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

use crate::util;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Profile {
    Quiet,
    Balanced,
    Performance,
}

impl Profile {
    pub const ALL: [Profile; 3] = [Profile::Quiet, Profile::Balanced, Profile::Performance];

    pub fn asus(self) -> &'static str {
        match self {
            Profile::Quiet => "Quiet",
            Profile::Balanced => "Balanced",
            Profile::Performance => "Performance",
        }
    }

    pub fn zh(self) -> &'static str {
        match self {
            Profile::Quiet => "静音",
            Profile::Balanced => "均衡",
            Profile::Performance => "性能",
        }
    }

    pub fn from_asus(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "quiet" => Some(Profile::Quiet),
            "balanced" => Some(Profile::Balanced),
            "performance" => Some(Profile::Performance),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct ProfileState {
    pub active: Profile,
    pub ac: Profile,
    pub battery: Profile,
}

pub fn profile_get() -> Result<ProfileState> {
    let out = util::run("asusctl", &["profile", "get"])?;
    let mut active = None;
    let mut ac = None;
    let mut battery = None;

    for line in out.lines() {
        if let Some(v) = line.strip_prefix("Active profile:") {
            active = Profile::from_asus(v);
        } else if let Some(v) = line.strip_prefix("AC profile") {
            ac = Profile::from_asus(v);
        } else if let Some(v) = line.strip_prefix("Battery profile") {
            battery = Profile::from_asus(v);
        }
    }

    match (active, ac, battery) {
        (Some(active), Some(ac), Some(battery)) => Ok(ProfileState {
            active,
            ac,
            battery,
        }),
        _ => bail!("无法解析 asusctl profile get 的输出：{out}"),
    }
}

pub fn profile_set(profile: Profile) -> Result<()> {
    util::run("asusctl", &["profile", "set", profile.asus()])?;
    Ok(())
}

#[derive(Debug, Clone, Copy)]
pub enum PowerSource {
    Ac,
    Battery,
}

pub fn profile_set_default(source: PowerSource, profile: Profile) -> Result<()> {
    let flag = match source {
        PowerSource::Ac => "--ac",
        PowerSource::Battery => "--battery",
    };
    util::run("asusctl", &["profile", "set", flag, profile.asus()])?;
    Ok(())
}

pub fn profile_tuning() -> Result<bool> {
    let out = util::run("asusctl", &["profile", "tuning"])?;
    Ok(out.to_ascii_lowercase().contains("true"))
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct BatteryInfo {
    pub limit: Option<u8>,
    pub capacity: Option<u8>,
    pub status: String,
    pub power_w: Option<f32>,
    pub voltage_v: Option<f32>,
    pub energy_full_wh: Option<f32>,
    pub energy_full_design_wh: Option<f32>,
    pub cycle_count: Option<u32>,
    pub ac_online: Option<bool>,
    pub manufacturer: String,
    pub model: String,
    pub technology: String,
}

impl BatteryInfo {
    pub fn status_zh(&self) -> &'static str {
        match self.status.as_str() {
            "Charging" => "充电中",
            "Discharging" => "放电中",
            "Full" => "已充满",
            "Not charging" => "未充电",
            _ => "未知",
        }
    }

    pub fn health_pct(&self) -> Option<f32> {
        match (self.energy_full_wh, self.energy_full_design_wh) {
            (Some(full), Some(design)) if design > 0.0 => Some(full / design * 100.0),
            _ => None,
        }
    }
}

pub fn battery_info() -> Result<BatteryInfo> {
    let mut info = BatteryInfo::default();
    let bat = battery_dir();

    if let Some(bat) = &bat {
        info.limit =
            read_sys(&bat.join("charge_control_end_threshold")).and_then(|v| v.parse().ok());
    }

    if info.limit.is_none()
        && let Ok(out) = util::run("asusctl", &["battery", "info"])
    {
        for line in out.lines() {
            if let Some(v) = line.strip_prefix("Current battery charge limit:") {
                info.limit = v.trim().trim_end_matches('%').parse().ok();
            }
        }
    }

    if let Some(bat) = bat {
        info.capacity = read_sys(&bat.join("capacity")).and_then(|v| v.parse().ok());
        info.status = read_sys(&bat.join("status")).unwrap_or_else(|| "未知".into());
        info.cycle_count = read_sys(&bat.join("cycle_count")).and_then(|v| v.parse().ok());
        info.manufacturer = read_sys(&bat.join("manufacturer")).unwrap_or_default();
        info.model = read_sys(&bat.join("model_name")).unwrap_or_default();
        info.technology = read_sys(&bat.join("technology")).unwrap_or_default();

        let voltage_uv = read_sys(&bat.join("voltage_now")).and_then(|v| v.parse::<f64>().ok());
        info.voltage_v = voltage_uv.map(|v| (v / 1_000_000.0) as f32);

        let power_uw = read_sys(&bat.join("power_now")).and_then(|v| v.parse::<f64>().ok());
        info.power_w = match (power_uw, voltage_uv) {
            (Some(p), _) if p > 0.0 => Some((p / 1_000_000.0) as f32),
            (_, Some(v)) => read_sys(&bat.join("current_now"))
                .and_then(|c| c.parse::<f64>().ok())
                .map(|c| (c * v / 1_000_000_000_000.0) as f32),
            _ => None,
        };

        let nominal_v = read_sys(&bat.join("voltage_min_design"))
            .and_then(|v| v.parse::<f64>().ok())
            .map(|v| v / 1_000_000.0);

        info.energy_full_wh = read_sys(&bat.join("energy_full"))
            .and_then(|v| v.parse::<f64>().ok())
            .map(|v| (v / 1_000_000.0) as f32)
            .or_else(|| {
                nominal_v.and_then(|volts| {
                    read_sys(&bat.join("charge_full"))
                        .and_then(|v| v.parse::<f64>().ok())
                        .map(|charge| (charge / 1_000_000.0 * volts) as f32)
                })
            });
        info.energy_full_design_wh = read_sys(&bat.join("energy_full_design"))
            .and_then(|v| v.parse::<f64>().ok())
            .map(|v| (v / 1_000_000.0) as f32)
            .or_else(|| {
                nominal_v.and_then(|volts| {
                    read_sys(&bat.join("charge_full_design"))
                        .and_then(|v| v.parse::<f64>().ok())
                        .map(|charge| (charge / 1_000_000.0 * volts) as f32)
                })
            });
    }

    info.ac_online = ac_online();

    Ok(info)
}

fn ac_online() -> Option<bool> {
    let entries = fs::read_dir("/sys/class/power_supply").ok()?;
    for entry in entries.filter_map(|e| e.ok()) {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !(name.starts_with("AC") || name.starts_with("ADP")) {
            continue;
        }
        if let Some(value) = read_sys(&entry.path().join("online")) {
            return value.parse::<u8>().ok().map(|v| v != 0);
        }
    }
    None
}

fn battery_dir() -> Option<std::path::PathBuf> {
    let entries = fs::read_dir("/sys/class/power_supply").ok()?;
    entries.filter_map(|e| e.ok()).map(|e| e.path()).find(|p| {
        p.file_name()
            .map(|n| n.to_string_lossy().starts_with("BAT"))
            .unwrap_or(false)
    })
}

fn read_sys(path: &Path) -> Option<String> {
    fs::read_to_string(path).ok().map(|v| v.trim().to_string())
}

pub fn battery_limit(percent: u8) -> Result<()> {
    if !(20..=100).contains(&percent) {
        bail!("电池上限必须在 20-100 之间");
    }
    util::run("asusctl", &["battery", "limit", &percent.to_string()])?;
    Ok(())
}

pub fn battery_oneshot(percent: Option<u8>) -> Result<()> {
    match percent {
        Some(p) => {
            util::run("asusctl", &["battery", "oneshot", &p.to_string()])?;
        }
        None => {
            util::run("asusctl", &["battery", "oneshot"])?;
        }
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub enum AttrKind {
    Range { min: i32, cur: i32, max: i32 },
    Options { values: Vec<i32>, cur: i32 },
    Scalar(i32),
}

#[derive(Debug, Clone)]
pub struct Attr {
    pub name: String,
    pub kind: AttrKind,
}

impl Attr {
    pub fn current(&self) -> i32 {
        match &self.kind {
            AttrKind::Range { cur, .. } => *cur,
            AttrKind::Options { cur, .. } => *cur,
            AttrKind::Scalar(v) => *v,
        }
    }

    pub fn min(&self) -> i32 {
        match &self.kind {
            AttrKind::Range { min, .. } => *min,
            AttrKind::Options { values, .. } => values.iter().copied().min().unwrap_or(0),
            AttrKind::Scalar(v) => *v,
        }
    }

    pub fn max(&self) -> i32 {
        match &self.kind {
            AttrKind::Range { max, .. } => *max,
            AttrKind::Options { values, .. } => values.iter().copied().max().unwrap_or(0),
            AttrKind::Scalar(v) => *v,
        }
    }
}

pub fn armoury() -> Result<Vec<Attr>> {
    let out = util::run("asusctl", &["armoury", "list"])?;
    let mut attrs = Vec::new();
    let mut pending_name: Option<String> = None;

    for line in out.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if !line.starts_with(' ') && trimmed.ends_with(':') {
            pending_name = Some(trimmed.trim_end_matches(':').to_string());
            continue;
        }

        if let (Some(name), Some(raw)) = (&pending_name, trimmed.strip_prefix("current:"))
            && let Some(kind) = parse_attr_value(raw)
        {
            attrs.push(Attr {
                name: name.clone(),
                kind,
            });
            pending_name = None;
        }
    }

    if attrs.is_empty() {
        bail!("asusctl armoury list 没有返回可用属性");
    }
    Ok(attrs)
}

fn parse_attr_value(raw: &str) -> Option<AttrKind> {
    let raw = raw.trim();

    if let Some(inner) = raw.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
        let mut values = Vec::new();
        let mut current = None;
        for token in inner.split(',') {
            let token = token.trim();
            if token.is_empty() {
                continue;
            }
            if let Some(active) = token.strip_prefix('(').and_then(|s| s.strip_suffix(')')) {
                current = active.parse::<i32>().ok();
            } else {
                values.push(token.parse::<i32>().ok()?);
            }
        }
        let cur = current.or_else(|| values.first().copied())?;
        return Some(AttrKind::Options { values, cur });
    }

    if raw.contains("..") {
        let mut parts = raw.split("..");
        let min = parts.next()?.trim().parse::<i32>().ok()?;
        let cur = parts
            .next()?
            .trim()
            .trim_start_matches('[')
            .trim_end_matches(']')
            .parse::<i32>()
            .ok()?;
        let max = parts.next()?.trim().parse::<i32>().ok()?;
        return Some(AttrKind::Range { min, cur, max });
    }

    raw.parse::<i32>().ok().map(AttrKind::Scalar)
}

pub fn attr_map(attrs: &[Attr]) -> HashMap<&str, &Attr> {
    attrs.iter().map(|a| (a.name.as_str(), a)).collect()
}

pub fn armoury_set(name: &str, value: i32) -> Result<()> {
    util::run("asusctl", &["armoury", "set", name, &value.to_string()])?;
    Ok(())
}

#[derive(Debug, Clone)]
pub struct PowerField {
    pub key: &'static str,
    pub label: &'static str,
    pub unit: &'static str,
    pub min: i32,
    pub max: i32,
    pub value: i32,
}

pub const POWER_FIELDS: [(&str, &str, &str); 6] = [
    ("ppt_pl1_spl", "CPU PL1（持续）", "W"),
    ("ppt_pl2_sppt", "CPU PL2（短时）", "W"),
    ("ppt_pl3_fppt", "CPU PL3（峰值）", "W"),
    ("nv_tgp", "dGPU TGP", "W"),
    ("nv_dynamic_boost", "Dynamic Boost", "W"),
    ("nv_temp_target", "dGPU 温度墙", "°C"),
];

pub fn power_fields(attrs: &[Attr]) -> Vec<PowerField> {
    let map = attr_map(attrs);
    let mut fields = Vec::new();
    for (key, label, unit) in POWER_FIELDS {
        let Some(attr) = map.get(key) else {
            continue;
        };
        fields.push(PowerField {
            key,
            label,
            unit,
            min: attr.min(),
            max: attr.max(),
            value: attr.current(),
        });
    }
    fields
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum GpuMode {
    Integrated,
    Hybrid,
    Ultimate,
}

impl GpuMode {
    pub const ALL: [GpuMode; 3] = [GpuMode::Integrated, GpuMode::Hybrid, GpuMode::Ultimate];

    pub fn zh(self) -> &'static str {
        match self {
            GpuMode::Integrated => "集显模式",
            GpuMode::Hybrid => "混合模式",
            GpuMode::Ultimate => "独显直连",
        }
    }

    pub fn detail(self) -> &'static str {
        match self {
            GpuMode::Integrated => "dgpu_disable=1, gpu_mux_mode=1",
            GpuMode::Hybrid => "dgpu_disable=0, gpu_mux_mode=1",
            GpuMode::Ultimate => "dgpu_disable=0, gpu_mux_mode=0",
        }
    }
}

pub fn gpu_mode(attrs: &[Attr]) -> Option<GpuMode> {
    let map = attr_map(attrs);
    let dgpu = map.get("dgpu_disable")?.current();
    let mux = map.get("gpu_mux_mode")?.current();

    match (dgpu, mux) {
        (1, _) => Some(GpuMode::Integrated),
        (0, 0) => Some(GpuMode::Ultimate),
        (0, _) => Some(GpuMode::Hybrid),
        _ => None,
    }
}

pub fn gpu_set(mode: GpuMode) -> Result<()> {
    match mode {
        GpuMode::Integrated => {
            armoury_set("gpu_mux_mode", 1)?;
            armoury_set("dgpu_disable", 1)?;
        }
        GpuMode::Hybrid => {
            armoury_set("gpu_mux_mode", 1)?;
            armoury_set("dgpu_disable", 0)?;
        }
        GpuMode::Ultimate => {
            armoury_set("dgpu_disable", 0)?;
            armoury_set("gpu_mux_mode", 0)?;
        }
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub struct FanCurve {
    pub fan: String,
    pub temp: Vec<u8>,
    pub pwm: Vec<u8>,
    pub enabled: bool,
}

pub fn fan_curves(profile: Profile) -> Result<Vec<FanCurve>> {
    let out = util::run(
        "asusctl",
        &["fan-curve", "--mod-profile", profile.asus(), "--fan", "cpu"],
    )?;
    Ok(parse_fan_curves(&out))
}

fn parse_fan_curves(text: &str) -> Vec<FanCurve> {
    let mut curves: Vec<FanCurve> = Vec::new();
    let mut current: Option<FanCurve> = None;

    for line in text.lines() {
        let trimmed = line.trim().trim_end_matches(',');
        if let Some(fan) = trimmed.strip_prefix("fan:") {
            if let Some(curve) = current.take() {
                curves.push(curve);
            }
            current = Some(FanCurve {
                fan: fan.trim().to_string(),
                temp: Vec::new(),
                pwm: Vec::new(),
                enabled: false,
            });
        } else if let Some(value) = trimmed.strip_prefix("temp:") {
            if let Some(curve) = current.as_mut() {
                curve.temp = parse_tuple(value);
            }
        } else if let Some(value) = trimmed.strip_prefix("pwm:") {
            if let Some(curve) = current.as_mut() {
                curve.pwm = parse_tuple(value);
            }
        } else if let Some(value) = trimmed.strip_prefix("enabled:")
            && let Some(curve) = current.as_mut()
        {
            curve.enabled = value.trim().eq_ignore_ascii_case("true");
        }
    }

    if let Some(curve) = current {
        curves.push(curve);
    }
    curves
}

fn parse_tuple(value: &str) -> Vec<u8> {
    value
        .trim()
        .trim_start_matches('(')
        .trim_end_matches(')')
        .split(',')
        .filter_map(|v| v.trim().parse::<u8>().ok())
        .collect()
}

pub fn fan_enabled_summary() -> Result<String> {
    let out = util::run("asusctl", &["fan-curve", "--get-enabled"])?;
    Ok(out.trim().to_string())
}

pub fn led_status() -> Result<String> {
    let out = util::run("asusctl", &["leds", "get"])?;
    let value = out
        .split(':')
        .next_back()
        .map(|v| v.trim().to_string())
        .unwrap_or_else(|| "未知".into());
    Ok(value)
}

pub fn led_next() -> Result<()> {
    util::run("asusctl", &["leds", "next"])?;
    Ok(())
}

pub fn fan_apply(profile: Profile, cpu: &str, gpu: &str) -> Result<()> {
    profile_set(profile)?;
    util::run(
        "asusctl",
        &[
            "fan-curve",
            "--mod-profile",
            profile.asus(),
            "--fan",
            "cpu",
            "--data",
            cpu,
        ],
    )?;
    util::run(
        "asusctl",
        &[
            "fan-curve",
            "--mod-profile",
            profile.asus(),
            "--fan",
            "gpu",
            "--data",
            gpu,
        ],
    )?;
    util::run(
        "asusctl",
        &[
            "fan-curve",
            "--mod-profile",
            profile.asus(),
            "--enable-fan-curves",
            "true",
        ],
    )?;
    Ok(())
}

pub fn fan_set_curve(profile: Profile, fan: &str, data: &str) -> Result<()> {
    util::run(
        "asusctl",
        &[
            "fan-curve",
            "--mod-profile",
            profile.asus(),
            "--fan",
            fan,
            "--data",
            data,
        ],
    )?;
    util::run(
        "asusctl",
        &[
            "fan-curve",
            "--mod-profile",
            profile.asus(),
            "--enable-fan-curves",
            "true",
        ],
    )?;
    Ok(())
}

pub fn fan_default(profile: Profile) -> Result<()> {
    profile_set(profile)?;
    util::run("asusctl", &["fan-curve", "--default"])?;
    Ok(())
}

pub fn fan_set_enabled(profile: Profile, enabled: bool) -> Result<()> {
    util::run(
        "asusctl",
        &[
            "fan-curve",
            "--mod-profile",
            profile.asus(),
            "--enable-fan-curves",
            if enabled { "true" } else { "false" },
        ],
    )?;
    Ok(())
}

/// 把曲线转成 asusctl 需要的原始 PWM 格式："31c:26,50c:38,..."。
/// 省略 % 时 asusctl 按 0-255 处理，避免百分比换算带来的舍入误差。
pub fn fan_data(curves: &[FanCurve]) -> Option<(String, String)> {
    let mut cpu = None;
    let mut gpu = None;

    for curve in curves {
        let points: Vec<String> = curve
            .temp
            .iter()
            .zip(curve.pwm.iter())
            .map(|(temp, pwm)| format!("{temp}c:{pwm}"))
            .collect();
        let data = points.join(",");
        if curve.fan.eq_ignore_ascii_case("cpu") {
            cpu = Some(data);
        } else if curve.fan.eq_ignore_ascii_case("gpu") {
            gpu = Some(data);
        }
    }

    Some((cpu?, gpu?))
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct AuraStatus {
    pub mode: u32,
    pub mode_name: String,
    pub mode_zh: String,
    pub brightness: u8,
    pub brightness_name: String,
    pub color1: String,
    pub color2: String,
    pub speed: String,
    pub zone: String,
    pub supported: Vec<u32>,
}

pub fn aura_status() -> Result<AuraStatus> {
    let out = util::run(
        "busctl",
        &[
            "--system",
            "--json=short",
            "call",
            "xyz.ljones.Asusd",
            "/xyz/ljones/aura/tuf",
            "org.freedesktop.DBus.Properties",
            "GetAll",
            "s",
            "xyz.ljones.Aura",
        ],
    )?;
    let value: serde_json::Value = serde_json::from_str(&out)?;
    let data = value
        .get("data")
        .and_then(|v| v.get(0))
        .ok_or_else(|| anyhow::anyhow!("Aura GetAll 返回格式不对"))?;

    let mode = data
        .get("LedMode")
        .and_then(|v| v.get("data"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    let brightness = data
        .get("Brightness")
        .and_then(|v| v.get("data"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u8;
    let supported = data
        .get("SupportedBasicModes")
        .and_then(|v| v.get("data"))
        .and_then(|v| v.as_array())
        .map(|v| {
            v.iter()
                .filter_map(|n| n.as_u64().map(|n| n as u32))
                .collect()
        })
        .unwrap_or_default();

    let md = data
        .get("LedModeData")
        .and_then(|v| v.get("data"))
        .and_then(|v| v.as_array());
    let color1 = md
        .and_then(|v| v.get(2))
        .map(rgb_hex)
        .unwrap_or_else(|| "#000000".into());
    let color2 = md
        .and_then(|v| v.get(3))
        .map(rgb_hex)
        .unwrap_or_else(|| "#000000".into());
    let speed = md
        .and_then(|v| v.get(4))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let zone = md
        .and_then(|v| v.get(5))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    Ok(AuraStatus {
        mode,
        mode_name: aura_mode_name(mode).into(),
        mode_zh: aura_mode_zh(mode).into(),
        brightness,
        brightness_name: brightness_name(brightness).into(),
        color1,
        color2,
        speed,
        zone,
        supported,
    })
}

fn rgb_hex(value: &serde_json::Value) -> String {
    let arr = value.as_array();
    let get = |i: usize| {
        arr.and_then(|a| a.get(i))
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u8
    };
    format!("#{:02x}{:02x}{:02x}", get(0), get(1), get(2))
}

pub fn aura_mode_name(mode: u32) -> &'static str {
    match mode {
        0 => "Static",
        1 => "Breathe",
        2 => "RainbowCycle",
        3 => "RainbowWave",
        4 => "Stars",
        5 => "Rain",
        6 => "Highlight",
        7 => "Laser",
        8 => "Ripple",
        10 => "Pulse",
        11 => "Comet",
        12 => "Flash",
        _ => "Unknown",
    }
}

pub fn aura_mode_zh(mode: u32) -> &'static str {
    match mode {
        0 => "静态",
        1 => "呼吸",
        2 => "彩虹循环",
        3 => "彩虹波浪",
        4 => "星光",
        5 => "雨滴",
        6 => "高亮",
        7 => "激光",
        8 => "涟漪",
        10 => "脉冲",
        11 => "彗星",
        12 => "闪烁",
        _ => "未知",
    }
}

pub fn brightness_name(level: u8) -> &'static str {
    match level {
        0 => "Off",
        1 => "Low",
        2 => "Med",
        3 => "High",
        _ => "未知",
    }
}

pub fn aura_next() -> Result<()> {
    util::run("asusctl", &["aura", "effect", "--next-mode"])?;
    Ok(())
}

pub fn aura_prev() -> Result<()> {
    util::run("asusctl", &["aura", "effect", "--prev-mode"])?;
    Ok(())
}

pub fn aura_static(colour: &str) -> Result<()> {
    util::run("asusctl", &["aura", "effect", "static", "-c", colour])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_range_value() {
        let kind = parse_attr_value("15..[55]..80").unwrap();
        match kind {
            AttrKind::Range { min, cur, max } => {
                assert_eq!((min, cur, max), (15, 55, 80));
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn parses_option_value() {
        let kind = parse_attr_value("[0,(1),2]").unwrap();
        match kind {
            AttrKind::Options { values, cur } => {
                assert_eq!(values, vec![0, 2]);
                assert_eq!(cur, 1);
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn parses_scalar_value() {
        let kind = parse_attr_value("55").unwrap();
        assert!(matches!(kind, AttrKind::Scalar(55)));
    }

    #[test]
    fn parses_fan_curves() {
        let text = r#"
Fan curves for Performance

[
    (
        fan: CPU,
        pwm: (26, 38, 76, 110, 148, 189, 224, 255),
        temp: (31, 50, 70, 72, 74, 80, 86, 92),
        enabled: true,
    ),
    (
        fan: GPU,
        pwm: (26, 38, 61, 92, 133, 173, 214, 250),
        temp: (40, 52, 62, 68, 74, 80, 86, 92),
        enabled: false,
    ),
]
"#;
        let curves = parse_fan_curves(text);
        assert_eq!(curves.len(), 2);
        assert_eq!(curves[0].fan, "CPU");
        assert!(curves[0].enabled);
        assert_eq!(curves[0].temp.len(), 8);
        assert_eq!(curves[1].fan, "GPU");
        assert!(!curves[1].enabled);
    }

    #[test]
    fn parses_profile_names() {
        assert_eq!(
            Profile::from_asus("Performance"),
            Some(Profile::Performance)
        );
        assert_eq!(Profile::from_asus(" quiet "), Some(Profile::Quiet));
        assert_eq!(Profile::from_asus("unknown"), None);
    }
}
