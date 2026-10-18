use std::collections::VecDeque;
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow, bail};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::DefaultTerminal;

use crate::asus::{self, BatteryInfo, GpuMode, PowerField, PowerSource, Profile, ProfileState};
use crate::config::Config;
use crate::sys::{Collector, Snapshot};
use crate::ui;
use crate::util;

pub const HISTORY: usize = 60;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    Dashboard,
    Performance,
    Battery,
    Fan,
    Lighting,
    Network,
    Settings,
}

impl Tab {
    pub const ALL: [Tab; 7] = [
        Tab::Dashboard,
        Tab::Performance,
        Tab::Battery,
        Tab::Fan,
        Tab::Lighting,
        Tab::Network,
        Tab::Settings,
    ];

    pub fn zh(self) -> &'static str {
        match self {
            Tab::Dashboard => "仪表盘",
            Tab::Performance => "性能",
            Tab::Battery => "电池",
            Tab::Fan => "风扇",
            Tab::Lighting => "灯光",
            Tab::Network => "网络",
            Tab::Settings => "设置",
        }
    }

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|t| *t == self).unwrap_or(0)
    }

    pub fn from_index(index: usize) -> Self {
        Self::ALL.get(index).copied().unwrap_or(Tab::Dashboard)
    }

    pub fn next(self) -> Self {
        Self::from_index((self.index() + 1) % Self::ALL.len())
    }

    pub fn prev(self) -> Self {
        Self::from_index((self.index() + Self::ALL.len() - 1) % Self::ALL.len())
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PerfFocus {
    Profiles,
    Power,
}

#[derive(Debug, Clone, Default)]
pub struct NetDevice {
    pub name: String,
    pub kind: String,
    pub state: String,
    pub connection: String,
}

#[derive(Debug, Clone, Default)]
pub struct NetSummary {
    pub devices: Vec<NetDevice>,
    pub wifi_enabled: Option<bool>,
    pub ssid: Option<String>,
}

#[derive(Debug)]
pub enum Pending {
    Profile(Profile),
    DefaultAc(Profile),
    DefaultBattery(Profile),
    Preset(usize),
    PowerApply,
    BatteryLimit(u8),
    Oneshot,
    Gpu(GpuMode),
    Quit,
}

#[derive(Debug)]
pub enum Modal {
    Confirm {
        title: String,
        body: String,
        pending: Pending,
    },
    Message {
        title: String,
        body: String,
    },
}

pub struct App {
    pub cfg: Config,
    pub tab: Tab,
    pub profile: Option<ProfileState>,
    pub battery: BatteryInfo,
    pub attrs: Vec<asus::Attr>,
    pub power: Vec<PowerField>,
    pub power_dirty: bool,
    pub power_sel: usize,
    pub preset_sel: usize,
    pub perf_focus: PerfFocus,
    pub profile_sel: usize,
    pub gpu_mode: Option<GpuMode>,
    pub gpu_sel: usize,
    pub fan_curves: Vec<asus::FanCurve>,
    pub fan_enabled: String,
    pub led: String,
    pub tuning: bool,
    pub net: NetSummary,
    pub snap: Snapshot,
    pub cpu_hist: VecDeque<u64>,
    pub mem_hist: VecDeque<u64>,
    pub rx_hist: VecDeque<u64>,
    pub tx_hist: VecDeque<u64>,
    pub limit_edit: u8,
    pub limit_dirty: bool,
    pub status: String,
    pub status_err: bool,
    pub modal: Option<Modal>,
    pub quit: bool,
    backed_up: bool,
    last_slow: Instant,
    last_tick: Instant,
    collector: Collector,
}

pub fn run(cfg: Config) -> Result<()> {
    let mut app = App::new(cfg);
    let mut terminal = ratatui::init();
    let result = app.event_loop(&mut terminal);
    ratatui::restore();
    result
}

impl App {
    pub fn new(cfg: Config) -> Self {
        let tab = Tab::from_index(cfg.ui.last_tab);
        let mut app = Self {
            cfg,
            tab,
            profile: None,
            battery: BatteryInfo::default(),
            attrs: Vec::new(),
            power: Vec::new(),
            power_dirty: false,
            power_sel: 0,
            preset_sel: 0,
            perf_focus: PerfFocus::Profiles,
            profile_sel: 0,
            gpu_mode: None,
            gpu_sel: 1,
            fan_curves: Vec::new(),
            fan_enabled: String::new(),
            led: "未知".into(),
            tuning: false,
            net: NetSummary::default(),
            snap: Snapshot::default(),
            cpu_hist: VecDeque::with_capacity(HISTORY),
            mem_hist: VecDeque::with_capacity(HISTORY),
            rx_hist: VecDeque::with_capacity(HISTORY),
            tx_hist: VecDeque::with_capacity(HISTORY),
            limit_edit: 80,
            limit_dirty: false,
            status: "就绪".into(),
            status_err: false,
            modal: None,
            quit: false,
            backed_up: false,
            last_slow: Instant::now(),
            last_tick: Instant::now(),
            collector: Collector::default(),
        };

        app.refresh_fast();
        app.refresh_slow();
        if let Some(profile) = app.profile {
            app.profile_sel = Profile::ALL
                .iter()
                .position(|p| *p == profile.active)
                .unwrap_or(0);
        }
        if let Some(mode) = app.gpu_mode {
            app.gpu_sel = GpuMode::ALL.iter().position(|m| *m == mode).unwrap_or(1);
        }
        if let Some(limit) = app.battery.limit {
            app.limit_edit = limit;
        }
        app
    }

    fn event_loop(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        loop {
            terminal.draw(|frame| ui::draw(frame, self))?;

            if event::poll(Duration::from_millis(200))?
                && let Event::Key(key) = event::read()?
                && key.kind == KeyEventKind::Press
                && let Err(err) = self.handle_key(key)
            {
                self.status = format!("错误：{err:#}");
                self.status_err = true;
            }

            if self.last_tick.elapsed() >= Duration::from_secs(1) {
                self.refresh_fast();
                self.last_tick = Instant::now();
            }

            if self.last_slow.elapsed() >= Duration::from_secs(5) {
                self.refresh_slow();
                self.last_slow = Instant::now();
            }

            if self.quit {
                self.cfg.ui.last_tab = self.tab.index();
                let _ = self.cfg.save();
                break;
            }
        }
        Ok(())
    }

    fn handle_key(&mut self, key: KeyEvent) -> Result<()> {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.quit = true;
            return Ok(());
        }

        if self.modal.is_some() {
            return self.handle_modal_key(key);
        }

        match key.code {
            KeyCode::Char('q') => {
                self.confirm("退出", "退出 rogtui？", Pending::Quit);
            }
            KeyCode::Char('r') => {
                self.refresh_slow();
                self.status = "已刷新".into();
                self.status_err = false;
            }
            KeyCode::Char('?') => {
                self.modal = Some(Modal::Message {
                    title: "快捷键".into(),
                    body: help_text(),
                });
            }
            KeyCode::Tab => self.switch_tab(Tab::next(self.tab)),
            KeyCode::BackTab => self.switch_tab(Tab::prev(self.tab)),
            KeyCode::Char(c @ '1'..='7') => {
                let index = c as usize - '1' as usize;
                self.switch_tab(Tab::from_index(index));
            }
            _ => self.handle_tab_key(key)?,
        }

        Ok(())
    }

    fn handle_modal_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc => {
                self.modal = None;
            }
            KeyCode::Enter => match self.modal.take() {
                Some(Modal::Confirm { pending, .. }) => self.apply_pending(pending)?,
                _ => self.modal = None,
            },
            _ => {}
        }
        Ok(())
    }

    fn switch_tab(&mut self, tab: Tab) {
        self.tab = tab;
        self.cfg.ui.last_tab = tab.index();
        let _ = self.cfg.save();
    }

    fn handle_tab_key(&mut self, key: KeyEvent) -> Result<()> {
        match self.tab {
            Tab::Performance => self.handle_performance_key(key)?,
            Tab::Battery => self.handle_battery_key(key)?,
            Tab::Lighting => self.handle_lighting_key(key)?,
            _ => {}
        }
        Ok(())
    }

    fn handle_performance_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Char('f') => {
                self.perf_focus = match self.perf_focus {
                    PerfFocus::Profiles => PerfFocus::Power,
                    PerfFocus::Power => PerfFocus::Profiles,
                };
            }
            KeyCode::Char('g') => {
                self.gpu_sel = (self.gpu_sel + 1) % GpuMode::ALL.len();
            }
            KeyCode::Char('G') => {
                let mode = GpuMode::ALL[self.gpu_sel % GpuMode::ALL.len()];
                self.confirm(
                    "切换 GPU 模式",
                    format!(
                        "将 GPU 模式设为{}？\n\n{}\n\n修改后需要重启系统才能生效。",
                        mode.zh(),
                        mode.detail()
                    ),
                    Pending::Gpu(mode),
                );
            }
            _ => match self.perf_focus {
                PerfFocus::Profiles => match key.code {
                    KeyCode::Down | KeyCode::Char('j') => {
                        self.profile_sel = (self.profile_sel + 1) % Profile::ALL.len();
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        self.profile_sel =
                            (self.profile_sel + Profile::ALL.len() - 1) % Profile::ALL.len();
                    }
                    KeyCode::Enter => {
                        let profile = Profile::ALL[self.profile_sel];
                        self.confirm(
                            "切换性能模式",
                            format!("切换到{}模式？", profile.zh()),
                            Pending::Profile(profile),
                        );
                    }
                    KeyCode::Char('a') => {
                        let profile = Profile::ALL[self.profile_sel];
                        self.confirm(
                            "设置 AC 默认模式",
                            format!("AC 供电时默认使用{}模式？", profile.zh()),
                            Pending::DefaultAc(profile),
                        );
                    }
                    KeyCode::Char('b') => {
                        let profile = Profile::ALL[self.profile_sel];
                        self.confirm(
                            "设置电池默认模式",
                            format!("电池供电时默认使用{}模式？", profile.zh()),
                            Pending::DefaultBattery(profile),
                        );
                    }
                    _ => {}
                },
                PerfFocus::Power => match key.code {
                    KeyCode::Down | KeyCode::Char('j') => {
                        if !self.power.is_empty() {
                            self.power_sel = (self.power_sel + 1) % self.power.len();
                        }
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        if !self.power.is_empty() {
                            self.power_sel =
                                (self.power_sel + self.power.len() - 1) % self.power.len();
                        }
                    }
                    KeyCode::Left | KeyCode::Char('h') => self.adjust_power(-1),
                    KeyCode::Right | KeyCode::Char('l') => self.adjust_power(1),
                    KeyCode::Char('H') => self.adjust_power(-5),
                    KeyCode::Char('L') => self.adjust_power(5),
                    KeyCode::Enter | KeyCode::Char('a') => {
                        if self.power.is_empty() {
                            bail!("没有可写的功耗属性");
                        }
                        self.confirm("应用功耗", self.power_summary(), Pending::PowerApply);
                    }
                    KeyCode::Char('p') => {
                        let preset = self
                            .cfg
                            .preset(self.preset_sel)
                            .ok_or_else(|| anyhow!("没有这个预设"))?
                            .clone();
                        self.confirm(
                            "套用功耗预设",
                            format!(
                                "套用「{}」预设到 {} 模式？\n\nPL {} / {} / {} W\ndGPU {} W，boost {}，温度墙 {}°C",
                                preset.name,
                                preset.profile.zh(),
                                preset.pl1,
                                preset.pl2,
                                preset.pl3,
                                preset.gpu_tgp,
                                preset.gpu_boost,
                                preset.gpu_temp
                            ),
                            Pending::Preset(self.preset_sel),
                        );
                    }
                    KeyCode::Char(']') if !self.cfg.power_presets.is_empty() => {
                        self.preset_sel = (self.preset_sel + 1) % self.cfg.power_presets.len();
                    }
                    KeyCode::Char('[') if !self.cfg.power_presets.is_empty() => {
                        self.preset_sel = (self.preset_sel + self.cfg.power_presets.len() - 1)
                            % self.cfg.power_presets.len();
                    }
                    _ => {}
                },
            },
        }
        Ok(())
    }

    fn adjust_power(&mut self, delta: i32) {
        let Some(field) = self.power.get_mut(self.power_sel) else {
            return;
        };
        field.value = (field.value + delta).clamp(field.min, field.max);
        self.power_dirty = true;
    }

    fn power_summary(&self) -> String {
        let mut lines = vec![
            "将以下值写入当前性能模式（持久化）：".to_string(),
            String::new(),
        ];
        for field in &self.power {
            lines.push(format!("{}  {} {}", field.label, field.value, field.unit));
        }
        lines.join("\n")
    }

    fn handle_battery_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Left | KeyCode::Char('h') => {
                self.limit_edit = self.limit_edit.saturating_sub(5).max(20);
                self.limit_dirty = true;
            }
            KeyCode::Right | KeyCode::Char('l') => {
                self.limit_edit = (self.limit_edit + 5).min(100);
                self.limit_dirty = true;
            }
            KeyCode::Enter | KeyCode::Char('a') => {
                self.confirm(
                    "设置充电上限",
                    format!("把电池充电上限设为 {}%？", self.limit_edit),
                    Pending::BatteryLimit(self.limit_edit),
                );
            }
            KeyCode::Char('o') => {
                self.confirm("单次充满", "忽略充电上限，单次充满电池？", Pending::Oneshot);
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_lighting_key(&mut self, key: KeyEvent) -> Result<()> {
        if matches!(key.code, KeyCode::Char('b')) {
            asus::led_next()?;
            self.refresh_slow();
            self.status = "键盘亮度已切换".into();
            self.status_err = false;
        }
        Ok(())
    }

    fn confirm(&mut self, title: &str, body: impl Into<String>, pending: Pending) {
        self.modal = Some(Modal::Confirm {
            title: title.into(),
            body: body.into(),
            pending,
        });
    }

    fn apply_pending(&mut self, pending: Pending) -> Result<()> {
        if matches!(pending, Pending::Quit) {
            self.quit = true;
            return Ok(());
        }

        self.status_err = false;
        self.ensure_backup()?;

        match pending {
            Pending::Quit => {}
            Pending::Profile(profile) => {
                asus::profile_set(profile)?;
                self.status = format!("已切换到{}模式", profile.zh());
            }
            Pending::DefaultAc(profile) => {
                asus::profile_set_default(PowerSource::Ac, profile)?;
                self.status = format!("AC 默认模式已设为{}", profile.zh());
            }
            Pending::DefaultBattery(profile) => {
                asus::profile_set_default(PowerSource::Battery, profile)?;
                self.status = format!("电池默认模式已设为{}", profile.zh());
            }
            Pending::Preset(index) => {
                let preset = self
                    .cfg
                    .preset(index)
                    .cloned()
                    .ok_or_else(|| anyhow!("没有这个预设"))?;
                asus::profile_set(preset.profile)?;
                let installed: Vec<&str> = self.attrs.iter().map(|a| a.name.as_str()).collect();
                for (key, value) in [
                    ("ppt_pl1_spl", preset.pl1),
                    ("ppt_pl2_sppt", preset.pl2),
                    ("ppt_pl3_fppt", preset.pl3),
                    ("nv_tgp", preset.gpu_tgp),
                    ("nv_dynamic_boost", preset.gpu_boost),
                    ("nv_temp_target", preset.gpu_temp),
                ] {
                    if installed.contains(&key) {
                        asus::armoury_set(key, value)?;
                    }
                }
                self.power_dirty = false;
                self.status = format!("已套用「{}」预设到{}模式", preset.name, preset.profile.zh());
            }
            Pending::PowerApply => {
                if self.power.is_empty() {
                    bail!("没有可写的功耗属性");
                }
                let mut max_pl3 = 0;
                for field in &self.power {
                    asus::armoury_set(field.key, field.value)?;
                    if field.key == "ppt_pl3_fppt" {
                        max_pl3 = field.value;
                    }
                }
                self.power_dirty = false;
                self.status = if max_pl3 > 65 {
                    format!("功耗已应用（PL3 {max_pl3}W，高于 65W，注意温度）")
                } else {
                    "功耗已应用并持久化".into()
                };
            }
            Pending::BatteryLimit(limit) => {
                asus::battery_limit(limit)?;
                self.limit_dirty = false;
                self.status = format!("充电上限已设为 {limit}%");
            }
            Pending::Oneshot => {
                asus::battery_oneshot(None)?;
                self.status = "已触发单次充满".into();
            }
            Pending::Gpu(mode) => {
                asus::gpu_set(mode)?;
                self.status = format!("GPU 模式已设为{}，请重启生效", mode.zh());
            }
        }

        self.refresh_slow();
        Ok(())
    }

    fn ensure_backup(&mut self) -> Result<()> {
        if self.backed_up {
            return Ok(());
        }
        let dir = util::backup_asusd()?;
        self.backed_up = true;
        self.status = format!("配置已备份到 {}", dir.display());
        Ok(())
    }

    pub fn refresh_fast(&mut self) {
        let snap = self.collector.sample();
        push_hist(&mut self.cpu_hist, snap.cpu_pct as u64);
        let mem_pct = if snap.mem_total > 0 {
            (snap.mem_used as f64 / snap.mem_total as f64 * 100.0) as u64
        } else {
            0
        };
        push_hist(&mut self.mem_hist, mem_pct);
        push_hist(&mut self.rx_hist, (snap.rx_bps / 1024.0) as u64);
        push_hist(&mut self.tx_hist, (snap.tx_bps / 1024.0) as u64);
        self.snap = snap;
    }

    pub fn refresh_slow(&mut self) {
        let mut errors = Vec::new();

        match asus::profile_get() {
            Ok(profile) => self.profile = Some(profile),
            Err(err) => errors.push(format!("性能模式：{err}")),
        }

        match asus::battery_info() {
            Ok(info) => {
                if !self.limit_dirty
                    && let Some(limit) = info.limit
                {
                    self.limit_edit = limit;
                }
                self.battery = info;
            }
            Err(err) => errors.push(format!("电池：{err}")),
        }

        match asus::armoury() {
            Ok(attrs) => {
                self.gpu_mode = asus::gpu_mode(&attrs);
                if !self.power_dirty {
                    self.power = asus::power_fields(&attrs);
                    self.power_sel = self.power_sel.min(self.power.len().saturating_sub(1));
                }
                self.attrs = attrs;
            }
            Err(err) => errors.push(format!("功耗：{err}")),
        }

        self.tuning = asus::profile_tuning().unwrap_or(false);

        let profile = self.profile.map(|p| p.active).unwrap_or(Profile::Balanced);
        if let Ok(curves) = asus::fan_curves(profile) {
            self.fan_curves = curves;
        }
        if let Ok(summary) = asus::fan_enabled_summary() {
            self.fan_enabled = summary;
        }
        if let Ok(led) = asus::led_status() {
            self.led = led;
        }

        self.net = net_summary();

        if !errors.is_empty() {
            self.status = format!("刷新失败：{}", errors.join("；"));
            self.status_err = true;
        }
    }
}

