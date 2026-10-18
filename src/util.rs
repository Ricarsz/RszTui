use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};

/// 是否以 root 运行。开发调试时可用 ROGTUI_SKIP_ROOT_CHECK=1 跳过。
pub fn is_root() -> bool {
    if env::var_os("ROGTUI_SKIP_ROOT_CHECK").is_some() {
        return true;
    }
    // SAFETY: geteuid 没有前置条件。
    unsafe { libc::geteuid() == 0 }
}

/// 执行命令，强制 C locale，返回 stdout。失败时带上 stderr。
pub fn run(program: &str, args: &[&str]) -> Result<String> {
    let output = Command::new(program)
        .args(args)
        .env("LC_ALL", "C")
        .env("LANG", "C")
        .output()
        .with_context(|| format!("无法执行 {program}，请确认已安装"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let detail = stderr.trim();
        if detail.is_empty() {
            bail!(
                "{program} {} 失败（退出码 {:?}）",
                args.join(" "),
                output.status.code()
            );
        }
        bail!("{program} {} 失败：{detail}", args.join(" "));
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
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

pub fn timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 备份 asusd 的配置文件，返回本次备份目录。
pub fn backup_asusd() -> Result<PathBuf> {
    let stamp = timestamp();
    let base = env::var_os("ROGTUI_BACKUP_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/var/backups/rogtui"));
    let dir = base.join(stamp.to_string());
    fs::create_dir_all(&dir).with_context(|| format!("无法创建备份目录 {}", dir.display()))?;

    let files = [
        "/etc/asusd/asusd.ron",
        "/etc/asusd/fan_curves.ron",
        "/etc/asusd/aura_tuf.ron",
    ];

    let mut copied = Vec::new();
    for file in files {
        let src = Path::new(file);
        if !src.exists() {
            continue;
        }
        let name = src.file_name().unwrap_or_default();
        let dst = dir.join(name);
        fs::copy(src, &dst).with_context(|| format!("备份 {} 失败", src.display()))?;
        copied.push(name.to_string_lossy().into_owned());
    }

    if copied.is_empty() {
        bail!("没有找到可备份的 asusd 配置文件");
    }

    Ok(dir)
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

pub fn human_rate(bytes_per_sec: f64) -> String {
    format!("{}/s", human_bytes(bytes_per_sec))
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
