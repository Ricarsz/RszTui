use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Check {
    pub name: &'static str,
    pub ok: bool,
    /// 缺少时是否算依赖检查失败。非 ASUS 机器上的 asusctl/asusd 不算。
    pub required: bool,
    pub detail: String,
    pub hint: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub distro: String,
    pub vendor: String,
    pub product: String,
    pub laptop: bool,
    pub asus_hardware: bool,
    pub checks: Vec<Check>,
}

impl Report {
    pub fn all_ok(&self) -> bool {
        self.checks.iter().filter(|c| c.required).all(|c| c.ok)
    }

    pub fn has_warnings(&self) -> bool {
        self.checks.iter().any(|c| !c.ok)
    }

    pub fn summary_text(&self) -> String {
        let mut lines = vec![
            format!("发行版：{}", self.distro),
            format!(
                "主机：{} {}（{}）",
                self.vendor,
                self.product,
                if self.laptop {
                    "笔记本"
                } else {
                    "非笔记本"
                }
            ),
            String::new(),
        ];
        for check in &self.checks {
            lines.push(format!(
                "{} {}：{}",
                if check.ok { "✅" } else { "❌" },
                check.name,
                check.detail
            ));
            if !check.ok
                && let Some(hint) = &check.hint
            {
                for line in hint.lines() {
                    lines.push(format!("   {line}"));
                }
            }
        }
        lines.push(String::new());
        if self.asus_hardware {
            lines.push("缺少 asusctl/asusd 时 ASUS 功能不可用；缺少 nmcli/NetworkManager 时网络功能不可用。".into());
        } else {
            lines.push("未检测到 ASUS 笔记本：ASUS 功能不可用，网络功能仍可使用。".into());
        }
        lines.push("按 Enter 继续（可用功能会降级）。".into());
        lines.join("\n")
    }

    pub fn startup_warning(&self) -> Option<String> {
        if !self.has_warnings() {
            return None;
        }
        let mut lines = vec![format!(
            "依赖/硬件检查：{} {}（{}）",
            self.vendor,
            self.product,
            if self.laptop {
                "笔记本"
            } else {
                "非笔记本"
            }
        )];
        for check in self.checks.iter().filter(|c| !c.ok) {
            lines.push(format!("  ❌ {}：{}", check.name, check.detail));
            if let Some(hint) = &check.hint {
                for line in hint.lines() {
                    lines.push(format!("     {line}"));
                }
            }
        }
        lines.push("运行 `rsztui doctor` 查看完整检查结果。".into());
        Some(lines.join("\n"))
    }
}

