use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

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
    pub bssid: String,
    pub mode: String,
    pub chan: String,
    pub freq: String,
    pub rate: String,
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

#[derive(Debug, Clone, Default, Serialize)]
pub struct DeviceDetail {
    pub device: String,
    pub kind: String,
    pub state: String,
    pub connection: String,
    pub hwaddr: String,
    pub mtu: String,
    pub speed: String,
    pub addresses: String,
    pub gateway: String,
    pub dns: String,
    pub routes: String,
    pub ip6_addresses: String,
    pub ip6_gateway: String,
    pub ip6_dns: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct CacheStats {
    pub current_size: u64,
    pub hits: u64,
    pub misses: u64,
    pub transactions: u64,
    pub dns_servers: Vec<String>,
    pub neighbors: usize,
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
    Ok(vpn_list_from(&connections()?))
}

pub fn vpn_list_from(list: &[ConnectionInfo]) -> Vec<ConnectionInfo> {
    list.iter()
        .filter(|c| c.kind.contains("vpn") || c.kind.contains("wireguard"))
        .cloned()
        .collect()
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
            "IN-USE,SSID,BSSID,MODE,CHAN,FREQ,RATE,SIGNAL,SECURITY",
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
        if parts.len() < 9 {
            continue;
        }
        let ssid = parts[1].clone();
        if ssid.is_empty() {
            continue;
        }
        let ap = WifiAp {
            ssid,
            bssid: parts[2].clone(),
            mode: parts[3].clone(),
            chan: parts[4].clone(),
            freq: parts[5].clone(),
            rate: parts[6].clone(),
            signal: parts[7].parse::<u8>().unwrap_or(0),
            security: parts[8].trim().to_string(),
            in_use: parts[0].contains('*') || parts[0].eq_ignore_ascii_case("yes"),
        };

        if let Some(existing) = result.iter_mut().find(|item| item.ssid == ap.ssid) {
            existing.in_use |= ap.in_use;
            if ap.signal > existing.signal {
                *existing = ap;
            }
        } else {
            result.push(ap);
        }
    }

    result.sort_by(|a, b| b.signal.cmp(&a.signal).then_with(|| a.ssid.cmp(&b.ssid)));
    Ok(result)
}

pub fn device_detail(device: &str) -> Result<DeviceDetail> {
    if device.is_empty() {
        return Ok(DeviceDetail::default());
    }
    let out = util::run(
        "nmcli",
        &[
            "-t",
            "-f",
            "GENERAL.DEVICE,GENERAL.TYPE,GENERAL.STATE,GENERAL.CONNECTION,GENERAL.HWADDR,GENERAL.MTU,IP4.ADDRESS,IP4.GATEWAY,IP4.DNS,IP4.ROUTE,IP6.ADDRESS,IP6.GATEWAY,IP6.DNS",
            "device",
            "show",
            device,
        ],
    )?;

    let mut detail = DeviceDetail {
        device: device.to_string(),
        ..DeviceDetail::default()
    };
    let mut addresses = Vec::new();
    let mut dns = Vec::new();
    let mut routes = Vec::new();
    let mut ip6_addresses = Vec::new();
    let mut ip6_dns = Vec::new();

    for line in out.lines() {
        let parts = util::split_nmcli(line);
        if parts.len() < 2 {
            continue;
        }
        let key = parts[0].as_str();
        let value = parts[1..].join(":").trim().to_string();
        if key.starts_with("GENERAL.TYPE") {
            detail.kind = value;
        } else if key.starts_with("GENERAL.STATE") {
            detail.state = value;
        } else if key.starts_with("GENERAL.CONNECTION") {
            detail.connection = value;
        } else if key.starts_with("GENERAL.HWADDR") {
            detail.hwaddr = value;
        } else if key.starts_with("GENERAL.MTU") {
            detail.mtu = value;
        } else if key.starts_with("IP4.ADDRESS") {
            if !value.is_empty() {
                addresses.push(value);
            }
        } else if key.starts_with("IP4.GATEWAY") {
            detail.gateway = value;
        } else if key.starts_with("IP4.DNS") && !value.is_empty() {
            dns.push(value);
        } else if key.starts_with("IP4.ROUTE") && !value.is_empty() {
            routes.push(value);
        } else if key.starts_with("IP6.ADDRESS") {
            if !value.is_empty() {
                ip6_addresses.push(value);
            }
        } else if key.starts_with("IP6.GATEWAY") {
            detail.ip6_gateway = value;
        } else if key.starts_with("IP6.DNS") && !value.is_empty() {
            ip6_dns.push(value);
        }
    }

    detail.addresses = addresses.join(", ");
    detail.dns = dns.join(", ");
    detail.routes = routes.join(" | ");
    detail.ip6_addresses = ip6_addresses.join(", ");
    detail.ip6_dns = ip6_dns.join(", ");
    detail.speed = fs::read_to_string(format!("/sys/class/net/{device}/speed"))
        .map(|v| v.trim().to_string())
        .ok()
        .filter(|v| !v.is_empty() && v != "-1")
        .unwrap_or_default();
    Ok(detail)
}