fn push_hist(hist: &mut VecDeque<u64>, value: u64) {
    if hist.len() >= HISTORY {
        hist.pop_front();
    }
    hist.push_back(value);
}

fn net_summary() -> NetSummary {
    let mut summary = NetSummary::default();

    if let Ok(out) = util::run(
        "nmcli",
        &[
            "-t",
            "-f",
            "DEVICE,TYPE,STATE,CONNECTION",
            "device",
            "status",
        ],
    ) {
        for line in out.lines() {
            let parts = split_nmcli(line);
            if parts.len() < 4 {
                continue;
            }
            summary.devices.push(NetDevice {
                name: parts[0].clone(),
                kind: parts[1].clone(),
                state: parts[2].clone(),
                connection: parts[3].clone(),
            });
        }
    }

    if let Ok(out) = util::run("nmcli", &["radio", "wifi"]) {
        summary.wifi_enabled = Some(out.trim().eq_ignore_ascii_case("enabled"));
    }

    if let Ok(out) = util::run("nmcli", &["-t", "-f", "ACTIVE,SSID", "device", "wifi"]) {
        for line in out.lines() {
            let parts = split_nmcli(line);
            if parts.first().map(String::as_str) == Some("yes")
                && let Some(ssid) = parts.get(1)
                && !ssid.is_empty()
            {
                summary.ssid = Some(ssid.clone());
            }
        }
    }

    summary
}

