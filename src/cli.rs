use anyhow::{Result, bail};
use clap::{Parser, Subcommand, ValueEnum};
use serde_json::json;

use crate::asus::{self, GpuMode, PowerSource, Profile};
use crate::config::{Config, FanPreset};
use crate::deps;
use crate::nm;

#[derive(Parser)]
#[command(
    name = "rsztui",
    version,
    about = "ASUS TUF 终端控制台",
    long_about = "性能模式、电池上限、功耗、风扇、灯光与 NetworkManager 的终端控制台。无子命令时启动 TUI。"
)]
pub struct Cli {
    /// 以 JSON 输出
    #[arg(long, global = true)]
    pub json: bool,

    #[command(subcommand)]
    pub command: Option<Cmd>,
}

#[derive(Subcommand)]
pub enum Cmd {
    /// 汇总当前状态
    Status,
    /// 性能模式
    Profile {
        #[command(subcommand)]
        cmd: ProfileCmd,
    },
    /// 电池上限与信息
    Battery {
        #[command(subcommand)]
        cmd: BatteryCmd,
    },
    /// 功耗预设与手动值
    Power {
        #[command(subcommand)]
        cmd: PowerCmd,
    },
    /// GPU 模式（需要重启）
    Gpu {
        #[command(subcommand)]
        cmd: GpuCmd,
    },
    /// 网络管理
    Net {
        #[command(subcommand)]
        cmd: NetCmd,
    },
    /// 风扇曲线与预设
    Fan {
        #[command(subcommand)]
        cmd: FanCmd,
    },
    /// 键盘灯效
    Aura {
        #[command(subcommand)]
        cmd: AuraCmd,
    },
    /// 显示配置路径与预设
    Config,
    /// 检查 asusctl/asusd/NetworkManager 依赖
    Doctor,
}

#[derive(Subcommand)]
pub enum ProfileCmd {
    /// 查看当前与 AC/电池默认模式
    Get,
    /// 切换当前性能模式
    Set {
        #[arg(value_enum)]
        profile: ProfileArg,
    },
    /// 设置 AC 或电池供电时的默认模式
    Default {
        #[arg(value_enum)]
        source: SourceArg,
        #[arg(value_enum)]
        profile: ProfileArg,
    },
}

#[derive(Subcommand)]
pub enum BatteryCmd {
    /// 查看电池信息与上限
    Info,
    /// 设置充电上限（20-100）
    Limit { percent: u8 },
    /// 单次充满（可选目标百分比）
    Oneshot { percent: Option<u8> },
}

#[derive(Subcommand)]
pub enum PowerCmd {
    /// 套用与性能模式同名的功耗预设
    Preset {
        #[arg(value_enum)]
        profile: ProfileArg,
    },
    /// 手动设置功耗值（只传需要修改的项）
    Apply {
        #[arg(long)]
        pl1: Option<i32>,
        #[arg(long)]
        pl2: Option<i32>,
        #[arg(long)]
        pl3: Option<i32>,
        #[arg(long)]
        gpu_tgp: Option<i32>,
        #[arg(long)]
        gpu_boost: Option<i32>,
        #[arg(long)]
        gpu_temp: Option<i32>,
    },
}

#[derive(Subcommand)]
pub enum GpuCmd {
    /// 查看当前 GPU 模式
    Get,
    /// 切换 GPU 模式（修改后需要重启）
    Set {
        #[arg(value_enum)]
        mode: GpuArg,
        /// 确认修改并接受重启要求
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Subcommand)]
pub enum NetCmd {
    /// 汇总网络状态
    Status,
    /// Wi-Fi 操作
    Wifi {
        #[command(subcommand)]
        cmd: WifiCmd,
    },
    /// 无线开关
    Radio {
        #[arg(value_enum)]
        state: OnOff,
    },
    /// IPv4 / DNS 设置
    Ip {
        #[command(subcommand)]
        cmd: IpCmd,
    },
    /// VPN
    Vpn {
        #[command(subcommand)]
        cmd: VpnCmd,
    },
    /// 普通连接操作
    Conn {
        #[command(subcommand)]
        cmd: ConnCmd,
    },
    /// 热点
    Hotspot {
        #[command(subcommand)]
        cmd: HotspotCmd,
    },
}

