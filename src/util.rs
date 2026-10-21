use std::env;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

/// 是否以 root 运行。开发调试时可用 RSZTUI_SKIP_ROOT_CHECK=1 跳过。
pub fn is_root() -> bool {
    if env::var_os("RSZTUI_SKIP_ROOT_CHECK").is_some() {
        return true;
    }
    // SAFETY: geteuid 没有前置条件。
    unsafe { libc::geteuid() == 0 }
}

/// 单条外部命令的默认时限，防止 nmcli/asusctl 卡住把整个 TUI 拖死。
const COMMAND_TIMEOUT: Duration = Duration::from_secs(30);

/// 执行命令，强制 C locale，返回 stdout。失败时带上 stderr。
pub fn run(program: &str, args: &[&str]) -> Result<String> {
    run_timeout(program, args, COMMAND_TIMEOUT)
}

pub fn run_timeout(program: &str, args: &[&str], timeout: Duration) -> Result<String> {
    let mut child = Command::new(program)
        .args(args)
        .env("LC_ALL", "C")
        .env("LANG", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("无法执行 {program}，请确认已安装"))?;

    let mut stdout = child.stdout.take().context("无法读取命令的标准输出")?;
    let mut stderr = child.stderr.take().context("无法读取命令的错误输出")?;

    // 单独线程读取管道，避免输出过多时子进程阻塞、双方互等。
    let out_thread = thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout.read_to_end(&mut buf);
        buf
    });
    let err_thread = thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stderr.read_to_end(&mut buf);
        buf
    });

    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().context("等待命令退出失败")? {
            break status;
        }
        if start.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            let _ = out_thread.join();
            let _ = err_thread.join();
            bail!(
                "{program} {} 超时（{} 秒），已终止",
                args.join(" "),
                timeout.as_secs()
            );
        }
        thread::sleep(Duration::from_millis(20));
    };

    let stdout = out_thread.join().unwrap_or_default();
    let stderr = err_thread.join().unwrap_or_default();

    if !status.success() {
        let stderr = String::from_utf8_lossy(&stderr);
        let detail = stderr.trim();
        if detail.is_empty() {
            bail!(
                "{program} {} 失败（退出码 {:?}）",
                args.join(" "),
                status.code()
            );
        }
        bail!("{program} {} 失败：{detail}", args.join(" "));
    }

    Ok(String::from_utf8_lossy(&stdout).into_owned())
}

/// 解析 /etc/passwd 中某个用户的 home。sudo 下 HOME 会变成 /root，这里还原真实用户。
pub fn real_user_home() -> PathBuf {
    if let Some(user) = env::var_os("SUDO_USER") {
        let user = user.to_string_lossy();
        if user != "root"
            && let Some(home) = passwd_home(&user)
        {
            return home;
        }
    }

    env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/root"))
}

fn passwd_home(user: &str) -> Option<PathBuf> {
    let passwd = fs::read_to_string("/etc/passwd").ok()?;
    for line in passwd.lines() {
        let mut fields = line.split(':');
        let name = fields.next()?;
        if name != user {
            continue;
        }
        let home = fields.nth(4)?;
        if !home.is_empty() {
            return Some(PathBuf::from(home));
        }
    }
    None
}

/// 把 root 创建的文件交还给 sudo 用户，避免配置目录变成 root 独占。
pub fn chown_to_sudo_user(path: &Path) {
    let uid = env::var("SUDO_UID")
        .ok()
        .and_then(|v| v.parse::<u32>().ok());
    let gid = env::var("SUDO_GID")
        .ok()
        .and_then(|v| v.parse::<u32>().ok());
    let (Some(uid), Some(gid)) = (uid, gid) else {
        return;
    };

    let _ = std::os::unix::fs::chown(path, Some(uid), Some(gid));
}

pub fn human_bytes(value: f64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = value.max(0.0);
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{value:.0} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RateStyle {
    Binary,
    Decimal,
    Bits,
}

impl RateStyle {
    pub fn from_name(name: &str) -> Self {
        match name {
            "decimal" => Self::Decimal,
            "bits" => Self::Bits,
            _ => Self::Binary,
        }
    }
}

#[cfg(test)]
pub fn human_rate(bytes_per_sec: f64) -> String {
    human_rate_with(bytes_per_sec, RateStyle::Binary)
}

pub fn human_rate_with(bytes_per_sec: f64, style: RateStyle) -> String {
    let (mut value, units, base, bits) = match style {
        RateStyle::Binary => (
            bytes_per_sec.max(0.0),
            ["B/s", "KiB/s", "MiB/s", "GiB/s", "TiB/s"],
            1024.0,
            false,
        ),
        RateStyle::Decimal => (
            bytes_per_sec.max(0.0),
            ["B/s", "KB/s", "MB/s", "GB/s", "TB/s"],
            1000.0,
            false,
        ),
        RateStyle::Bits => (
            bytes_per_sec.max(0.0) * 8.0,
            ["b/s", "Kbit/s", "Mbit/s", "Gbit/s", "Tbit/s"],
            1000.0,
            true,
        ),
    };
    let mut unit = 0;
    while value >= base && unit < units.len() - 1 {
        value /= base;
        unit += 1;
    }
    if unit == 0 {
        let suffix = if bits { "bit/s" } else { units[unit] };
        format!("{value:.0} {suffix}")
    } else {
        format!("{value:.1} {}", units[unit])
    }
}

/// 按字符宽度截断（中文按 2 列算）。
pub fn truncate(text: &str, width: usize) -> String {
    use unicode_width::UnicodeWidthChar;

    let mut used = 0;
    let mut out = String::new();
    for ch in text.chars() {
        let w = ch.width().unwrap_or(0);
        if used + w > width {
            break;
        }
        used += w;
        out.push(ch);
    }
    out
}

/// 解析 nmcli -t 输出：`\:` 表示冒号，`\\` 表示反斜杠。
pub fn split_nmcli(line: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut current = String::new();
    let mut escaped = false;
    for ch in line.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == ':' {
            result.push(std::mem::take(&mut current));
        } else {
            current.push(ch);
        }
    }
    result.push(current);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_rates_with_binary_units() {
        assert_eq!(human_rate(0.0), "0 B/s");
        assert_eq!(human_rate(1536.0), "1.5 KiB/s");
        assert_eq!(human_rate(1024.0 * 1024.0), "1.0 MiB/s");
    }

    #[test]
    fn formats_rates_with_style() {
        assert_eq!(human_rate_with(1_500.0, RateStyle::Decimal), "1.5 KB/s");
        assert_eq!(human_rate_with(1_500.0, RateStyle::Bits), "12.0 Kbit/s");
        assert_eq!(human_rate_with(0.0, RateStyle::Bits), "0 bit/s");
    }

    #[test]
    fn splits_nmcli_escaped_separators() {
        assert_eq!(
            split_nmcli(r"wlan0:wifi:connected:My\:SSID"),
            vec!["wlan0", "wifi", "connected", "My:SSID"]
        );
        assert_eq!(split_nmcli("a:b:c"), vec!["a", "b", "c"]);
    }

    #[test]
    fn timeout_kills_hung_command() {
        let start = Instant::now();
        let err = run_timeout("sleep", &["10"], Duration::from_millis(150)).unwrap_err();
        assert!(err.to_string().contains("超时"), "{err:#}");
        assert!(start.elapsed() < Duration::from_secs(3));
    }
}