fn split_nmcli(line: &str) -> Vec<String> {
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

fn help_text() -> String {
    [
        "全局",
        "  Tab / Shift+Tab  切换页面     1-7  直接跳转",
        "  r 刷新   ? 帮助   q 退出      Ctrl+C 立即退出",
        "",
        "性能",
        "  f 切换焦点   j/k 选择   Enter 应用",
        "  a 设为 AC 默认   b 设为电池默认",
        "  功耗焦点：h/l 微调（H/L 每次 5）  a 应用  p 套用预设  [ ] 选择预设",
        "  g 选择 GPU 模式   G 应用（需重启）",
        "",
        "电池",
        "  h/l 调整上限   Enter 应用   o 单次充满",
        "",
        "灯光",
        "  b 切换键盘亮度",
        "",
        "说明",
        "  所有写入都会先备份 /etc/asusd/*.ron 到 /var/backups/rogtui/",
        "  功耗值写入当前性能模式的 tuning，重启后由 asusd 自动恢复",
    ]
    .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_nmcli_escaped_separators() {
        assert_eq!(
            split_nmcli(r"wlan0:wifi:connected:My\:SSID"),
            vec!["wlan0", "wifi", "connected", "My:SSID"]
        );
        assert_eq!(split_nmcli("a:b:c"), vec!["a", "b", "c"]);
    }

    #[test]
    fn history_keeps_capacity() {
        let mut hist = VecDeque::new();
        for i in 0..(HISTORY + 5) {
            push_hist(&mut hist, i as u64);
        }
        assert_eq!(hist.len(), HISTORY);
        assert_eq!(*hist.front().unwrap(), 5);
        assert_eq!(*hist.back().unwrap(), (HISTORY + 4) as u64);
    }
}