#[derive(Subcommand)]
pub enum WifiCmd {
    /// 扫描并列出 Wi-Fi
    List,
    /// 连接（密码写入 NM keyfile，不进 argv）
    Connect {
        ssid: String,
        #[arg(long)]
        password: Option<String>,
        #[arg(long, default_value = "wpa-psk")]
        key_mgmt: String,
        /// 只创建/更新连接，不立即激活
        #[arg(long)]
        no_up: bool,
    },
}

#[derive(Subcommand)]
pub enum IpCmd {
    /// 查看连接的 IPv4/DNS
    Show { conn: String },
    /// 修改连接（只传需要改的项）
    Set {
        conn: String,
        #[arg(long)]
        method: Option<String>,
        #[arg(long)]
        address: Option<String>,
        #[arg(long)]
        gateway: Option<String>,
        #[arg(long)]
        dns: Option<String>,
        /// -1 默认, 0 关闭, 1 机会性, 2 强制
        #[arg(long)]
        dot: Option<i32>,
    },
}

#[derive(Subcommand)]
pub enum VpnCmd {
    List,
    Up { name: String },
    Down { name: String },
}

#[derive(Subcommand)]
pub enum ConnCmd {
    Up { name: String },
    Down { name: String },
    Delete { name: String },
}

#[derive(Subcommand)]
pub enum HotspotCmd {
    On {
        #[arg(long)]
        password: String,
        #[arg(long, default_value = "rsztui-hotspot")]
        ssid: String,
    },
    Off,
}

#[derive(Subcommand)]
pub enum FanCmd {
    /// 当前曲线与预设
    List,
    /// 把当前曲线保存为预设
    Save { name: String },
    /// 套用预设
    Apply { name: String },
    /// 删除预设
    Delete { name: String },
    /// 恢复当前模式的固件默认曲线
    Default,
    /// 启用当前模式的风扇曲线
    Enable,
    /// 停用当前模式的风扇曲线
    Disable,
}

#[derive(Subcommand)]
pub enum AuraCmd {
    Next,
    Prev,
    Static { colour: String },
}

#[derive(ValueEnum, Clone, Copy, Debug)]
pub enum OnOff {
    On,
    Off,
}

#[derive(ValueEnum, Clone, Copy, Debug)]
pub enum ProfileArg {
    Quiet,
    Balanced,
    Performance,
}

impl From<ProfileArg> for Profile {
    fn from(value: ProfileArg) -> Self {
        match value {
            ProfileArg::Quiet => Profile::Quiet,
            ProfileArg::Balanced => Profile::Balanced,
            ProfileArg::Performance => Profile::Performance,
        }
    }
}

#[derive(ValueEnum, Clone, Copy, Debug)]
pub enum SourceArg {
    Ac,
    Battery,
}

impl From<SourceArg> for PowerSource {
    fn from(value: SourceArg) -> Self {
        match value {
            SourceArg::Ac => PowerSource::Ac,
            SourceArg::Battery => PowerSource::Battery,
        }
    }
}

#[derive(ValueEnum, Clone, Copy, Debug)]
pub enum GpuArg {
    Integrated,
    Hybrid,
    Ultimate,
}

impl From<GpuArg> for GpuMode {
    fn from(value: GpuArg) -> Self {
        match value {
            GpuArg::Integrated => GpuMode::Integrated,
            GpuArg::Hybrid => GpuMode::Hybrid,
            GpuArg::Ultimate => GpuMode::Ultimate,
        }
    }
}

pub fn run(cmd: Cmd, json: bool) -> Result<()> {
    match cmd {
        Cmd::Status => status(json),
        Cmd::Profile { cmd } => profile(cmd, json),
        Cmd::Battery { cmd } => battery(cmd, json),
        Cmd::Power { cmd } => power(cmd, json),
        Cmd::Gpu { cmd } => gpu(cmd, json),
        Cmd::Net { cmd } => net(cmd, json),
        Cmd::Fan { cmd } => fan(cmd, json),
        Cmd::Aura { cmd } => aura(cmd, json),
        Cmd::Config => config(json),
        Cmd::Doctor => doctor(json),
    }
}