pub fn check() -> Report {
    let (id, like, pretty) = distro();
    let hardware = hardware();
    let asus_required = hardware.asus && hardware.laptop;

    let mut checks = Vec::new();

    checks.push(Check {
        name: "发行版",
        ok: true,
        required: false,
        detail: if pretty.is_empty() {
            id.clone()
        } else if id.is_empty() {
            pretty.clone()
        } else {
            format!("{pretty}（{id}）")
        },
        hint: None,
    });

    checks.push(Check {
        name: "主机厂商",
        ok: asus_required,
        required: false,
        detail: format!(
            "{} {}（{}，chassis={}）",
            if hardware.vendor.is_empty() {
                "未知厂商"
            } else {
                &hardware.vendor
            },
            if hardware.product.is_empty() {
                "未知型号"
            } else {
                &hardware.product
            },
            if hardware.laptop { "笔记本" } else { "非笔记本" },
            hardware
                .chassis
                .map(|v| v.to_string())
                .unwrap_or_else(|| "未知".into())
        ),
        hint: if asus_required {
            None
        } else if hardware.asus {
            Some("检测到 ASUS 硬件，但没有识别为笔记本；asusctl 的笔记本功能可能不可用。".into())
        } else {
            Some("本工具面向 ASUS TUF/ROG 笔记本；当前设备非 ASUS，ASUS 功能不可用，网络功能仍可使用。".into())
        },
    });

    let asusctl_path = which("asusctl");
    let asusctl_version = asusctl_path.as_ref().and_then(|_| {
        command_stdout("asusctl", &["info"]).and_then(|text| {
            text.lines()
                .next()
                .map(|line| line.trim().to_string())
                .filter(|line| !line.is_empty())
        })
    });
    checks.push(Check {
        name: "asusctl",
        ok: asusctl_path.is_some(),
        required: asus_required,
        detail: match (&asusctl_path, &asusctl_version) {
            (Some(_), Some(version)) => version.lines().next().unwrap_or(version).to_string(),
            (Some(path), None) => format!("已安装（{}）", path.display()),
            (None, _) => "未安装".into(),
        },
        hint: if asusctl_path.is_some() || !asus_required {
            None
        } else {
            Some(install_hint_asusctl(&id, &like))
        },
    });

    let systemd = which("systemctl").is_some();
    let asusd_active = service_active("asusd");
    let asusd_enabled = service_enabled("asusd");
    checks.push(Check {
        name: "asusd",
        ok: asusd_active == Some(true),
        required: asus_required,
        detail: match (systemd, asusd_active, asusd_enabled) {
            (false, _, _) => "无法检测：systemctl 不存在".into(),
            (true, Some(true), Some(true)) => "运行中，已设置开机启动".into(),
            (true, Some(true), _) => "运行中，但未设置开机启动".into(),
            (true, Some(false), Some(true)) => "未运行（已设置开机启动）".into(),
            (true, Some(false), _) => "未运行".into(),
            (true, None, _) => "无法检测".into(),
        },
        hint: if asusd_active == Some(true) || !asus_required {
            None
        } else {
            Some("启动并设为开机自启：sudo systemctl enable --now asusd".into())
        },
    });

    let nmcli = which("nmcli");
    let nmcli_version = nmcli
        .as_ref()
        .and_then(|_| command_stdout("nmcli", &["--version"]));
    checks.push(Check {
        name: "nmcli",
        ok: nmcli.is_some(),
        required: true,
        detail: match (&nmcli, &nmcli_version) {
            (Some(_), Some(version)) => version.lines().next().unwrap_or(version).to_string(),
            (Some(path), None) => format!("已安装（{}）", path.display()),
            (None, _) => "未安装".into(),
        },
        hint: if nmcli.is_some() {
            None
        } else {
            Some(install_hint_nm(&id, &like))
        },
    });

    let nm_active = service_active("NetworkManager");
    checks.push(Check {
        name: "NetworkManager",
        ok: nm_active == Some(true),
        required: true,
        detail: match (systemd, nm_active) {
            (false, _) => "无法检测：systemctl 不存在".into(),
            (true, Some(true)) => "运行中".into(),
            (true, Some(false)) => "未运行".into(),
            (true, None) => "无法检测".into(),
        },
        hint: if nm_active == Some(true) {
            None
        } else {
            Some("启动并设为开机自启：sudo systemctl enable --now NetworkManager".into())
        },
    });

    Report {
        distro: if pretty.is_empty() { id } else { pretty },
        vendor: hardware.vendor,
        product: hardware.product,
        laptop: hardware.laptop,
        asus_hardware: asus_required,
        checks,
    }
}

struct Hardware {
    vendor: String,
    product: String,
    chassis: Option<u32>,
    laptop: bool,
    asus: bool,
}

fn hardware() -> Hardware {
    let vendor = read_dmi("sys_vendor")
        .filter(|v| !v.is_empty())
        .or_else(|| read_dmi("board_vendor"))
        .unwrap_or_default();
    let product = read_dmi("product_name").unwrap_or_default();
    let chassis = read_dmi("chassis_type").and_then(|v| v.parse::<u32>().ok());
    let battery = has_battery();
    let laptop = chassis.is_some_and(is_laptop_chassis) || battery;
    let asus = is_asus_vendor(&vendor);

    Hardware {
        vendor,
        product,
        chassis,
        laptop,
        asus,
    }
}

fn read_dmi(name: &str) -> Option<String> {
    fs::read_to_string(format!("/sys/class/dmi/id/{name}"))
        .ok()
        .map(|v| v.trim().to_string())
}

fn has_battery() -> bool {
    fs::read_dir("/sys/class/power_supply")
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .any(|e| e.file_name().to_string_lossy().starts_with("BAT"))
        })
        .unwrap_or(false)
}

/// SMBIOS chassis type: 8 Portable, 9 Laptop, 10 Notebook, 14 Sub Notebook,
/// 30 Tablet, 31 Convertible, 32 Detachable.
fn is_laptop_chassis(value: u32) -> bool {
    matches!(value, 8..=10 | 14 | 30..=32)
}

