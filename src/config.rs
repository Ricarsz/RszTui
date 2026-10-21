use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::asus::Profile;
use crate::util;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub ui: UiConfig,
    #[serde(default = "default_presets")]
    pub power_presets: Vec<PowerPreset>,
    #[serde(default)]
    pub fan_presets: Vec<FanPreset>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiConfig {
    #[serde(default)]
    pub last_tab: usize,
    #[serde(default = "default_true")]
    pub sparkline: bool,
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default = "default_rate_style")]
    pub rate_style: String,
}

pub const THEMES: [(&str, &str); 5] = [
    ("ocean", "海洋"),
    ("forest", "森林"),
    ("sunset", "日落"),
    ("violet", "紫罗兰"),
    ("mono", "灰阶"),
];

pub const RATE_STYLES: [(&str, &str); 3] = [
    ("binary", "二进制"),
    ("decimal", "十进制"),
    ("bits", "比特"),
];

pub fn theme_index(name: &str) -> usize {
    THEMES.iter().position(|(key, _)| *key == name).unwrap_or(0)
}

pub fn rate_style_index(name: &str) -> usize {
    RATE_STYLES
        .iter()
        .position(|(key, _)| *key == name)
        .unwrap_or(0)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FanPreset {
    pub name: String,
    pub profile: Profile,
    /// asusctl 原始 PWM 格式：31c:26,50c:38,...
    pub cpu: String,
    pub gpu: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PowerPreset {
    pub name: String,
    pub profile: Profile,
    pub pl1: i32,
    pub pl2: i32,
    pub pl3: i32,
    pub gpu_tgp: i32,
    pub gpu_boost: i32,
    pub gpu_temp: i32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            ui: UiConfig::default(),
            power_presets: default_presets(),
            fan_presets: Vec::new(),
        }
    }
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            last_tab: 0,
            sparkline: true,
            theme: default_theme(),
            rate_style: default_rate_style(),
        }
    }
}

fn default_theme() -> String {
    "ocean".into()
}

fn default_rate_style() -> String {
    "binary".into()
}

fn default_true() -> bool {
    true
}

fn default_presets() -> Vec<PowerPreset> {
    vec![
        PowerPreset {
            name: "静音".into(),
            profile: Profile::Quiet,
            pl1: 15,
            pl2: 35,
            pl3: 35,
            gpu_tgp: 55,
            gpu_boost: 5,
            gpu_temp: 75,
        },
        PowerPreset {
            name: "均衡".into(),
            profile: Profile::Balanced,
            pl1: 45,
            pl2: 55,
            pl3: 65,
            gpu_tgp: 55,
            gpu_boost: 5,
            gpu_temp: 75,
        },
        PowerPreset {
            name: "性能".into(),
            profile: Profile::Performance,
            pl1: 55,
            pl2: 60,
            pl3: 65,
            gpu_tgp: 70,
            gpu_boost: 10,
            gpu_temp: 80,
        },
    ]
}

impl Config {
    pub fn path() -> PathBuf {
        util::real_user_home().join(".config/rsztui/config.toml")
    }

    pub fn load() -> Result<Self> {
        let path = Self::path();
        if !path.exists() {
            let cfg = Self::default();
            cfg.save().ok();
            return Ok(cfg);
        }

        let text =
            fs::read_to_string(&path).with_context(|| format!("无法读取 {}", path.display()))?;
        match toml::from_str::<Config>(&text) {
            Ok(cfg) => Ok(cfg),
            Err(err) => {
                // 配置损坏时先留档，再用默认值重建，避免用户手动找回。
                let stamp = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                let broken = path.with_file_name(format!("config.toml.corrupt-{stamp}"));
                fs::rename(&path, &broken)
                    .with_context(|| format!("配置解析失败，且无法备份到 {}", broken.display()))?;
                util::chown_to_sudo_user(&broken);
                eprintln!(
                    "配置解析失败，已备份为 {}，将使用默认配置：{err:#}",
                    broken.display()
                );
                let cfg = Self::default();
                cfg.save()?;
                Ok(cfg)
            }
        }
    }

    pub fn save(&self) -> Result<()> {
        let text = toml::to_string_pretty(self)?;
        write_atomic(&Self::path(), &text)
    }

    pub fn preset(&self, index: usize) -> Option<&PowerPreset> {
        self.power_presets.get(index)
    }
}

/// 先写临时文件并 fsync，再原子替换；旧文件保留为 `.bak`。
/// 这样即使写入过程中断电，也不会留下半截配置。
fn write_atomic(path: &Path, text: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("无法创建 {}", parent.display()))?;
    }

    let backup = with_suffix(path, ".bak");
    if path.exists() {
        fs::copy(path, &backup).with_context(|| format!("无法备份配置到 {}", backup.display()))?;
        util::chown_to_sudo_user(&backup);
    }

    let tmp = with_suffix(path, ".tmp");
    let result = (|| -> Result<()> {
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)
            .with_context(|| format!("无法写入 {}", tmp.display()))?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        drop(file);
        util::chown_to_sudo_user(&tmp);
        fs::rename(&tmp, path).with_context(|| format!("无法替换 {}", path.display()))?;
        util::chown_to_sudo_user(path);
        if let Some(parent) = path.parent() {
            util::chown_to_sudo_user(parent);
        }
        Ok(())
    })();

    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(suffix);
    path.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_config(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "rsztui-config-test-{}-{name}.toml",
            std::process::id()
        ))
    }

    #[test]
    fn atomic_write_keeps_backup_and_content() {
        let path = temp_config("write");
        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(with_suffix(&path, ".bak"));

        write_atomic(&path, "first = 1\n").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "first = 1\n");

        write_atomic(&path, "second = 2\n").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "second = 2\n");
        assert_eq!(
            fs::read_to_string(with_suffix(&path, ".bak")).unwrap(),
            "first = 1\n"
        );

        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(with_suffix(&path, ".bak"));
    }

    #[test]
    fn with_suffix_appends_after_extension() {
        assert_eq!(
            with_suffix(Path::new("/tmp/config.toml"), ".bak"),
            PathBuf::from("/tmp/config.toml.bak")
        );
    }
}