fn doctor(json: bool) -> Result<()> {
    let report = deps::check();
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("依赖检查 · {}", report.distro);
        println!();
        for check in &report.checks {
            println!(
                "{} {:<16} {}",
                if check.ok { "✅" } else { "❌" },
                check.name,
                check.detail
            );
            if !check.ok
                && let Some(hint) = &check.hint
            {
                for line in hint.lines() {
                    println!("   {line}");
                }
            }
        }
    }
    if !report.all_ok() {
        bail!("依赖检查未通过");
    }
    if report.has_warnings() {
        println!();
        println!("存在硬件/依赖警告：ASUS 功能可能不可用，网络功能不受影响。");
    }
    Ok(())
}

fn status(json: bool) -> Result<()> {
    let profile = asus::profile_get()?;
    let battery = asus::battery_info()?;
    let attrs = asus::armoury()?;
    let fields = asus::power_fields(&attrs);
    let gpu = asus::gpu_mode(&attrs);
    let tuning = asus::profile_tuning().unwrap_or(false);

    if json {
        let power: serde_json::Map<String, serde_json::Value> = fields
            .iter()
            .map(|f| (f.key.to_string(), json!(f.value)))
            .collect();
        let value = json!({
            "profile": profile,
            "battery": battery,
            "gpu_mode": gpu,
            "profile_tuning": tuning,
            "power": power,
        });
        println!("{}", serde_json::to_string_pretty(&value)?);
        return Ok(());
    }

    println!("性能模式");
    println!("  当前    {}", profile.active.zh());
    println!("  AC 默认 {}", profile.ac.zh());
    println!("  电池    {}", profile.battery.zh());
    println!("  调参持久化 {}", if tuning { "开启" } else { "关闭" });
    println!();
    println!("功耗");
    for field in &fields {
        println!("  {:<16} {:>3} {}", field.label, field.value, field.unit);
    }
    println!();
    println!("GPU");
    println!(
        "  {}  {}",
        gpu.map(|m| m.zh()).unwrap_or("未知"),
        gpu.map(|m| m.detail()).unwrap_or("")
    );
    println!();
    println!("电池");
    println!(
        "  电量 {}%  状态 {}  上限 {}",
        battery
            .capacity
            .map(|v| v.to_string())
            .unwrap_or_else(|| "?".into()),
        battery.status_zh(),
        battery
            .limit
            .map(|v| format!("{v}%"))
            .unwrap_or_else(|| "未知".into())
    );
    if let Some(power) = battery.power_w {
        println!("  功率 {power:.1} W");
    }
    if let Some(health) = battery.health_pct() {
        println!("  健康度 {health:.1}%");
    }
    Ok(())
}

fn profile(cmd: ProfileCmd, json: bool) -> Result<()> {
    match cmd {
        ProfileCmd::Get => {
            let state = asus::profile_get()?;
            if json {
                println!("{}", serde_json::to_string_pretty(&state)?);
            } else {
                println!("当前    {}", state.active.zh());
                println!("AC 默认 {}", state.ac.zh());
                println!("电池    {}", state.battery.zh());
            }
            Ok(())
        }
        ProfileCmd::Set { profile } => {
            let profile: Profile = profile.into();
            asus::profile_set(profile)?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "ok": true,
                        "profile": profile,
                    }))?
                );
            } else {
                println!("已切换到{}模式", profile.zh());
            }
            Ok(())
        }
        ProfileCmd::Default { source, profile } => {
            let source: PowerSource = source.into();
            let profile: Profile = profile.into();
            asus::profile_set_default(source, profile)?;
            if json {
                let source_name = match source {
                    PowerSource::Ac => "ac",
                    PowerSource::Battery => "battery",
                };
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "ok": true,
                        "source": source_name,
                        "profile": profile,
                    }))?
                );
            } else {
                let source_name = match source {
                    PowerSource::Ac => "AC",
                    PowerSource::Battery => "电池",
                };
                println!("已将{source_name}默认模式设为{}", profile.zh());
            }
            Ok(())
        }
    }
}

