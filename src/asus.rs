use std::collections::HashMap;
use std::fs;
use std::path::Path;

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

use crate::util;

/// asusctl 通过 DBus 写入失败时只把 `Error: ...` 打到 stdout，退出码仍然是 0。
/// 只检查退出码会把失败当成功，因此所有 asusctl 调用统一走这里。
fn ctl(args: &[&str]) -> Result<String> {
    let out = util::run("asusctl", args)?;
    if let Some(line) = out
        .lines()
        .find(|line| line.trim_start().starts_with("Error:"))
    {
        let detail = line.trim_start().trim_start_matches("Error:").trim();
        bail!("asusctl {} 失败：{detail}", args.join(" "));
    }
    Ok(out)
}

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
    let out = ctl(&["profile", "get"])?;
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
    ctl(&["profile", "set", profile.asus()])?;
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
    ctl(&["profile", "set", flag, profile.asus()])?;
    Ok(())
}

pub fn profile_tuning() -> Result<bool> {
    let out = ctl(&["profile", "tuning"])?;
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
    pub energy_now_wh: Option<f32>,
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

    /// 按当前放电功率估算剩余时间（小时）。
    pub fn hours_left(&self) -> Option<f32> {
        if self.status != "Discharging" {
            return None;
        }
        let now = self.energy_now_wh?;
        let power = self.power_w?;
        (power > 0.1).then(|| now / power)
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
        && let Ok(out) = ctl(&["battery", "info"])
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

        info.energy_now_wh = read_sys(&bat.join("energy_now"))
            .and_then(|v| v.parse::<f64>().ok())
            .map(|v| (v / 1_000_000.0) as f32)
            .or_else(|| {
                nominal_v.and_then(|volts| {
                    read_sys(&bat.join("charge_now"))
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
    ctl(&["battery", "limit", &percent.to_string()])?;
    Ok(())
}

pub fn battery_oneshot(percent: Option<u8>) -> Result<()> {
    match percent {
        Some(p) => {
            ctl(&["battery", "oneshot", &p.to_string()])?;
        }
        None => {
            ctl(&["battery", "oneshot"])?;
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
    let out = ctl(&["armoury", "list"])?;
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
    ctl(&["armoury", "set", name, &value.to_string()])?;
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

impl PowerField {
    /// 固件给的是 `min == max` 时这个属性根本不可写（例如关掉 dGPU 后的 nv_tgp）。
    pub fn writable(&self) -> bool {
        self.max > self.min
    }

    /// asusd 存了值、固件却还停在旧值：说明上次写入被固件拒绝。
    pub fn stale(&self) -> bool {
        self.writable() && (self.value < self.min || self.value > self.max)
    }
}

/// 固件真值：`asus-armoury` 的 current/min/max_value。asusctl 的 `armoury list`
/// 报的是 asusd 存盘配置，可能落在固件范围外，所以两边都要看。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FirmwareRange {
    pub current: i32,
    pub min: i32,
    pub max: i32,
}

/// 固件属性目录；测试里用 ROGTUI_FW_ATTRS 指向临时目录。
fn firmware_attrs() -> std::path::PathBuf {
    std::env::var_os("ROGTUI_FW_ATTRS")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::PathBuf::from("/sys/class/firmware-attributes/asus-armoury/attributes")
        })
}

pub fn firmware_range(name: &str) -> Option<FirmwareRange> {
    let base = firmware_attrs();
    let read = |file: &str| -> Option<i32> {
        fs::read_to_string(base.join(name).join(file))
            .ok()?
            .trim()
            .parse::<i32>()
            .ok()
    };
    Some(FirmwareRange {
        current: read("current_value")?,
        min: read("min_value")?,
        max: read("max_value")?,
    })
}

/// 单次写入的结果：界面按字段汇总，不再「一错全废」。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetOutcome {
    Applied(i32),
    Skipped(&'static str),
    Failed(String),
}

pub fn set_power_field(field: &PowerField) -> SetOutcome {
    if !field.writable() {
        return SetOutcome::Skipped("固件不可调");
    }
    let want = field.value.clamp(field.min, field.max);
    if let Err(err) = armoury_set(field.key, want) {
        return SetOutcome::Failed(format!("{err:#}"));
    }
    // 回读校验：asusctl 退出码是 0 也可能没写进去。
    let mut actual = None;
    for _ in 0..3 {
        actual = firmware_range(field.key).map(|range| range.current);
        if actual == Some(want) {
            return SetOutcome::Applied(want);
        }
        std::thread::sleep(std::time::Duration::from_millis(40));
    }
    match actual {
        Some(value) if value != want => SetOutcome::Failed(format!("固件未生效（当前 {value}）")),
        _ => SetOutcome::Failed("写入后读不回来".to_string()),
    }
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
        // 优先用固件真值；读不到（非 ARMOURY 机型的老内核）才退回 asusctl。
        let range = firmware_range(key);
        fields.push(PowerField {
            key,
            label,
            unit,
            min: range.map(|r| r.min).unwrap_or_else(|| attr.min()),
            max: range.map(|r| r.max).unwrap_or_else(|| attr.max()),
            value: range.map(|r| r.current).unwrap_or_else(|| attr.current()),
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
    // 不带 --fan：一次读回 CPU 与 GPU，避免不同 asusctl 版本把查询过滤成单条曲线。
    let out = ctl(&["fan-curve", "--mod-profile", profile.asus()])?;
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
    let out = ctl(&["fan-curve", "--get-enabled"])?;
    Ok(out.trim().to_string())
}

pub fn led_status() -> Result<String> {
    let out = ctl(&["leds", "get"])?;
    let value = out
        .split(':')
        .next_back()
        .map(|v| v.trim().to_string())
        .unwrap_or_else(|| "未知".into());
    Ok(value)
}

pub fn led_next() -> Result<()> {
    ctl(&["leds", "next"])?;
    Ok(())
}

/// 解析并校验 asusctl 的曲线格式 `43c:30,46c:30,...`。
/// asusd 要求温度严格递增、PWM 逐点不降，违反时只会报错、退出码仍是 0。
fn parse_fan_points(data: &str) -> Result<(Vec<u8>, Vec<u8>)> {
    let mut temps = Vec::new();
    let mut pwms = Vec::new();
    for point in data.split(',') {
        let Some((temp, pwm)) = point.split_once('c') else {
            bail!("风扇曲线格式不正确：{point}");
        };
        let temp: u8 = temp
            .trim()
            .parse()
            .map_err(|_| anyhow::anyhow!("温度不是数字：{point}"))?;
        let pwm: u8 = pwm
            .trim()
            .trim_start_matches(':')
            .parse()
            .map_err(|_| anyhow::anyhow!("PWM 不是数字：{point}"))?;
        temps.push(temp);
        pwms.push(pwm);
    }
    if temps.len() < 2 {
        bail!("风扇曲线至少需要两个点");
    }
    for pair in temps.windows(2) {
        if pair[1] <= pair[0] {
            bail!("温度必须严格递增：{}°C → {}°C", pair[0], pair[1]);
        }
    }
    for pair in pwms.windows(2) {
        if pair[1] < pair[0] {
            bail!("PWM 必须逐点不降：{} → {}", pair[0], pair[1]);
        }
    }
    Ok((temps, pwms))
}

/// 回读校验：写入后曲线必须和提交的一致，且该风扇确实处于启用状态。
fn verify_fan_curve(profile: Profile, fan: &str, data: &str) -> Result<()> {
    let (temps, pwms) = parse_fan_points(data)?;
    let curves = fan_curves(profile)?;
    let curve = curves
        .iter()
        .find(|c| c.fan.eq_ignore_ascii_case(fan))
        .ok_or_else(|| anyhow::anyhow!("写入后没有读到 {fan} 曲线"))?;
    if curve.temp != temps || curve.pwm != pwms {
        bail!("{fan} 曲线写入未生效（回读 PWM {:?}）", curve.pwm);
    }
    if !curve.enabled {
        bail!("{fan} 曲线已写入但未启用");
    }
    Ok(())
}

pub fn fan_apply(profile: Profile, cpu: &str, gpu: &str) -> Result<()> {
    parse_fan_points(cpu)?;
    parse_fan_points(gpu)?;
    profile_set(profile)?;
    ctl(&[
        "fan-curve",
        "--mod-profile",
        profile.asus(),
        "--fan",
        "cpu",
        "--data",
        cpu,
    ])?;
    ctl(&[
        "fan-curve",
        "--mod-profile",
        profile.asus(),
        "--fan",
        "gpu",
        "--data",
        gpu,
    ])?;
    for (fan, data) in [("cpu", cpu), ("gpu", gpu)] {
        ctl(&[
            "fan-curve",
            "--mod-profile",
            profile.asus(),
            "--fan",
            fan,
            "--enable-fan-curve",
            "true",
        ])?;
        verify_fan_curve(profile, fan, data)?;
    }
    Ok(())
}

pub fn fan_set_curve(profile: Profile, fan: &str, data: &str) -> Result<()> {
    parse_fan_points(data)?;
    ctl(&[
        "fan-curve",
        "--mod-profile",
        profile.asus(),
        "--fan",
        fan,
        "--data",
        data,
    ])?;
    // 只启用当前风扇，避免把用户特意停用的另一条曲线重新打开。
    ctl(&[
        "fan-curve",
        "--mod-profile",
        profile.asus(),
        "--fan",
        fan,
        "--enable-fan-curve",
        "true",
    ])?;
    verify_fan_curve(profile, fan, data)?;
    Ok(())
}

pub fn fan_default(profile: Profile) -> Result<()> {
    profile_set(profile)?;
    ctl(&["fan-curve", "--default"])?;
    // asusctl 的 --default 会把曲线置为未启用；这里重新启用，
    // 避免「恢复默认」顺带关掉用户的风扇控制、界面显示成「停用」。
    for fan in ["cpu", "gpu"] {
        ctl(&[
            "fan-curve",
            "--mod-profile",
            profile.asus(),
            "--fan",
            fan,
            "--enable-fan-curve",
            "true",
        ])?;
    }
    if fan_curves(profile)?.iter().any(|c| !c.enabled) {
        bail!("{} 风扇曲线已恢复默认，但未全部启用", profile.asus());
    }
    Ok(())
}

pub fn fan_set_enabled(profile: Profile, enabled: bool) -> Result<()> {
    ctl(&[
        "fan-curve",
        "--mod-profile",
        profile.asus(),
        "--enable-fan-curves",
        if enabled { "true" } else { "false" },
    ])?;
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
    ctl(&["aura", "effect", "--next-mode"])?;
    Ok(())
}

pub fn aura_prev() -> Result<()> {
    ctl(&["aura", "effect", "--prev-mode"])?;
    Ok(())
}

pub fn aura_static(colour: &str) -> Result<()> {
    ctl(&["aura", "effect", "static", "-c", &bare_hex(colour)])?;
    Ok(())
}

/// asusctl 的颜色解析不认 `#` 前缀，界面里到处是 `#rrggbb`，写入前统一剥掉。
pub fn bare_hex(colour: &str) -> String {
    colour.trim().trim_start_matches('#').to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_hash_from_aura_colour() {
        assert_eq!(bare_hex("#FF1A1A"), "ff1a1a");
        assert_eq!(bare_hex("00ff00"), "00ff00");
        assert_eq!(bare_hex(" #AbC "), "abc");
    }

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
    fn validates_fan_curve_points() {
        let (temps, pwms) = parse_fan_points("43c:30,46c:30,52c:50").unwrap();
        assert_eq!(temps, vec![43, 46, 52]);
        assert_eq!(pwms, vec![30, 30, 50]);
    }

    #[test]
    fn rejects_decreasing_pwm() {
        // asusd 拒绝相邻点 PWM 下降，这个错误必须在这里就拦下来。
        let err = parse_fan_points("43c:50,46c:30").unwrap_err();
        assert!(err.to_string().contains("PWM 必须逐点不降"));
    }

    #[test]
    fn rejects_non_increasing_temps() {
        let err = parse_fan_points("43c:30,43c:40").unwrap_err();
        assert!(err.to_string().contains("温度必须严格递增"));
    }

    #[test]
    fn rejects_malformed_points() {
        assert!(parse_fan_points("43:30").is_err());
        assert!(parse_fan_points("43c:abc").is_err());
        assert!(parse_fan_points("43c:30").is_err());
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

#[cfg(test)]
mod firmware_tests {
    use super::*;

    fn attr(root: &std::path::Path, name: &str, current: &str, min: &str, max: &str) {
        let dir = root.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        for (file, value) in [
            ("current_value", current),
            ("min_value", min),
            ("max_value", max),
        ] {
            std::fs::write(dir.join(file), format!("{value}\n")).unwrap();
        }
    }

    struct EnvGuard(String);
    impl Drop for EnvGuard {
        fn drop(&mut self) {
            unsafe { std::env::remove_var(&self.0) }
        }
    }

    #[test]
    fn reads_firmware_truth_and_flags_unwritable() {
        let root = std::env::temp_dir().join(format!("rogtui-fw-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        attr(&root, "ppt_pl2_sppt", "30", "35", "65");
        attr(&root, "nv_tgp", "55", "0", "0");
        // SAFETY: 测试单线程改动本进程环境变量。
        unsafe { std::env::set_var("ROGTUI_FW_ATTRS", &root) };
        let _guard = EnvGuard("ROGTUI_FW_ATTRS".into());

        let pl2 = firmware_range("ppt_pl2_sppt").expect("应读到固件范围");
        assert_eq!((pl2.current, pl2.min, pl2.max), (30, 35, 65));
        let mut field = PowerField {
            key: "ppt_pl2_sppt",
            label: "测试",
            unit: "W",
            value: 30,
            min: 35,
            max: 65,
        };
        assert!(field.writable(), "min < max 应可写");
        // 存盘值 30 低于固件下限 35 → 越界提示
        field.value = 30;
        assert!(field.stale(), "30 低于固件下限，应判越界");
        field.value = 45;
        assert!(!field.stale(), "45 在 35..65 内不算越界");
        let nv = firmware_range("nv_tgp").unwrap();
        assert_eq!(nv.min, nv.max, "dGPU 关闭时固件 min=max=0");
        let nv_field = PowerField {
            key: "nv_tgp",
            label: "TGP",
            unit: "W",
            value: 55,
            min: nv.min,
            max: nv.max,
        };
        assert!(!nv_field.writable(), "min == max 不可写");
        // 不可写时界面走「固件不可调」分支，不再判越界
        assert!(!nv_field.stale());
        assert!(matches!(set_power_field(&nv_field), SetOutcome::Skipped(_)));

        std::fs::remove_dir_all(&root).ok();
    }
}