pub fn cache_stats() -> Result<CacheStats> {
    let text = util::run("resolvectl", &["statistics"])?;
    let mut stats = parse_cache_stats(&text);
    stats.dns_servers = dns_servers();
    stats.neighbors = neighbor_count();
    Ok(stats)
}

fn parse_cache_stats(text: &str) -> CacheStats {
    let value = |key: &str| {
        text.lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                if name.trim().eq_ignore_ascii_case(key) {
                    value.trim().parse::<u64>().ok()
                } else {
                    None
                }
            })
            .unwrap_or(0)
    };
    CacheStats {
        current_size: value("Current Cache Size"),
        hits: value("Cache Hits"),
        misses: value("Cache Misses"),
        transactions: value("Total Transactions"),
        ..CacheStats::default()
    }
}

fn dns_servers() -> Vec<String> {
    let Ok(out) = util::run("resolvectl", &["dns"]) else {
        return Vec::new();
    };
    let mut result: Vec<String> = Vec::new();
    for line in out.lines() {
        let Some((_, rest)) = line.split_once(':') else {
            continue;
        };
        for token in rest.split_whitespace() {
            if (token.contains('.') || token.contains(':'))
                && !token.contains('(')
                && !result.iter().any(|v| v == token)
            {
                result.push(token.to_string());
            }
        }
    }
    result
}

fn neighbor_count() -> usize {
    util::run("ip", &["neigh", "show"])
        .map(|out| out.lines().filter(|line| line.contains(" dev ")).count())
        .unwrap_or(0)
}

pub fn flush_dns_cache() -> Result<()> {
    util::run("resolvectl", &["flush-caches"])?;
    Ok(())
}

pub fn flush_neighbor_cache() -> Result<()> {
    util::run("ip", &["neigh", "flush", "all"])?;
    Ok(())
}

pub fn set_dns_cache_size(size: u32) -> Result<()> {
    if size > 65536 {
        bail!("缓存条目上限不能超过 65536");
    }
    let dir = Path::new("/etc/systemd/resolved.conf.d");
    fs::create_dir_all(dir).with_context(|| format!("无法创建 {}", dir.display()))?;
    let path = dir.join("rsztui-cache.conf");
    fs::write(&path, format!("[Resolve]\nCacheSize={size}\n"))
        .with_context(|| format!("无法写入 {}", path.display()))?;
    util::run("systemctl", &["restart", "systemd-resolved"])?;
    Ok(())
}

pub fn ping(host: &str) -> Result<String> {
    let host = validate_host(host)?;
    let mut command = Command::new("ping");
    if host.contains(':') {
        command.arg("-6");
    }
    let output = command
        .args(["-c", "4", "-W", "1", &host])
        .env("LC_ALL", "C")
        .env("LANG", "C")
        .output()
        .with_context(|| "无法执行 ping，请确认 iputils 已安装")?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if text.trim().is_empty() {
        bail!("ping 没有输出");
    }
    Ok(text.trim().to_string())
}

pub fn dns_query(domain: &str) -> Result<String> {
    let domain = validate_host(domain)?;
    let output = Command::new("resolvectl")
        .args(["query", &domain])
        .env("LC_ALL", "C")
        .env("LANG", "C")
        .output()
        .with_context(|| "无法执行 resolvectl")?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if text.trim().is_empty() {
        bail!("resolvectl 没有输出");
    }
    Ok(text.trim().to_string())
}