fn battery(cmd: BatteryCmd, json: bool) -> Result<()> {
    match cmd {
        BatteryCmd::Info => {
            let info = asus::battery_info()?;
            if json {
                println!("{}", serde_json::to_string_pretty(&info)?);
            } else {
                println!(
                    "电量 {}%  状态 {}",
                    info.capacity
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "?".into()),
                    info.status_zh()
                );
                println!(
                    "上限 {}",
                    info.limit
                        .map(|v| format!("{v}%"))
                        .unwrap_or_else(|| "未知".into())
                );
                if let Some(power) = info.power_w {
                    println!("功率 {power:.1} W");
                }
                if let Some(v) = info.voltage_v {
                    println!("电压 {v:.2} V");
                }
                if let (Some(full), Some(design)) =
                    (info.energy_full_wh, info.energy_full_design_wh)
                {
                    println!("容量 {full:.1} / {design:.1} Wh");
                }
                if let Some(health) = info.health_pct() {
                    println!("健康度 {health:.1}%");
                }
                if let Some(cycles) = info.cycle_count {
                    println!("循环次数 {cycles}");
                }
            }
            Ok(())
        }
        BatteryCmd::Limit { percent } => {
            asus::battery_limit(percent)?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "ok": true,
                        "limit": percent,
                    }))?
                );
            } else {
                println!("电池充电上限已设为 {percent}%");
            }
            Ok(())
        }
        BatteryCmd::Oneshot { percent } => {
            asus::battery_oneshot(percent)?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "ok": true,
                        "oneshot": percent,
                    }))?
                );
            } else {
                match percent {
                    Some(p) => println!("已触发单次充电至 {p}%"),
                    None => println!("已触发单次充满"),
                }
            }
            Ok(())
        }
    }
}

fn power(cmd: PowerCmd, json: bool) -> Result<()> {
    let attrs = asus::armoury()?;
    let installed: Vec<&str> = attrs.iter().map(|a| a.name.as_str()).collect();

    let apply = |key: &str, value: i32| -> Result<()> {
        if installed.contains(&key) {
            asus::armoury_set(key, value)?;
        }
        Ok(())
    };

    match cmd {
        PowerCmd::Preset { profile } => {
            let profile: Profile = profile.into();
            let cfg = Config::load()?;
            let preset = cfg
                .power_presets
                .iter()
                .find(|p| p.profile == profile)
                .ok_or_else(|| anyhow::anyhow!("配置里没有 {} 的功耗预设", profile.zh()))?
                .clone();

            asus::profile_set(profile)?;
            apply("ppt_pl1_spl", preset.pl1)?;
            apply("ppt_pl2_sppt", preset.pl2)?;
            apply("ppt_pl3_fppt", preset.pl3)?;
            apply("nv_tgp", preset.gpu_tgp)?;
            apply("nv_dynamic_boost", preset.gpu_boost)?;
            apply("nv_temp_target", preset.gpu_temp)?;

            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "ok": true,
                        "profile": profile,
                        "preset": preset,
                    }))?
                );
            } else {
                println!(
                    "已套用{}预设：PL {} / {} / {} W，dGPU {} W，boost {}，温度墙 {}°C",
                    profile.zh(),
                    preset.pl1,
                    preset.pl2,
                    preset.pl3,
                    preset.gpu_tgp,
                    preset.gpu_boost,
                    preset.gpu_temp
                );
            }
            Ok(())
        }
        PowerCmd::Apply {
            pl1,
            pl2,
            pl3,
            gpu_tgp,
            gpu_boost,
            gpu_temp,
        } => {
            if [pl1, pl2, pl3, gpu_tgp, gpu_boost, gpu_temp]
                .iter()
                .all(Option::is_none)
            {
                bail!("至少指定一个功耗值，例如 --pl1 45 --pl3 65");
            }
            let mut changed = Vec::new();
            for (key, value) in [
                ("ppt_pl1_spl", pl1),
                ("ppt_pl2_sppt", pl2),
                ("ppt_pl3_fppt", pl3),
                ("nv_tgp", gpu_tgp),
                ("nv_dynamic_boost", gpu_boost),
                ("nv_temp_target", gpu_temp),
            ] {
                if let Some(value) = value {
                    apply(key, value)?;
                    changed.push((key, value));
                }
            }

            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "ok": true,
                        "changed": changed,
                    }))?
                );
            } else {
                for (key, value) in &changed {
                    println!("{key} = {value}");
                }
                if pl3.is_some_and(|v| v > 65) {
                    println!("提示：PL3 高于 65W，长时间负载温度和噪音会明显上升");
                }
            }
            Ok(())
        }
    }
}

