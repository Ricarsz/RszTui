use std::process::Command;

use anyhow::{Result, bail};
use serde::Serialize;

use crate::util;

pub const DEFAULT_UNITS: [&str; 4] = [
    "asusd.service",
    "asus-shutdown.service",
    "NetworkManager.service",
    "systemd-resolved.service",
];

#[derive(Debug, Clone, Serialize)]
pub struct ServiceInfo {
    pub unit: String,
    pub load: String,
    pub active: String,
    pub enabled: String,
}

impl ServiceInfo {
    pub fn is_active(&self) -> bool {
        self.active == "active"
    }

    pub fn display_name(&self) -> &str {
        self.unit.strip_suffix(".service").unwrap_or(&self.unit)
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceAction {
    Start,
    Stop,
    Restart,
    Enable,
    Disable,
    EnableNow,
    DisableNow,
}

impl ServiceAction {
    pub fn label(self) -> &'static str {
        match self {
            ServiceAction::Start => "启动",
            ServiceAction::Stop => "停止",
            ServiceAction::Restart => "重启",
            ServiceAction::Enable => "启用开机自启",
            ServiceAction::Disable => "禁用开机自启",
            ServiceAction::EnableNow => "启用并立即启动",
            ServiceAction::DisableNow => "禁用并停止",
        }
    }

    fn args(self) -> &'static [&'static str] {
        match self {
            ServiceAction::Start => &["start"],
            ServiceAction::Stop => &["stop"],
            ServiceAction::Restart => &["restart"],
            ServiceAction::Enable => &["enable"],
            ServiceAction::Disable => &["disable"],
            ServiceAction::EnableNow => &["enable", "--now"],
            ServiceAction::DisableNow => &["disable", "--now"],
        }
    }
}

pub fn normalize_unit(name: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty() {
        bail!("服务名不能为空");
    }
    let unit = if name.ends_with(".service") {
        name.to_string()
    } else {
        format!("{name}.service")
    };
    if unit.len() > 256
        || !unit
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'@' | b'.' | b'_' | b'-' | b':'))
    {
        bail!("服务名不合法：{name}");
    }
    Ok(unit)
}

pub fn query(name: &str) -> ServiceInfo {
    let unit = normalize_unit(name).unwrap_or_else(|_| name.to_string());
    let output = Command::new("systemctl")
        .args([
            "show",
            "-p",
            "LoadState",
            "-p",
            "ActiveState",
            "-p",
            "UnitFileState",
            "--value",
            &unit,
        ])
        .env("LC_ALL", "C")
        .env("LANG", "C")
        .output();

    let (load, active, enabled) = match output {
        Ok(output) if output.status.success() => {
            let text = String::from_utf8_lossy(&output.stdout);
            let mut lines = text.lines();
            (
                lines.next().unwrap_or("unknown").trim().to_string(),
                lines.next().unwrap_or("unknown").trim().to_string(),
                lines
                    .next()
                    .map(|v| v.trim().to_string())
                    .filter(|v| !v.is_empty())
                    .unwrap_or_else(|| "unknown".into()),
            )
        }
        _ => ("unknown".into(), "unknown".into(), "unknown".into()),
    };

    ServiceInfo {
        unit,
        load,
        active,
        enabled,
    }
}

pub fn status_all() -> Vec<ServiceInfo> {
    DEFAULT_UNITS.iter().map(|unit| query(unit)).collect()
}

pub fn apply(name: &str, action: ServiceAction) -> Result<()> {
    let unit = normalize_unit(name)?;
    let mut args: Vec<&str> = action.args().to_vec();
    args.push(unit.as_str());
    util::run("systemctl", &args)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_unit_names() {
        assert_eq!(normalize_unit("asusd").unwrap(), "asusd.service");
        assert_eq!(
            normalize_unit("NetworkManager.service").unwrap(),
            "NetworkManager.service"
        );
        assert_eq!(normalize_unit("getty@tty1").unwrap(), "getty@tty1.service");
    }

    #[test]
    fn rejects_bad_unit_names() {
        assert!(normalize_unit("").is_err());
        assert!(normalize_unit("foo/bar").is_err());
        assert!(normalize_unit("foo bar").is_err());
    }
}