fn validate_host(value: &str) -> Result<String> {
    let value = value.trim();
    if value.is_empty() || value.len() > 253 {
        bail!("主机名/域名不合法");
    }
    if !value
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_' | b':'))
    {
        bail!("主机名/域名不合法");
    }
    Ok(value.to_string())
}

#[derive(Debug, Clone, Default)]
pub struct NetworkSnapshot {
    pub devices: Vec<Device>,
    pub connections: Vec<ConnectionInfo>,
    pub vpns: Vec<ConnectionInfo>,
    pub ssid: Option<String>,
    pub wifi_enabled: Option<bool>,
    pub wifi: Vec<WifiAp>,
    pub wifi_scanned: bool,
}

pub fn network_snapshot(include_wifi: bool, force_scan: bool) -> Result<NetworkSnapshot> {
    let devices = devices()?;
    let connections: Vec<ConnectionInfo> = connections()?
        .into_iter()
        .filter(|c| c.kind != "loopback")
        .collect();
    let vpns = vpn_list_from(&connections);
    let wifi_enabled = wifi_enabled().ok();

    let (ssid, wifi, wifi_scanned) = if include_wifi {
        let wifi = wifi_scan(force_scan).unwrap_or_default();
        (active_ssid().ok().flatten(), wifi, true)
    } else {
        (None, Vec::new(), false)
    };

    Ok(NetworkSnapshot {
        devices,
        connections,
        vpns,
        ssid,
        wifi_enabled,
        wifi,
        wifi_scanned,
    })
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

pub fn connection_ipv4(name: &str) -> Result<Ipv4Settings> {
    let out = util::run(
        "nmcli",
        &[
            "-t",
            "-f",
            "ipv4.method,ipv4.addresses,ipv4.gateway,ipv4.dns,connection.dns-over-tls",
            "connection",
            "show",
            name,
        ],
    )?;

    let mut settings = Ipv4Settings {
        method: "auto".into(),
        ..Ipv4Settings::default()
    };
    let mut addresses = Vec::new();
    let mut dns = Vec::new();

    for line in out.lines() {
        let parts = util::split_nmcli(line);
        if parts.len() < 2 {
            continue;
        }
        let key = parts[0].as_str();
        let value = parts[1..].join(":").trim().to_string();
        if key.starts_with("ipv4.method") {
            settings.method = value;
        } else if key.starts_with("ipv4.addresses") && !value.is_empty() {
            addresses.push(value);
        } else if key.starts_with("ipv4.gateway") {
            settings.gateway = value;
        } else if key.starts_with("ipv4.dns") && !value.is_empty() {
            dns.push(value);
        } else if key.starts_with("connection.dns-over-tls") {
            settings.dot = match value.as_str() {
                "no" | "0" => 0,
                "opportunistic" | "1" => 1,
                "yes" | "2" => 2,
                _ => -1,
            };
        }
    }

    settings.addresses = addresses.join(", ");
    settings.dns = dns.join(", ");
    Ok(settings)
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

/// 修改已保存 Wi-Fi 连接的密码；连接正在使用时立即重连生效。
pub fn set_wifi_password(conn: &str, password: &str) -> Result<()> {
    let len = password.chars().count();
    if !(8..=63).contains(&len) {
        bail!("WPA 密码长度需为 8–63 个字符");
    }
    let active = connections()?
        .into_iter()
        .find(|c| c.name == conn)
        .is_some_and(|c| c.active);
    util::run(
        "nmcli",
        &[
            "connection",
            "modify",
            conn,
            "wifi-sec.key-mgmt",
            "wpa-psk",
            "wifi-sec.psk",
            password,
        ],
    )?;
    if active {
        connection_up(conn)?;
    }
    Ok(())
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
    fn parses_cache_stats() {
        let text = "Transactions\n  Current Transactions: 0\n    Total Transactions: 20823\nCache\n  Current Cache Size: 15\n    Cache Hits: 5036\n  Cache Misses: 15759\n";
        let stats = parse_cache_stats(text);
        assert_eq!(stats.current_size, 15);
        assert_eq!(stats.hits, 5036);
        assert_eq!(stats.misses, 15759);
        assert_eq!(stats.transactions, 20823);
    }

    #[test]
    fn validates_hosts() {
        assert!(validate_host("1.1.1.1").is_ok());
        assert!(validate_host("example.com").is_ok());
        assert!(validate_host("bad host").is_err());
        assert!(validate_host("a/b").is_err());
    }

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