fn gpu(cmd: GpuCmd, json: bool) -> Result<()> {
    match cmd {
        GpuCmd::Get => {
            let attrs = asus::armoury()?;
            let mode = asus::gpu_mode(&attrs);
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "mode": mode,
                    }))?
                );
            } else {
                println!("当前 GPU 模式：{}", mode.map(|m| m.zh()).unwrap_or("未知"));
            }
            Ok(())
        }
        GpuCmd::Set { mode, yes } => {
            if !yes {
                bail!("GPU 模式修改后需要重启，确认请加 --yes");
            }
            let mode: GpuMode = mode.into();
            asus::gpu_set(mode)?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "ok": true,
                        "mode": mode,
                        "reboot_required": true,
                    }))?
                );
            } else {
                println!("GPU 模式已设为{}，请重启生效", mode.zh());
            }
            Ok(())
        }
    }
}

fn net(cmd: NetCmd, json: bool) -> Result<()> {
    match cmd {
        NetCmd::Status => {
            let devices = nm::devices()?;
            let connections = nm::connections()?;
            let vpns = nm::vpn_list()?;
            let wifi_enabled = nm::wifi_enabled().unwrap_or(false);
            let ssid = nm::active_ssid().unwrap_or(None);
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "wifi_enabled": wifi_enabled,
                        "ssid": ssid,
                        "devices": devices,
                        "connections": connections,
                        "vpns": vpns,
                    }))?
                );
            } else {
                println!(
                    "Wi-Fi {}   SSID {}",
                    if wifi_enabled {
                        "已启用"
                    } else {
                        "已禁用"
                    },
                    ssid.clone().unwrap_or_else(|| "-".into())
                );
                println!();
                println!("设备");
                for device in &devices {
                    if device.kind == "loopback" {
                        continue;
                    }
                    println!(
                        "  {:<12} {:<10} {:<22} {}",
                        device.name, device.kind, device.state, device.connection
                    );
                }
                println!();
                println!("连接");
                for conn in &connections {
                    println!(
                        "  {:<24} {:<16} {}{}",
                        conn.name,
                        conn.kind,
                        if conn.active { "[已连接] " } else { "" },
                        conn.device
                    );
                }
                if !vpns.is_empty() {
                    println!();
                    println!("VPN");
                    for vpn in &vpns {
                        println!(
                            "  {:<24} {}",
                            vpn.name,
                            if vpn.active { "已连接" } else { "未连接" }
                        );
                    }
                }
            }
            Ok(())
        }
        NetCmd::Wifi { cmd } => match cmd {
            WifiCmd::List => {
                let list = nm::wifi_scan(true)?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&list)?);
                } else {
                    for ap in &list {
                        println!(
                            "{:<32} {:>3}%  {:<12} {}",
                            ap.ssid,
                            ap.signal,
                            if ap.security.is_empty() {
                                "开放"
                            } else {
                                &ap.security
                            },
                            if ap.in_use { "[已连接]" } else { "" }
                        );
                    }
                }
                Ok(())
            }
            WifiCmd::Connect {
                ssid,
                password,
                key_mgmt,
                no_up,
            } => {
                nm::wifi_connect(&ssid, password.as_deref(), &key_mgmt, !no_up)?;
                if json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&json!({
                            "ok": true,
                            "ssid": ssid,
                        }))?
                    );
                } else if no_up {
                    println!("Wi-Fi 连接配置已就绪：{ssid}");
                } else {
                    println!("已连接 Wi-Fi：{ssid}");
                }
                Ok(())
            }
        },
        NetCmd::Radio { state } => {
            let on = matches!(state, OnOff::On);
            nm::set_radio(on)?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({ "ok": true, "wifi": on }))?
                );
            } else {
                println!("Wi-Fi 已{}", if on { "启用" } else { "禁用" });
            }
            Ok(())
        }
        NetCmd::Ip { cmd } => match cmd {
            IpCmd::Show { conn } => {
                let settings = nm::connection_ipv4(&conn)?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&settings)?);
                } else {
                    println!("连接 {conn}");
                    println!("  方法  {}", settings.method);
                    println!(
                        "  地址  {}",
                        if settings.addresses.is_empty() {
                            "-"
                        } else {
                            &settings.addresses
                        }
                    );
                    println!(
                        "  网关  {}",
                        if settings.gateway.is_empty() {
                            "-"
                        } else {
                            &settings.gateway
                        }
                    );
                    println!(
                        "  DNS   {}",
                        if settings.dns.is_empty() {
                            "-"
                        } else {
                            &settings.dns
                        }
                    );
                    println!(
                        "  DoT   {}",
                        match settings.dot {
                            -1 => "默认",
                            0 => "关闭",
                            1 => "机会性",
                            2 => "强制",
                            _ => "默认",
                        }
                    );
                }
                Ok(())
            }
            IpCmd::Set {
                conn,
                method,
                address,
                gateway,
                dns,
                dot,
            } => {
                if [
                    method.as_ref(),
                    address.as_ref(),
                    gateway.as_ref(),
                    dns.as_ref(),
                ]
                .iter()
                .all(|v| v.is_none())
                    && dot.is_none()
                {
                    bail!("至少指定一个参数，例如 --method manual --address 192.168.31.50/24");
                }
                let mut settings = nm::connection_ipv4(&conn)?;
                if let Some(method) = method {
                    settings.method = method;
                }
                if let Some(address) = address {
                    settings.addresses = address;
                }
                if let Some(gateway) = gateway {
                    settings.gateway = gateway;
                }
                if let Some(dns) = dns {
                    settings.dns = dns;
                }
                if let Some(dot) = dot {
                    settings.dot = dot;
                }
                nm::apply_ipv4(&conn, &settings)?;
                if json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&json!({
                            "ok": true,
                            "connection": conn,
                            "settings": settings,
                        }))?
                    );
                } else {
                    println!("已更新 {conn} 的 IPv4/DNS 设置");
                }
                Ok(())
            }
        },
        NetCmd::Vpn { cmd } => match cmd {
            VpnCmd::List => {
                let list = nm::vpn_list()?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&list)?);
                } else if list.is_empty() {
                    println!("没有 VPN 连接");
                } else {
                    for vpn in &list {
                        println!(
                            "{:<24} {}",
                            vpn.name,
                            if vpn.active { "已连接" } else { "未连接" }
                        );
                    }
                }
                Ok(())
            }
            VpnCmd::Up { name } => {
                nm::connection_up(&name)?;
                println!("VPN 已连接：{name}");
                Ok(())
            }
            VpnCmd::Down { name } => {
                nm::connection_down(&name)?;
                println!("VPN 已断开：{name}");
                Ok(())
            }
        },
        NetCmd::Conn { cmd } => match cmd {
            ConnCmd::Up { name } => {
                nm::connection_up(&name)?;
                println!("已激活：{name}");
                Ok(())
            }
            ConnCmd::Down { name } => {
                nm::connection_down(&name)?;
                println!("已断开：{name}");
                Ok(())
            }
            ConnCmd::Delete { name } => {
                nm::connection_delete(&name)?;
                println!("已删除：{name}");
                Ok(())
            }
        },
        NetCmd::Hotspot { cmd } => match cmd {
            HotspotCmd::On { password, ssid } => {
                nm::hotspot(&password, &ssid)?;
                println!("热点已开启：{ssid}");
                Ok(())
            }
            HotspotCmd::Off => {
                nm::hotspot_off()?;
                println!("热点已关闭");
                Ok(())
            }
        },
    }
}

