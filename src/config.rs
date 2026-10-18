use std::fs;
use std::path::PathBuf;

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
        let cfg: Config =
            toml::from_str(&text).with_context(|| format!("解析 {} 失败", path.display()))?;
        Ok(cfg)
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| format!("无法创建 {}", parent.display()))?;
        }
        let text = toml::to_string_pretty(self)?;
        fs::write(&path, text).with_context(|| format!("无法写入 {}", path.display()))?;
        util::chown_to_sudo_user(&path);
        if let Some(parent) = path.parent() {
            util::chown_to_sudo_user(parent);
        }
        Ok(())
    }

    pub fn preset(&self, index: usize) -> Option<&PowerPreset> {
        self.power_presets.get(index)
    }
}
