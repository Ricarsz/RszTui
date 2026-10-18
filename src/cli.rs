use anyhow::{Result, bail};
use clap::{Parser, Subcommand, ValueEnum};
use serde_json::json;

use crate::asus::{self, GpuMode, PowerSource, Profile};
use crate::config::Config;
use crate::util;

#[derive(Parser)]
#[command(
    name = "rogtui",
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
    /// 显示配置路径与预设
    Config,
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
        Cmd::Config => config(json),
    }
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
            let backup = util::backup_asusd()?;
            asus::profile_set(profile)?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "ok": true,
                        "profile": profile,
                        "backup": backup,
                    }))?
                );
            } else {
                println!("已切换到{}模式", profile.zh());
                println!("配置已备份到 {}", backup.display());
            }
            Ok(())
        }
        ProfileCmd::Default { source, profile } => {
            let source: PowerSource = source.into();
            let profile: Profile = profile.into();
            let backup = util::backup_asusd()?;
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
                        "backup": backup,
                    }))?
                );
            } else {
                let source_name = match source {
                    PowerSource::Ac => "AC",
                    PowerSource::Battery => "电池",
                };
                println!("已将{source_name}默认模式设为{}", profile.zh());
                println!("配置已备份到 {}", backup.display());
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
            let backup = util::backup_asusd()?;
            asus::battery_limit(percent)?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "ok": true,
                        "limit": percent,
                        "backup": backup,
                    }))?
                );
            } else {
                println!("电池充电上限已设为 {percent}%");
                println!("配置已备份到 {}", backup.display());
            }
            Ok(())
        }
        BatteryCmd::Oneshot { percent } => {
            let backup = util::backup_asusd()?;
            asus::battery_oneshot(percent)?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "ok": true,
                        "oneshot": percent,
                        "backup": backup,
                    }))?
                );
            } else {
                match percent {
                    Some(p) => println!("已触发单次充电至 {p}%"),
                    None => println!("已触发单次充满"),
                }
                println!("配置已备份到 {}", backup.display());
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

            let backup = util::backup_asusd()?;
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
                        "backup": backup,
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
                println!("配置已备份到 {}", backup.display());
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
            let backup = util::backup_asusd()?;
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
                        "backup": backup,
                    }))?
                );
            } else {
                for (key, value) in &changed {
                    println!("{key} = {value}");
                }
                println!("配置已备份到 {}", backup.display());
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
            let backup = util::backup_asusd()?;
            asus::gpu_set(mode)?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "ok": true,
                        "mode": mode,
                        "reboot_required": true,
                        "backup": backup,
                    }))?
                );
            } else {
                println!("GPU 模式已设为{}，请重启生效", mode.zh());
                println!("配置已备份到 {}", backup.display());
            }
            Ok(())
        }
    }
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