fn active_profile() -> Result<Profile> {
    Ok(asus::profile_get()?.active)
}

fn fan(cmd: FanCmd, json: bool) -> Result<()> {
    match cmd {
        FanCmd::List => {
            let profile = active_profile()?;
            let curves = asus::fan_curves(profile)?;
            let cfg = Config::load()?;
            if json {
                let curves_json: Vec<serde_json::Value> = curves
                    .iter()
                    .map(|curve| {
                        json!({
                            "fan": curve.fan,
                            "temp": curve.temp,
                            "pwm": curve.pwm,
                            "enabled": curve.enabled,
                        })
                    })
                    .collect();
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "profile": profile,
                        "curves": curves_json,
                        "presets": cfg.fan_presets,
                    }))?
                );
            } else {
                println!("{} 模式", profile.zh());
                for curve in &curves {
                    let points: Vec<String> = curve
                        .temp
                        .iter()
                        .zip(curve.pwm.iter())
                        .map(|(temp, pwm)| format!("{temp}c:{}%", *pwm as u32 * 100 / 255))
                        .collect();
                    println!(
                        "  {:<4} {:<4} {}",
                        curve.fan,
                        if curve.enabled { "启用" } else { "停用" },
                        points.join(",")
                    );
                }
                println!();
                if cfg.fan_presets.is_empty() {
                    println!("没有保存的风扇预设");
                } else {
                    println!("预设");
                    for preset in &cfg.fan_presets {
                        println!("  {:<16} [{}]", preset.name, preset.profile.zh());
                    }
                }
            }
            Ok(())
        }
        FanCmd::Save { name } => {
            let profile = active_profile()?;
            let curves = asus::fan_curves(profile)?;
            let Some((cpu, gpu)) = asus::fan_data(&curves) else {
                bail!("没有读到完整的 CPU/GPU 曲线");
            };
            let mut cfg = Config::load()?;
            let preset = FanPreset {
                name: name.clone(),
                profile,
                cpu,
                gpu,
            };
            if let Some(existing) = cfg.fan_presets.iter_mut().find(|p| p.name == name) {
                *existing = preset;
            } else {
                cfg.fan_presets.push(preset);
            }
            cfg.save()?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({ "ok": true, "preset": name }))?
                );
            } else {
                println!("已保存风扇预设：{name}");
            }
            Ok(())
        }
        FanCmd::Apply { name } => {
            let cfg = Config::load()?;
            let preset = cfg
                .fan_presets
                .iter()
                .find(|p| p.name == name)
                .ok_or_else(|| anyhow::anyhow!("没有名为 {name} 的风扇预设"))?
                .clone();
            asus::fan_apply(preset.profile, &preset.cpu, &preset.gpu)?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({ "ok": true, "preset": name }))?
                );
            } else {
                println!("已套用风扇预设：{name}");
            }
            Ok(())
        }
        FanCmd::Delete { name } => {
            let mut cfg = Config::load()?;
            let before = cfg.fan_presets.len();
            cfg.fan_presets.retain(|p| p.name != name);
            if cfg.fan_presets.len() == before {
                bail!("没有名为 {name} 的风扇预设");
            }
            cfg.save()?;
            println!("已删除风扇预设：{name}");
            Ok(())
        }
        FanCmd::Default => {
            let profile = active_profile()?;
            asus::fan_default(profile)?;
            println!("{} 的风扇曲线已恢复默认", profile.zh());
            Ok(())
        }
        FanCmd::Enable => {
            let profile = active_profile()?;
            asus::fan_set_enabled(profile, true)?;
            println!("{} 的风扇曲线已启用", profile.zh());
            Ok(())
        }
        FanCmd::Disable => {
            let profile = active_profile()?;
            asus::fan_set_enabled(profile, false)?;
            println!("{} 的风扇曲线已停用", profile.zh());
            Ok(())
        }
    }
}

fn aura(cmd: AuraCmd, json: bool) -> Result<()> {
    match cmd {
        AuraCmd::Next => asus::aura_next()?,
        AuraCmd::Prev => asus::aura_prev()?,
        AuraCmd::Static { colour } => asus::aura_static(&colour)?,
    }
    if json {
        println!("{}", serde_json::to_string_pretty(&json!({ "ok": true }))?);
    } else {
        println!("灯效已更新");
    }
    Ok(())
}

fn config(json: bool) -> Result<()> {
    let path = Config::path();
    let cfg = Config::load()?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "path": path,
                "config": cfg,
            }))?
        );
    } else {
        println!("配置文件 {}", path.display());
        println!();
        for preset in &cfg.power_presets {
            println!(
                "{}：PL {} / {} / {} W，dGPU {} W，boost {}，温度墙 {}°C",
                preset.name,
                preset.pl1,
                preset.pl2,
                preset.pl3,
                preset.gpu_tgp,
                preset.gpu_boost,
                preset.gpu_temp
            );
        }
    }
    Ok(())
}