fn is_asus_vendor(vendor: &str) -> bool {
    let lower = vendor.to_ascii_lowercase();
    lower.contains("asus") || lower.contains("asustek")
}

fn which(bin: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    for dir in env::split_paths(&path) {
        let candidate = dir.join(bin);
        if let Ok(meta) = fs::metadata(&candidate)
            && meta.is_file()
            && meta.permissions().mode() & 0o111 != 0
        {
            return Some(candidate);
        }
    }
    None
}

fn command_stdout(program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program)
        .args(args)
        .env("LC_ALL", "C")
        .env("LANG", "C")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn service_active(name: &str) -> Option<bool> {
    which("systemctl")?;
    Command::new("systemctl")
        .args(["is-active", "--quiet", name])
        .status()
        .ok()
        .map(|status| status.success())
}

fn service_enabled(name: &str) -> Option<bool> {
    which("systemctl")?;
    Command::new("systemctl")
        .args(["is-enabled", "--quiet", name])
        .status()
        .ok()
        .map(|status| status.success())
}

fn distro() -> (String, String, String) {
    let text = fs::read_to_string("/etc/os-release").unwrap_or_default();
    let mut id = String::new();
    let mut like = String::new();
    let mut pretty = String::new();
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim().trim_matches('"').to_string();
        match key {
            "ID" => id = value,
            "ID_LIKE" => like = value,
            "PRETTY_NAME" => pretty = value,
            _ => {}
        }
    }
    (id, like, pretty)
}

fn is_arch(id: &str, like: &str) -> bool {
    id == "arch"
        || id == "cachyos"
        || id == "endeavouros"
        || id == "manjaro"
        || like.contains("arch")
}

fn is_fedora(id: &str, like: &str) -> bool {
    id == "fedora" || id == "nobara" || id == "ultramarine" || like.contains("fedora")
}

fn is_suse(id: &str, like: &str) -> bool {
    id.contains("opensuse") || id.contains("suse") || like.contains("suse")
}

fn is_debian(id: &str, like: &str) -> bool {
    id == "debian"
        || id == "ubuntu"
        || id == "linuxmint"
        || id == "pop"
        || like.contains("debian")
        || like.contains("ubuntu")
}

fn install_hint_asusctl(id: &str, like: &str) -> String {
    if is_arch(id, like) {
        "安装：paru -S asusctl\n（若已添加 OGC 仓库，也可 sudo pacman -S asusctl）".into()
    } else if is_fedora(id, like) {
        "安装：sudo dnf install asusctl\n（需要先启用 Terra 仓库）".into()
    } else if is_suse(id, like) {
        "安装：sudo zypper install asusctl\n（需要先添加 OBS 仓库）".into()
    } else if is_debian(id, like) {
        "Debian/Ubuntu 无官方包，需从源码构建：\nhttps://github.com/OpenGamingCollective/asusctl"
            .into()
    } else {
        "安装说明：https://asus-linux.org/".into()
    }
}

fn install_hint_nm(id: &str, like: &str) -> String {
    if is_arch(id, like) {
        "安装：sudo pacman -S networkmanager".into()
    } else if is_fedora(id, like) {
        "安装：sudo dnf install NetworkManager".into()
    } else if is_suse(id, like) {
        "安装：sudo zypper install NetworkManager".into()
    } else if is_debian(id, like) {
        "安装：sudo apt install networkmanager".into()
    } else {
        "安装 NetworkManager 后启用服务：sudo systemctl enable --now NetworkManager".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_arch_family() {
        assert!(is_arch("cachyos", "arch"));
        assert!(is_arch("arch", ""));
        assert!(!is_arch("fedora", ""));
    }

    #[test]
    fn detects_asus_vendor() {
        assert!(is_asus_vendor("ASUSTeK COMPUTER INC."));
        assert!(is_asus_vendor("ASUS"));
        assert!(!is_asus_vendor("LENOVO"));
        assert!(!is_asus_vendor(""));
    }

    #[test]
    fn detects_laptop_chassis() {
        assert!(is_laptop_chassis(10));
        assert!(is_laptop_chassis(31));
        assert!(!is_laptop_chassis(3));
    }

    #[test]
    fn distro_hint_is_not_empty() {
        assert!(!install_hint_asusctl("arch", "").is_empty());
        assert!(!install_hint_nm("debian", "").is_empty());
    }
}
