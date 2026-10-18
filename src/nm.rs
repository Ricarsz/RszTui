use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Serialize;

use crate::util;

#[derive(Debug, Clone, Default, Serialize)]
pub struct Device {
    pub name: String,
    pub kind: String,
    pub state: String,
    pub connection: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ConnectionInfo {
    pub name: String,
    pub uuid: String,
    pub kind: String,
    pub device: String,
    pub active: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct WifiAp {
    pub ssid: String,
    pub signal: u8,
    pub security: String,
    pub in_use: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Ipv4Settings {
    pub method: String,
    pub addresses: String,
    pub gateway: String,
    pub dns: String,
    pub dot: i32,
}

pub fn devices() -> Result<Vec<Device>> {
    let out = util::run(
        "nmcli",
        &[
            "-t",
            "-f",
            "DEVICE,TYPE,STATE,CONNECTION",
            "device",
            "status",
        ],
    )?;
    let mut result = Vec::new();
    for line in out.lines() {
        let parts = util::split_nmcli(line);
        if parts.len() < 4 {
            continue;
        }
        result.push(Device {
            name: parts[0].clone(),
            kind: parts[1].clone(),
            state: parts[2].clone(),
            connection: parts[3].clone(),
        });
    }
    Ok(result)
}

pub fn wifi_enabled() -> Result<bool> {
    let out = util::run("nmcli", &["radio", "wifi"])?;
    Ok(out.trim().eq_ignore_ascii_case("enabled"))
}

pub fn set_radio(on: bool) -> Result<()> {
    util::run("nmcli", &["radio", "wifi", if on { "on" } else { "off" }])?;
    Ok(())
}

pub fn active_ssid() -> Result<Option<String>> {
    let out = util::run("nmcli", &["-t", "-f", "ACTIVE,SSID", "device", "wifi"])?;
    for line in out.lines() {
        let parts = util::split_nmcli(line);
        if parts.first().map(String::as_str) == Some("yes")
            && let Some(ssid) = parts.get(1)
            && !ssid.is_empty()
        {
            return Ok(Some(ssid.clone()));
        }
    }
    Ok(None)
}

pub fn connections() -> Result<Vec<ConnectionInfo>> {
    let out = util::run(
        "nmcli",
        &[
            "-t",
            "-f",
            "NAME,UUID,TYPE,DEVICE,ACTIVE",
            "connection",
            "show",
        ],
    )?;
    let mut result = Vec::new();
    for line in out.lines() {
        let parts = util::split_nmcli(line);
        if parts.len() < 5 {
            continue;
        }
        result.push(ConnectionInfo {
            name: parts[0].clone(),
            uuid: parts[1].clone(),
            kind: parts[2].clone(),
            device: parts[3].clone(),
            active: parts[4].eq_ignore_ascii_case("yes"),
        });
    }
    Ok(result)
}

pub fn vpn_list() -> Result<Vec<ConnectionInfo>> {
    Ok(connections()?
        .into_iter()
        .filter(|c| c.kind.contains("vpn") || c.kind.contains("wireguard"))
        .collect())
}

pub fn connection_up(name: &str) -> Result<()> {
    util::run("nmcli", &["connection", "up", "id", name])?;
    Ok(())
}

pub fn connection_down(name: &str) -> Result<()> {
    util::run("nmcli", &["connection", "down", "id", name])?;
    Ok(())
}

pub fn connection_delete(name: &str) -> Result<()> {
    util::run("nmcli", &["connection", "delete", "id", name])?;
    Ok(())
}

pub fn wifi_scan(rescan: bool) -> Result<Vec<WifiAp>> {
    let out = util::run(
        "nmcli",
        &[
            "-t",
            "-f",
            "IN-USE,SSID,SIGNAL,SECURITY",
            "device",
            "wifi",
            "list",
            "--rescan",
            if rescan { "yes" } else { "no" },
        ],
    )?;

    let mut result: Vec<WifiAp> = Vec::new();
    for line in out.lines() {
        let parts = util::split_nmcli(line);
        if parts.len() < 4 {
            continue;
        }
        let ssid = parts[1].clone();
        if ssid.is_empty() {
            continue;
        }
        let signal = parts[2].parse::<u8>().unwrap_or(0);
        let security = parts[3].trim().to_string();
        let in_use = parts[0].contains('*');

        if let Some(existing) = result.iter_mut().find(|ap| ap.ssid == ssid) {
            if signal > existing.signal {
                existing.signal = signal;
                existing.security = security;
            }
            existing.in_use |= in_use;
        } else {
            result.push(WifiAp {
                ssid,
                signal,
                security,
                in_use,
            });
        }
    }

    result.sort_by(|a, b| b.signal.cmp(&a.signal).then_with(|| a.ssid.cmp(&b.ssid)));
    Ok(result)
}

pub fn find_connection(name: &str) -> Result<Option<ConnectionInfo>> {
    Ok(connections()?.into_iter().find(|c| c.name == name))
}

pub fn wifi_profile(ssid: &str) -> Result<Option<ConnectionInfo>> {
    find_wifi_connection(ssid)
}

fn find_wifi_connection(ssid: &str) -> Result<Option<ConnectionInfo>> {
    for connection in connections()?
        .into_iter()
        .filter(|c| c.kind.contains("wireless"))
    {
        let value = get_field(&connection.name, "802-11-wireless.ssid").unwrap_or_default();
        if value == ssid {
            return Ok(Some(connection));
        }
    }
    Ok(None)
}

fn get_field(name: &str, field: &str) -> Result<String> {
    let out = util::run("nmcli", &["-g", field, "connection", "show", name])?;
    Ok(out.trim().to_string())
}

fn get_multi(name: &str, field: &str) -> Result<String> {
    let out = util::run("nmcli", &["-g", field, "connection", "show", name])?;
    Ok(out
        .lines()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .collect::<Vec<_>>()
        .join(", "))
}

pub fn connection_ipv4(name: &str) -> Result<Ipv4Settings> {
    let dot_raw = get_field(name, "connection.dns-over-tls").unwrap_or_default();
    let dot = match dot_raw.as_str() {
        "no" | "0" => 0,
        "opportunistic" | "1" => 1,
        "yes" | "2" => 2,
        _ => -1,
    };

    Ok(Ipv4Settings {
        method: get_field(name, "ipv4.method").unwrap_or_else(|_| "auto".into()),
        addresses: get_multi(name, "ipv4.addresses").unwrap_or_default(),
        gateway: get_field(name, "ipv4.gateway").unwrap_or_default(),
        dns: get_multi(name, "ipv4.dns").unwrap_or_default(),
        dot,
    })
}

pub fn apply_ipv4(name: &str, settings: &Ipv4Settings) -> Result<()> {
    let method = settings.method.trim();
    if method != "auto" && method != "manual" {
        bail!("IPv4 方法只能是 auto 或 manual");
    }

    if method == "manual" {
        if settings.addresses.trim().is_empty() {
            bail!("手动 IP 必须填写地址，例如 192.0.2.10/24");
        }
        if !validate_cidr_list(&settings.addresses) {
            bail!("地址格式不对，应为 192.0.2.10/24，多个用逗号分隔");
        }
        if !settings.gateway.trim().is_empty() && !validate_ipv4(settings.gateway.trim()) {
            bail!("网关格式不对，例如 192.0.2.1");
        }
        if !settings.dns.trim().is_empty() && !validate_dns_list(&settings.dns) {
            bail!("DNS 格式不对，多个用逗号或空格分隔");
        }
    }

    if !(-1..=2).contains(&settings.dot) {
        bail!("DoT 只能是 -1（默认）、0（关闭）、1（机会性）、2（强制）");
    }

    let dot = match settings.dot {
        0 => "no",
        1 => "opportunistic",
        2 => "yes",
        _ => "default",
    };

    let (addresses, gateway) = if method == "manual" {
        (settings.addresses.trim(), settings.gateway.trim())
    } else {
        ("", "")
    };
    let dns = settings.dns.trim();

    let args: Vec<String> = vec![
        "connection".into(),
        "modify".into(),
        "id".into(),
        name.into(),
        "ipv4.method".into(),
        method.into(),
        "ipv4.addresses".into(),
        addresses.into(),
        "ipv4.gateway".into(),
        gateway.into(),
        "ipv4.dns".into(),
        dns.into(),
        "connection.dns-over-tls".into(),
        dot.into(),
        "ipv4.ignore-auto-dns".into(),
        if dns.is_empty() { "no" } else { "yes" }.into(),
    ];
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    util::run("nmcli", &refs)?;

    let active = find_connection(name)?.map(|c| c.active).unwrap_or(false);
    if active {
        util::run("nmcli", &["connection", "up", "id", name])?;
    }
    Ok(())
}

fn validate_ipv4(value: &str) -> bool {
    let parts: Vec<&str> = value.split('.').collect();
    parts.len() == 4
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.parse::<u8>().is_ok())
}

fn validate_cidr_list(value: &str) -> bool {
    value.split(',').all(|item| {
        let item = item.trim();
        let (ip, prefix) = match item.split_once('/') {
            Some((ip, prefix)) => (ip, Some(prefix)),
            None => (item, None),
        };
        if !validate_ipv4(ip) {
            return false;
        }
        match prefix {
            Some(prefix) => prefix.parse::<u8>().map(|v| v <= 32).unwrap_or(false),
            None => false,
        }
    })
}

fn validate_dns_list(value: &str) -> bool {
    value
        .split([',', ' '])
        .filter(|v| !v.trim().is_empty())
        .all(|v| validate_ipv4(v.trim()))
}

pub fn wifi_connect(
    ssid: &str,
    password: Option<&str>,
    key_mgmt: &str,
    activate: bool,
) -> Result<()> {
    if let Some(existing) = find_wifi_connection(ssid)? {
        if activate {
            connection_up(&existing.name)?;
        }
        return Ok(());
    }

    let uuid = new_uuid()?;
    let mut content = String::new();
    content.push_str("[connection]\n");
    content.push_str(&format!("id={}\n", keyfile_value(ssid)?));
    content.push_str(&format!("uuid={uuid}\n"));
    content.push_str("type=wifi\n");
    content.push_str("autoconnect=true\n");
    content.push_str("\n[wifi]\n");
    content.push_str("mode=infrastructure\n");
    content.push_str(&format!("ssid={}\n", keyfile_value(ssid)?));

    if let Some(password) = password {
        let key_mgmt = if key_mgmt.is_empty() {
            "wpa-psk"
        } else {
            key_mgmt
        };
        content.push_str("\n[wifi-security]\n");
        content.push_str(&format!("key-mgmt={key_mgmt}\n"));
        content.push_str(&format!("psk={}\n", keyfile_value(password)?));
    }

    content.push_str("\n[ipv4]\nmethod=auto\n");

    let filename = format!("rsztui-{uuid}.nmconnection");
    install_keyfile(&content, &filename)?;
    if activate {
        util::run("nmcli", &["connection", "up", "id", ssid])?;
    }
    Ok(())
}

pub fn hotspot(password: &str, ssid: &str) -> Result<()> {
    let existing = find_connection("rsztui-hotspot")?;
    let uuid = existing
        .as_ref()
        .map(|c| c.uuid.clone())
        .unwrap_or(new_uuid()?);
    let filename = existing
        .as_ref()
        .and_then(|c| get_field(&c.name, "connection.filename").ok())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "rsztui-hotspot.nmconnection".to_string());

    let mut content = String::new();
    content.push_str("[connection]\n");
    content.push_str("id=rsztui-hotspot\n");
    content.push_str(&format!("uuid={uuid}\n"));
    content.push_str("type=wifi\n");
    content.push_str("autoconnect=false\n");
    content.push_str("\n[wifi]\n");
    content.push_str("mode=ap\n");
    content.push_str(&format!("ssid={}\n", keyfile_value(ssid)?));
    content.push_str("band=bg\n");
    content.push_str("\n[wifi-security]\n");
    content.push_str("key-mgmt=wpa-psk\n");
    content.push_str(&format!("psk={}\n", keyfile_value(password)?));
    content.push_str("\n[ipv4]\n");
    content.push_str("method=shared\n");

    install_keyfile(&content, &filename)?;
    util::run("nmcli", &["connection", "up", "id", "rsztui-hotspot"])?;
    Ok(())
}

pub fn hotspot_off() -> Result<()> {
    let _ = util::run("nmcli", &["connection", "down", "id", "rsztui-hotspot"]);
    Ok(())
}

pub fn hotspot_active() -> bool {
    connections()
        .map(|list| list.iter().any(|c| c.name == "rsztui-hotspot" && c.active))
        .unwrap_or(false)
}

fn keyfile_value(value: &str) -> Result<&str> {
    if value.contains(['\n', '\r', '\0']) {
        bail!("SSID/密码里不能包含换行或 NUL");
    }
    Ok(value)
}

fn install_keyfile(content: &str, filename: &str) -> Result<PathBuf> {
    let dir = Path::new("/etc/NetworkManager/system-connections");
    fs::create_dir_all(dir).with_context(|| format!("无法创建 {}", dir.display()))?;
    let path = if Path::new(filename).is_absolute() {
        PathBuf::from(filename)
    } else {
        dir.join(filename)
    };

    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .mode(0o600)
        .open(&path)
        .with_context(|| format!("无法写入 {}", path.display()))?;
    file.write_all(content.as_bytes())?;
    drop(file);

    util::run("nmcli", &["connection", "reload"])?;
    Ok(path)
}

fn new_uuid() -> Result<String> {
    let uuid = fs::read_to_string("/proc/sys/kernel/random/uuid")
        .context("无法读取 /proc/sys/kernel/random/uuid")?;
    Ok(uuid.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks_ipv4() {
        assert!(validate_ipv4("192.0.2.1"));
        assert!(!validate_ipv4("192.0.2"));
        assert!(!validate_ipv4("256.1.1.1"));
    }

    #[test]
    fn checks_cidr() {
        assert!(validate_cidr_list("192.0.2.10/24"));
        assert!(validate_cidr_list("10.0.0.2/8, 10.0.0.3/8"));
        assert!(!validate_cidr_list("192.0.2.10"));
        assert!(!validate_cidr_list("192.0.2.10/33"));
    }

    #[test]
    fn validates_keyfile_values() {
        assert!(keyfile_value("a;b\\c").is_ok());
        assert!(keyfile_value("a\nb").is_err());
    }
}
