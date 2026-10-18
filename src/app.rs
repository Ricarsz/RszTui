use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow, bail};
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
    KeyEventState, KeyModifiers, KeyboardEnhancementFlags, MouseButton, MouseEvent, MouseEventKind,
    PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::execute;
use ratatui::DefaultTerminal;
use ratatui::layout::Rect;

use crate::asus::{self, BatteryInfo, GpuMode, PowerField, PowerSource, Profile, ProfileState};
use crate::config::{Config, FanPreset};
use crate::deps;
use crate::hw;
use crate::nm;
use crate::sys::{Collector, Snapshot};
use crate::ui;

pub const HISTORY: usize = 60;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    Dashboard,
    Performance,
    Battery,
    Fan,
    Lighting,
    Network,
    Services,
    Settings,
}

impl Tab {
    pub const ALL: [Tab; 8] = [
        Tab::Dashboard,
        Tab::Performance,
        Tab::Battery,
        Tab::Fan,
        Tab::Lighting,
        Tab::Network,
        Tab::Services,
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
            Tab::Services => "服务",
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

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NetView {
    Connections,
    Wifi,
    Vpn,
}

impl NetView {
    pub const ALL: [NetView; 3] = [NetView::Connections, NetView::Wifi, NetView::Vpn];

    pub fn zh(self) -> &'static str {
        match self {
            NetView::Connections => "连接",
            NetView::Wifi => "Wi-Fi",
            NetView::Vpn => "VPN",
        }
    }

    pub fn next(self) -> Self {
        match self {
            NetView::Connections => NetView::Wifi,
            NetView::Wifi => NetView::Vpn,
            NetView::Vpn => NetView::Connections,
        }
    }
}

#[derive(Debug, Clone)]
pub struct IpForm {
    pub conn: String,
    pub method: String,
    pub address: String,
    pub gateway: String,
    pub dns: String,
    pub dot: i32,
    pub sel: usize,
}

impl IpForm {
    pub const LABELS: [&'static str; 5] = ["IPv4 方法", "地址", "网关", "DNS", "DoT"];

    pub fn value(&self, index: usize) -> String {
        match index {
            0 => self.method.clone(),
            1 => self.address.clone(),
            2 => self.gateway.clone(),
            3 => self.dns.clone(),
            4 => match self.dot {
                -1 => "默认".into(),
                0 => "关闭".into(),
                1 => "机会性".into(),
                2 => "强制".into(),
                _ => "默认".into(),
            },
            _ => String::new(),
        }
    }

    pub fn editable(&self, index: usize) -> bool {
        (1..=3).contains(&index) && self.method == "manual"
    }
}

#[derive(Debug, Clone)]
pub struct FanEdit {
    pub fan: String,
    pub temp: Vec<u8>,
    pub pwm: Vec<u8>,
    pub sel: usize,
    pub dirty: bool,
}

impl FanEdit {
    pub fn from_curve(curve: &asus::FanCurve, fan: &str) -> Option<Self> {
        if curve.temp.len() != curve.pwm.len() || curve.temp.is_empty() {
            return None;
        }
        Some(Self {
            fan: fan.to_string(),
            temp: curve.temp.clone(),
            pwm: curve.pwm.clone(),
            sel: 0,
            dirty: false,
        })
    }

    pub fn data(&self) -> String {
        self.temp
            .iter()
            .zip(self.pwm.iter())
            .map(|(temp, pwm)| format!("{temp}c:{pwm}"))
            .collect::<Vec<_>>()
            .join(",")
    }

    pub fn percent(&self, index: usize) -> u32 {
        self.pwm
            .get(index)
            .map(|pwm| ((*pwm as f32 / 255.0) * 100.0).round() as u32)
            .unwrap_or(0)
    }

    pub fn adjust_pwm(&mut self, delta: i32) {
        if let Some(value) = self.pwm.get_mut(self.sel) {
            *value = (*value as i32 + delta).clamp(0, 255) as u8;
            self.dirty = true;
        }
    }

    pub fn adjust_temp(&mut self, delta: i32) {
        if self.sel >= self.temp.len() {
            return;
        }
        let lower = if self.sel == 0 {
            20
        } else {
            self.temp[self.sel - 1] as i32 + 1
        };
        let upper = if self.sel + 1 >= self.temp.len() {
            100
        } else {
            self.temp[self.sel + 1] as i32 - 1
        };
        let current = self.temp[self.sel] as i32;
        let upper = upper.max(lower);
        self.temp[self.sel] = (current + delta).clamp(lower, upper) as u8;
        self.dirty = true;
    }
}

#[derive(Debug, Clone)]
pub enum InputAction {
    Wifi { ssid: String, key_mgmt: String },
    Hotspot,
    Ping,
    DnsLookup,
    DnsCacheSize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseAction {
    Tab(Tab),
    Profile(usize),
    PerfPreset(usize),
    PowerField(usize),
    FanPreset(usize),
    FanPoint(usize),
    Net(usize),
    Service(usize),
    Gpu(usize),
    Color(usize),
    CacheFlushDns,
    CacheFlushNeighbors,
    CacheEditSize,
    CacheRefresh,
    CacheClose,
    NetPing,
    NetDns,
    NetCache,
    NetRefresh,
    Key(KeyCode, KeyModifiers),
    PerfSwitch,
    PerfAc,
    PerfBat,
    PerfPowerApply,
    PerfPresetApply,
    ModalConfirm,
    ModalCancel,
}

#[derive(Debug, Clone, Copy)]
pub struct MouseTarget {
    pub rect: Rect,
    pub action: MouseAction,
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
    NetApply(IpForm),
    NetDelete(String),
    Radio(bool),
    FlushDns,
    FlushNeighbors,
    FanApply(usize),
    FanSave,
    FanDelete(usize),
    FanDefault,
    FanToggle(bool),
    FanCurveApply(FanEdit),
    FanEditClose,
    Service {
        unit: String,
        action: crate::services::ServiceAction,
    },
    HotspotOff,
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
    Input {
        title: String,
        prompt: String,
        value: String,
        masked: bool,
        action: InputAction,
    },
}

struct CoreSnapshot {
    profile: Result<ProfileState, String>,
    battery: Result<BatteryInfo, String>,
    attrs: Result<Vec<asus::Attr>, String>,
    tuning: bool,
}

pub struct App {
    pub cfg: Config,
    pub deps: deps::Report,
    pub hw: hw::HardwareInfo,
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
    pub fan_sel: usize,
    pub fan_edit: Option<FanEdit>,
    pub led: String,
    pub aura: Option<asus::AuraStatus>,
    pub aura_color: usize,
    pub tuning: bool,
    pub net_view: NetView,
    pub net_sel: usize,
    pub devices: Vec<nm::Device>,
    pub connections: Vec<nm::ConnectionInfo>,
    pub wifi: Vec<nm::WifiAp>,
    pub vpns: Vec<nm::ConnectionInfo>,
    pub ssid: Option<String>,
    pub wifi_enabled: Option<bool>,
    pub ip_form: Option<IpForm>,
    pub net_ipv4: Option<nm::Ipv4Settings>,
    pub net_detail: Option<nm::DeviceDetail>,
    net_detail_at: Instant,
    pub cache_open: bool,
    pub cache_stats: Option<nm::CacheStats>,
    net_tx: Sender<Result<nm::NetworkSnapshot, String>>,
    net_rx: Receiver<Result<nm::NetworkSnapshot, String>>,
    pub net_loading: bool,
    net_refresh_again: bool,
    net_force_scan: bool,
    last_net_header: Instant,
    pub hover: Option<MouseAction>,
    core_tx: Sender<CoreSnapshot>,
    core_rx: Receiver<CoreSnapshot>,
    core_loading: bool,
    core_refresh_again: bool,
    last_mouse: Option<(Instant, MouseAction)>,
    pub services: Vec<crate::services::ServiceInfo>,
    pub service_sel: usize,
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
    pub mouse_targets: RefCell<Vec<MouseTarget>>,
    pub quit: bool,
    last_slow: Instant,
    last_tick: Instant,
    collector: Collector,
}

pub fn run(cfg: Config, report: deps::Report) -> Result<()> {
    let mut app = App::new(cfg, report);
    let mut terminal = ratatui::init();
    // 启用 Kitty 键盘协议，让终端把数字小键盘标记为 KEYPAD（配合 NumLock）。
    let _ = execute!(
        std::io::stdout(),
        PushKeyboardEnhancementFlags(
            KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                | KeyboardEnhancementFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES
        ),
        EnableMouseCapture
    );
    let result = app.event_loop(&mut terminal);
    let _ = execute!(
        std::io::stdout(),
        PopKeyboardEnhancementFlags,
        DisableMouseCapture
    );
    ratatui::restore();
    result
}

impl App {
    pub fn new(cfg: Config, report: deps::Report) -> Self {
        let (net_tx, net_rx) = mpsc::channel();
        let (core_tx, core_rx) = mpsc::channel();
        let tab = Tab::from_index(cfg.ui.last_tab);
        let mut app = Self {
            cfg,
            deps: report,
            hw: hw::collect(),
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
            fan_sel: 0,
            fan_edit: None,
            led: "未知".into(),
            aura: None,
            aura_color: 0,
            tuning: false,
            net_view: NetView::Connections,
            net_sel: 0,
            devices: Vec::new(),
            connections: Vec::new(),
            wifi: Vec::new(),
            vpns: Vec::new(),
            ssid: None,
            wifi_enabled: None,
            ip_form: None,
            net_ipv4: None,
            net_detail: None,
            net_detail_at: Instant::now(),
            cache_open: false,
            cache_stats: None,
            net_tx,
            net_rx,
            net_loading: false,
            net_refresh_again: false,
            core_tx,
            core_rx,
            core_loading: false,
            core_refresh_again: false,
            net_force_scan: false,
            last_net_header: Instant::now(),
            hover: None,
            last_mouse: None,
            services: Vec::new(),
            service_sel: 0,
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
            mouse_targets: RefCell::new(Vec::new()),
            quit: false,
            last_slow: Instant::now(),
            last_tick: Instant::now(),
            collector: Collector::default(),
        };

        app.refresh_fast();
        app.refresh_core_sync();
        app.refresh_current_tab();
        app.request_net_snapshot();
        app.tuning = asus::profile_tuning().unwrap_or(false);
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
        if app.deps.has_warnings() {
            app.modal = Some(Modal::Message {
                title: "依赖检查".into(),
                body: app.deps.summary_text(),
            });
        }
        app
    }

    fn event_loop(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        let mut dirty = true;
        loop {
            if dirty {
                terminal.draw(|frame| ui::draw(frame, self))?;
                dirty = false;
            }

            if event::poll(Duration::from_millis(200))? {
                match event::read()? {
                    Event::Key(key) if key.kind == KeyEventKind::Press => {
                        if let Err(err) = self.handle_key(key) {
                            self.status = format!("错误：{err:#}");
                            self.status_err = true;
                        }
                        dirty = true;
                    }
                    Event::Mouse(mouse) => {
                        let moved = matches!(mouse.kind, MouseEventKind::Moved);
                        let hover_before = self.hover;
                        if let Err(err) = self.handle_mouse(mouse) {
                            self.status = format!("错误：{err:#}");
                            self.status_err = true;
                        }
                        if !moved || self.hover != hover_before {
                            dirty = true;
                        }
                    }
                    _ => {}
                }
            }

            if self.poll_net() {
                dirty = true;
            }
            if self.poll_core() {
                dirty = true;
            }

            if self.last_tick.elapsed() >= Duration::from_secs(1) {
                self.refresh_fast();
                self.last_tick = Instant::now();
                dirty = true;
            }

            if self.last_slow.elapsed() >= Duration::from_secs(5) {
                self.refresh_slow();
                self.last_slow = Instant::now();
                dirty = true;
            }

            if self.quit {
                self.cfg.ui.last_tab = self.tab.index();
                let _ = self.cfg.save();
                break;
            }
        }
        Ok(())
    }

    fn handle_key(&mut self, mut key: KeyEvent) -> Result<()> {
        key = self.translate_keypad(key);

        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.quit = true;
            return Ok(());
        }

        if self.modal.is_some() {
            return self.handle_modal_key(key);
        }

        if self.ip_form.is_some() {
            return self.handle_ip_form_key(key);
        }

        if self.cache_open {
            return self.handle_cache_key(key);
        }

        match key.code {
            KeyCode::Char('q') => {
                self.confirm("退出", "退出 rsztui？", Pending::Quit);
            }
            KeyCode::Char('r') => {
                self.deps = deps::check();
                self.hw = hw::collect();
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
            KeyCode::Char(c @ '1'..='8') => {
                let index = c as usize - '1' as usize;
                self.switch_tab(Tab::from_index(index));
            }
            _ => self.handle_tab_key(key)?,
        }

        Ok(())
    }

    /// 数字小键盘：NumLock 关闭时终端本来就发方向键；
    /// NumLock 开启时依靠 Kitty 协议的 KEYPAD 标记，把 8/2/4/6/5 映射成方向/确认。
    /// 文本输入（IP 表单、密码）不映射，保证能输入数字。
    fn translate_keypad(&self, key: KeyEvent) -> KeyEvent {
        if !key.state.contains(KeyEventState::KEYPAD) {
            return key;
        }
        let editing = matches!(self.modal, Some(Modal::Input { .. })) || self.ip_form.is_some();
        if editing {
            return key;
        }
        let code = match key.code {
            KeyCode::Char('8') => KeyCode::Up,
            KeyCode::Char('2') => KeyCode::Down,
            KeyCode::Char('4') => KeyCode::Left,
            KeyCode::Char('6') => KeyCode::Right,
            KeyCode::Char('5') => KeyCode::Enter,
            _ => return key,
        };
        KeyEvent {
            code,
            modifiers: key.modifiers,
            kind: key.kind,
            state: key.state,
        }
    }

    fn handle_mouse(&mut self, mouse: MouseEvent) -> Result<()> {
        if self.modal.is_some() {
            if let MouseEventKind::Down(MouseButton::Left) = mouse.kind {
                let action = {
                    let targets = self.mouse_targets.borrow();
                    targets
                        .iter()
                        .rev()
                        .find(|target| point_in(target.rect, mouse.column, mouse.row))
                        .map(|target| target.action)
                };
                match action {
                    Some(MouseAction::ModalConfirm) => self.modal_confirm()?,
                    Some(MouseAction::ModalCancel) => self.modal = None,
                    _ => {}
                }
            }
            return Ok(());
        }

        match mouse.kind {
            MouseEventKind::Moved => {
                self.hover = self.mouse_action_at(mouse.column, mouse.row);
            }
            MouseEventKind::Down(MouseButton::Left) => {
                let action = self.mouse_action_at(mouse.column, mouse.row);
                self.hover = action;
                if let Some(action) = action {
                    let now = Instant::now();
                    let double = self.last_mouse.as_ref().is_some_and(|(time, last)| {
                        *last == action && now.duration_since(*time) < Duration::from_millis(450)
                    });
                    self.last_mouse = Some((now, action));
                    self.activate_mouse(action, double)?;
                }
            }
            MouseEventKind::ScrollUp => self.scroll(-1)?,
            MouseEventKind::ScrollDown => self.scroll(1)?,
            _ => {}
        }
        Ok(())
    }

    fn mouse_action_at(&self, x: u16, y: u16) -> Option<MouseAction> {
        let targets = self.mouse_targets.borrow();
        targets
            .iter()
            .rev()
            .find(|target| point_in(target.rect, x, y))
            .map(|target| target.action)
    }

    fn scroll(&mut self, direction: i32) -> Result<()> {
        if self.ip_form.is_some() {
            return Ok(());
        }
        let code = if direction < 0 {
            KeyCode::Up
        } else {
            KeyCode::Down
        };
        self.handle_tab_key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn activate_mouse(&mut self, action: MouseAction, double: bool) -> Result<()> {
        match action {
            MouseAction::Tab(tab) => self.switch_tab(tab),
            MouseAction::Profile(index) => {
                self.perf_focus = PerfFocus::Profiles;
                self.profile_sel = index.min(Profile::ALL.len().saturating_sub(1));
                if double {
                    let profile = Profile::ALL[self.profile_sel];
                    self.confirm(
                        "切换性能模式",
                        format!("切换到{}模式？", profile.zh()),
                        Pending::Profile(profile),
                    );
                }
            }
            MouseAction::PerfPreset(index) => {
                self.preset_sel = index.min(self.cfg.power_presets.len().saturating_sub(1));
                if double && let Some(preset) = self.cfg.preset(self.preset_sel) {
                    self.confirm(
                        "套用功耗预设",
                        format!(
                            "套用「{}」预设到 {} 模式？",
                            preset.name,
                            preset.profile.zh()
                        ),
                        Pending::Preset(self.preset_sel),
                    );
                }
            }
            MouseAction::PowerField(index) => {
                self.perf_focus = PerfFocus::Power;
                self.power_sel = index.min(self.power.len().saturating_sub(1));
            }
            MouseAction::FanPreset(index) => {
                self.fan_sel = index.min(self.cfg.fan_presets.len().saturating_sub(1));
                if double && let Some(preset) = self.cfg.fan_presets.get(self.fan_sel).cloned() {
                    self.confirm(
                        "套用风扇预设",
                        format!("把「{}」应用到 {} 模式？", preset.name, preset.profile.zh()),
                        Pending::FanApply(self.fan_sel),
                    );
                }
            }
            MouseAction::FanPoint(index) => {
                if let Some(edit) = self.fan_edit.as_mut()
                    && index < edit.temp.len()
                {
                    edit.sel = index;
                }
            }
            MouseAction::Net(index) => {
                self.net_sel = index.min(self.net_len().saturating_sub(1));
                self.refresh_net_detail();
                if double {
                    self.net_enter()?;
                }
            }
            MouseAction::Service(index) => {
                self.service_sel = index.min(self.services.len().saturating_sub(1));
            }
            MouseAction::Gpu(index) => {
                self.gpu_sel = index.min(GpuMode::ALL.len().saturating_sub(1));
                if double {
                    let mode = GpuMode::ALL[self.gpu_sel];
                    self.confirm(
                        "切换 GPU 模式",
                        format!("将 GPU 模式设为{}？修改后需要重启。", mode.zh()),
                        Pending::Gpu(mode),
                    );
                }
            }
            MouseAction::Color(index) => {
                self.aura_color = index.min(AURA_COLORS.len().saturating_sub(1));
                if double {
                    let (name, hex) = AURA_COLORS[self.aura_color];
                    asus::aura_static(hex)?;
                    self.refresh_current_tab();
                    self.status = format!("灯效：静态 {name}");
                    self.status_err = false;
                }
            }
            MouseAction::CacheFlushDns => self.confirm(
                "清空 DNS 缓存",
                "清空 systemd-resolved 的 DNS 缓存？",
                Pending::FlushDns,
            ),
            MouseAction::CacheFlushNeighbors => self.confirm(
                "清空邻居缓存",
                "清空内核 ARP/邻居缓存？",
                Pending::FlushNeighbors,
            ),
            MouseAction::CacheEditSize => {
                self.modal = Some(Modal::Input {
                    title: "DNS 缓存上限".into(),
                    prompt: "CacheSize（条，0-65536；重启 resolved 会短暂中断 DNS）".into(),
                    value: "4096".into(),
                    masked: false,
                    action: InputAction::DnsCacheSize,
                });
            }
            MouseAction::CacheRefresh => {
                self.cache_stats = nm::cache_stats().ok();
            }
            MouseAction::CacheClose => {
                self.cache_open = false;
            }
            MouseAction::NetPing => self.open_ping_input(),
            MouseAction::NetDns => self.open_dns_input(),
            MouseAction::NetCache => self.open_cache(),
            MouseAction::NetRefresh => {
                self.request_net_snapshot();
                self.status = "正在刷新网络…".into();
                self.status_err = false;
            }
            MouseAction::Key(code, modifiers) => {
                self.handle_key(KeyEvent::new(code, modifiers))?;
            }
            MouseAction::PerfSwitch => {
                self.perf_focus = PerfFocus::Profiles;
                self.handle_tab_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))?;
            }
            MouseAction::PerfAc => {
                self.perf_focus = PerfFocus::Profiles;
                self.handle_tab_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE))?;
            }
            MouseAction::PerfBat => {
                self.perf_focus = PerfFocus::Profiles;
                self.handle_tab_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE))?;
            }
            MouseAction::PerfPowerApply => {
                self.perf_focus = PerfFocus::Power;
                self.handle_tab_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))?;
            }
            MouseAction::PerfPresetApply => {
                self.perf_focus = PerfFocus::Power;
                self.handle_tab_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE))?;
            }
            MouseAction::ModalConfirm | MouseAction::ModalCancel => {}
        }
        Ok(())
    }

    fn modal_confirm(&mut self) -> Result<()> {
        match self.modal.take() {
            Some(Modal::Confirm { pending, .. }) => self.apply_pending(pending)?,
            Some(Modal::Input { value, action, .. }) => self.submit_input(action, value)?,
            Some(Modal::Message { .. }) | None => {}
        }
        Ok(())
    }

    fn handle_modal_key(&mut self, key: KeyEvent) -> Result<()> {
        if let Some(Modal::Input { value, .. }) = &mut self.modal {
            match key.code {
                KeyCode::Esc => self.modal = None,
                KeyCode::Backspace => {
                    value.pop();
                }
                KeyCode::Enter => {
                    if let Some(Modal::Input { value, action, .. }) = self.modal.take() {
                        self.submit_input(action, value)?;
                    }
                }
                KeyCode::Char(c) if !c.is_control() => value.push(c),
                _ => {}
            }
            return Ok(());
        }

        match key.code {
            KeyCode::Esc => {
                self.modal = None;
            }
            KeyCode::Enter => self.modal_confirm()?,
            _ => {}
        }
        Ok(())
    }

    fn submit_input(&mut self, action: InputAction, value: String) -> Result<()> {
        if value.trim().is_empty() {
            bail!("输入不能为空");
        }
        match action {
            InputAction::Wifi { ssid, key_mgmt } => {
                nm::wifi_connect(&ssid, Some(&value), &key_mgmt, true)?;
                self.status = format!("已连接 Wi-Fi：{ssid}");
            }
            InputAction::Hotspot => {
                nm::hotspot(&value, "rsztui-hotspot")?;
                self.status = "热点已开启：rsztui-hotspot".into();
            }
            InputAction::Ping => {
                let output = nm::ping(&value)?;
                self.modal = Some(Modal::Message {
                    title: format!("Ping {value}"),
                    body: output,
                });
            }
            InputAction::DnsLookup => {
                let output = nm::dns_query(&value)?;
                self.modal = Some(Modal::Message {
                    title: format!("DNS 查询 {value}"),
                    body: output,
                });
            }
            InputAction::DnsCacheSize => {
                let size: u32 = value
                    .trim()
                    .parse()
                    .map_err(|_| anyhow::anyhow!("缓存大小必须是数字"))?;
                nm::set_dns_cache_size(size)?;
                self.cache_stats = nm::cache_stats().ok();
                self.status = format!("DNS 缓存上限已设为 {size} 条");
            }
        }
        self.status_err = false;
        self.refresh_slow();
        Ok(())
    }

    fn switch_tab(&mut self, tab: Tab) {
        self.tab = tab;
        self.cfg.ui.last_tab = tab.index();
        let _ = self.cfg.save();
        if tab == Tab::Settings {
            self.deps = deps::check();
        }
        self.refresh_current_tab();
    }

    fn handle_tab_key(&mut self, key: KeyEvent) -> Result<()> {
        match self.tab {
            Tab::Performance => self.handle_performance_key(key)?,
            Tab::Battery => self.handle_battery_key(key)?,
            Tab::Fan => self.handle_fan_key(key)?,
            Tab::Lighting => self.handle_lighting_key(key)?,
            Tab::Network => self.handle_network_key(key)?,
            Tab::Services => self.handle_service_key(key)?,
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
                    KeyCode::Home => self.profile_sel = 0,
                    KeyCode::End => self.profile_sel = Profile::ALL.len() - 1,
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
                    KeyCode::Home => self.power_sel = 0,
                    KeyCode::End => self.power_sel = self.power.len().saturating_sub(1),
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
            KeyCode::Home => {
                self.limit_edit = 20;
                self.limit_dirty = true;
            }
            KeyCode::End => {
                self.limit_edit = 100;
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

    fn handle_fan_key(&mut self, key: KeyEvent) -> Result<()> {
        if self.fan_edit.is_some() {
            return self.handle_fan_edit_key(key);
        }

        let len = self.cfg.fan_presets.len();
        match key.code {
            KeyCode::Char('i') => self.open_fan_edit("cpu")?,
            KeyCode::Char('I') => self.open_fan_edit("gpu")?,
            KeyCode::Down | KeyCode::Char('j') if len > 0 => {
                self.fan_sel = (self.fan_sel + 1) % len;
            }
            KeyCode::Up | KeyCode::Char('k') if len > 0 => {
                self.fan_sel = (self.fan_sel + len - 1) % len;
            }
            KeyCode::Home if len > 0 => self.fan_sel = 0,
            KeyCode::End if len > 0 => self.fan_sel = len - 1,
            KeyCode::PageUp if len > 0 => self.fan_sel = self.fan_sel.saturating_sub(5),
            KeyCode::PageDown if len > 0 => self.fan_sel = (self.fan_sel + 5).min(len - 1),
            KeyCode::Char('p') if len > 0 => {
                let preset = self.cfg.fan_presets[self.fan_sel].clone();
                self.confirm(
                    "套用风扇预设",
                    format!(
                        "把「{}」应用到 {} 模式？\n\nCPU: {}\nGPU: {}",
                        preset.name,
                        preset.profile.zh(),
                        pretty_fan_data(&preset.cpu),
                        pretty_fan_data(&preset.gpu)
                    ),
                    Pending::FanApply(self.fan_sel),
                );
            }
            KeyCode::Char('s') => {
                self.confirm(
                    "保存风扇预设",
                    "把当前曲线的百分比值保存为预设？",
                    Pending::FanSave,
                );
            }
            KeyCode::Char('x') if len > 0 => {
                let preset = self.cfg.fan_presets[self.fan_sel].clone();
                self.confirm(
                    "删除风扇预设",
                    format!("删除「{}」？", preset.name),
                    Pending::FanDelete(self.fan_sel),
                );
            }
            KeyCode::Char('d') => {
                self.confirm(
                    "恢复默认风扇",
                    "把当前性能模式的风扇曲线恢复为固件默认？",
                    Pending::FanDefault,
                );
            }
            KeyCode::Char('e') => {
                let enabled = !self.fan_curves.iter().any(|c| c.enabled);
                self.confirm(
                    "风扇曲线开关",
                    format!(
                        "{}当前性能模式的风扇曲线？",
                        if enabled { "启用" } else { "停用" }
                    ),
                    Pending::FanToggle(enabled),
                );
            }
            _ => {}
        }
        Ok(())
    }

    fn open_fan_edit(&mut self, fan: &str) -> Result<()> {
        if self.fan_curves.is_empty() {
            let profile = self.profile.map(|p| p.active).unwrap_or(Profile::Balanced);
            self.fan_curves = asus::fan_curves(profile)?;
        }
        let Some(curve) = self
            .fan_curves
            .iter()
            .find(|c| c.fan.eq_ignore_ascii_case(fan))
        else {
            bail!("没有找到 {fan} 风扇曲线");
        };
        let Some(edit) = FanEdit::from_curve(curve, fan) else {
            bail!("{fan} 曲线数据不完整");
        };
        self.fan_edit = Some(edit);
        self.status = format!(
            "编辑 {} 曲线：j/k 选点，h/l 调 PWM，[ ] 调温度，a 写入",
            fan.to_uppercase()
        );
        self.status_err = false;
        Ok(())
    }

    fn handle_fan_edit_key(&mut self, key: KeyEvent) -> Result<()> {
        let mut apply = None;
        let mut close = false;
        let mut switch = None;

        if let Some(edit) = self.fan_edit.as_mut() {
            match key.code {
                KeyCode::Esc => close = true,
                KeyCode::Down | KeyCode::Char('j') => {
                    if !edit.temp.is_empty() {
                        edit.sel = (edit.sel + 1) % edit.temp.len();
                    }
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    if !edit.temp.is_empty() {
                        edit.sel = (edit.sel + edit.temp.len() - 1) % edit.temp.len();
                    }
                }
                KeyCode::Home => edit.sel = 0,
                KeyCode::End => edit.sel = edit.temp.len().saturating_sub(1),
                KeyCode::PageUp => edit.sel = edit.sel.saturating_sub(5),
                KeyCode::PageDown => {
                    edit.sel = (edit.sel + 5).min(edit.temp.len().saturating_sub(1));
                }
                KeyCode::Left | KeyCode::Char('h') => edit.adjust_pwm(-1),
                KeyCode::Right | KeyCode::Char('l') => edit.adjust_pwm(1),
                KeyCode::Char('H') => edit.adjust_pwm(-10),
                KeyCode::Char('L') => edit.adjust_pwm(10),
                KeyCode::Char('[') => edit.adjust_temp(-1),
                KeyCode::Char(']') => edit.adjust_temp(1),
                KeyCode::Char('c') => {
                    switch = Some(if edit.fan.eq_ignore_ascii_case("cpu") {
                        "gpu"
                    } else {
                        "cpu"
                    });
                }
                KeyCode::Char('a') => apply = Some(edit.clone()),
                _ => {}
            }
        }

        if let Some(next) = switch {
            self.open_fan_edit(next)?;
            self.status = format!(
                "已切换到 {} 曲线（未保存的修改已丢弃）",
                next.to_uppercase()
            );
        }

        if close {
            if self.fan_edit.as_ref().is_some_and(|e| e.dirty) {
                self.confirm(
                    "放弃修改",
                    "风扇曲线有未写入的修改，确定放弃？",
                    Pending::FanEditClose,
                );
            } else {
                self.fan_edit = None;
            }
        }

        if let Some(edit) = apply {
            self.confirm(
                "写入风扇曲线",
                format!(
                    "把 {} 曲线写入 {} 模式？\n\n{}",
                    edit.fan.to_uppercase(),
                    self.profile.map(|p| p.active.zh()).unwrap_or("当前"),
                    pretty_fan_data(&edit.data())
                ),
                Pending::FanCurveApply(edit),
            );
        }

        Ok(())
    }

    fn handle_service_key(&mut self, key: KeyEvent) -> Result<()> {
        let Some(info) = self.services.get(self.service_sel).cloned() else {
            return Ok(());
        };

        let action = match key.code {
            KeyCode::Down | KeyCode::Char('j') => {
                if !self.services.is_empty() {
                    self.service_sel = (self.service_sel + 1) % self.services.len();
                }
                return Ok(());
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if !self.services.is_empty() {
                    self.service_sel =
                        (self.service_sel + self.services.len() - 1) % self.services.len();
                }
                return Ok(());
            }
            KeyCode::Home => {
                self.service_sel = 0;
                return Ok(());
            }
            KeyCode::End => {
                self.service_sel = self.services.len().saturating_sub(1);
                return Ok(());
            }
            KeyCode::PageUp => {
                self.service_sel = self.service_sel.saturating_sub(5);
                return Ok(());
            }
            KeyCode::PageDown => {
                self.service_sel =
                    (self.service_sel + 5).min(self.services.len().saturating_sub(1));
                return Ok(());
            }
            KeyCode::Char('s') => crate::services::ServiceAction::Start,
            KeyCode::Char('t') => crate::services::ServiceAction::Stop,
            KeyCode::Char('R') => crate::services::ServiceAction::Restart,
            KeyCode::Char('e') => crate::services::ServiceAction::Enable,
            KeyCode::Char('d') => crate::services::ServiceAction::Disable,
            KeyCode::Char('E') => crate::services::ServiceAction::EnableNow,
            KeyCode::Char('D') => crate::services::ServiceAction::DisableNow,
            _ => return Ok(()),
        };

        let impact = match (info.unit.as_str(), action) {
            ("NetworkManager.service", crate::services::ServiceAction::Stop)
            | ("NetworkManager.service", crate::services::ServiceAction::Restart)
            | ("NetworkManager.service", crate::services::ServiceAction::DisableNow) => {
                "\n\n注意：会短暂或完全断开网络连接。"
            }
            ("asusd.service", crate::services::ServiceAction::Stop)
            | ("asusd.service", crate::services::ServiceAction::DisableNow) => {
                "\n\n注意：停止后性能模式、电池、风扇和灯光控制都会不可用。"
            }
            ("systemd-resolved.service", crate::services::ServiceAction::Stop)
            | ("systemd-resolved.service", crate::services::ServiceAction::DisableNow) => {
                "\n\n注意：DNS 解析可能失效，DoT 也会受影响。"
            }
            _ => "",
        };

        self.confirm(
            "服务操作",
            format!("{} {}？{impact}", action.label(), info.unit),
            Pending::Service {
                unit: info.unit,
                action,
            },
        );
        Ok(())
    }

    fn handle_lighting_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Char('b') => {
                asus::led_next()?;
                self.refresh_current_tab();
                self.status = "键盘亮度已切换".into();
                self.status_err = false;
            }
            KeyCode::Char('e') => {
                asus::aura_next()?;
                self.refresh_current_tab();
                self.status = "已切换到下一个灯效".into();
                self.status_err = false;
            }
            KeyCode::Char('E') => {
                asus::aura_prev()?;
                self.refresh_current_tab();
                self.status = "已切换到上一个灯效".into();
                self.status_err = false;
            }
            KeyCode::Char('c') => {
                self.aura_color = (self.aura_color + 1) % AURA_COLORS.len();
            }
            KeyCode::Char('S') => {
                let (name, hex) = AURA_COLORS[self.aura_color % AURA_COLORS.len()];
                asus::aura_static(hex)?;
                self.refresh_current_tab();
                self.status = format!("灯效：静态 {name}");
                self.status_err = false;
            }
            _ => {}
        }
        Ok(())
    }

    pub fn net_len(&self) -> usize {
        match self.net_view {
            NetView::Connections => self.connections.len(),
            NetView::Wifi => self.wifi.len(),
            NetView::Vpn => self.vpns.len(),
        }
    }

    fn net_move(&mut self, delta: isize) {
        let len = self.net_len();
        if len == 0 {
            self.net_sel = 0;
            self.refresh_net_detail();
            return;
        }
        let len = len as isize;
        self.net_sel = ((self.net_sel as isize + delta).rem_euclid(len)) as usize;
        self.refresh_net_detail();
    }

    fn refresh_net_detail(&mut self) {
        if self.net_view == NetView::Connections {
            self.net_ipv4 = self
                .connections
                .get(self.net_sel)
                .and_then(|c| nm::connection_ipv4(&c.name).ok());

            let device = self
                .connections
                .get(self.net_sel)
                .map(|c| c.device.clone())
                .unwrap_or_default();
            let cached = self
                .net_detail
                .as_ref()
                .is_some_and(|detail| detail.device == device);
            if !device.is_empty()
                && !(cached && self.net_detail_at.elapsed() < Duration::from_secs(10))
            {
                self.net_detail = nm::device_detail(&device).ok();
                self.net_detail_at = Instant::now();
            } else if device.is_empty() {
                self.net_detail = None;
            }
        } else {
            self.net_ipv4 = None;
            self.net_detail = None;
        }
    }

    fn rescan_wifi(&mut self, rescan: bool) -> Result<()> {
        self.net_force_scan |= rescan;
        self.request_net_snapshot();
        self.status = if rescan {
            "正在扫描 Wi-Fi…".into()
        } else {
            "正在刷新网络…".into()
        };
        self.status_err = false;
        Ok(())
    }

    fn request_net_snapshot(&mut self) {
        if self.net_loading {
            self.net_refresh_again = true;
            return;
        }
        self.net_loading = true;
        let include_wifi = self.tab == Tab::Network && self.net_view == NetView::Wifi;
        let force_scan = self.net_force_scan;
        self.net_force_scan = false;
        let tx = self.net_tx.clone();
        std::thread::spawn(move || {
            let result =
                nm::network_snapshot(include_wifi, force_scan).map_err(|err| format!("{err:#}"));
            let _ = tx.send(result);
        });
    }

    fn poll_net(&mut self) -> bool {
        let mut updated = false;
        loop {
            match self.net_rx.try_recv() {
                Ok(Ok(snapshot)) => {
                    self.net_loading = false;
                    self.devices = snapshot.devices;
                    self.connections = snapshot.connections;
                    self.vpns = snapshot.vpns;
                    self.wifi_enabled = snapshot.wifi_enabled;
                    if snapshot.wifi_scanned {
                        self.wifi = snapshot.wifi;
                        self.ssid = snapshot.ssid;
                    }
                    self.net_sel = self.net_sel.min(self.net_len().saturating_sub(1));
                    self.refresh_net_detail();
                    updated = true;
                }
                Ok(Err(err)) => {
                    self.net_loading = false;
                    self.status = format!("网络刷新失败：{err}");
                    self.status_err = true;
                    updated = true;
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => break,
            }
        }
        if self.net_refresh_again {
            self.net_refresh_again = false;
            self.request_net_snapshot();
        }
        updated
    }

    fn selected_connection(&self) -> Option<nm::ConnectionInfo> {
        if self.net_view != NetView::Connections {
            return None;
        }
        self.connections.get(self.net_sel).cloned()
    }

    fn handle_network_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Char('v') => {
                self.net_view = self.net_view.next();
                self.net_sel = 0;
                self.status = format!("网络视图：{}", self.net_view.zh());
                self.status_err = false;
                self.refresh_net_detail();
                if self.net_view == NetView::Wifi {
                    self.rescan_wifi(true)?;
                }
            }
            KeyCode::Char('s') => {
                self.refresh_net_detail();
                if self.net_view == NetView::Wifi {
                    self.rescan_wifi(true)?;
                } else {
                    self.refresh_slow();
                    self.status = "已刷新".into();
                    self.status_err = false;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => self.net_move(1),
            KeyCode::Up | KeyCode::Char('k') => self.net_move(-1),
            KeyCode::Home => {
                self.net_sel = 0;
                self.refresh_net_detail();
            }
            KeyCode::End => {
                self.net_sel = self.net_len().saturating_sub(1);
                self.refresh_net_detail();
            }
            KeyCode::PageUp => {
                for _ in 0..5 {
                    self.net_move(-1);
                }
            }
            KeyCode::PageDown => {
                for _ in 0..5 {
                    self.net_move(1);
                }
            }
            KeyCode::Char('u') => self.net_up()?,
            KeyCode::Char('d') => self.net_down()?,
            KeyCode::Char('x') => self.net_delete(),
            KeyCode::Char('o') => {
                let on = !self.wifi_enabled.unwrap_or(false);
                self.confirm(
                    "无线开关",
                    format!("{} Wi-Fi？", if on { "启用" } else { "禁用" }),
                    Pending::Radio(on),
                );
            }
            KeyCode::Char('t') => self.toggle_hotspot(),
            KeyCode::Char('c') => self.open_cache(),
            KeyCode::Char('p') => self.open_ping_input(),
            KeyCode::Char('n') => self.open_dns_input(),
            KeyCode::Enter => self.net_enter()?,
            _ => {}
        }
        Ok(())
    }

    fn net_up(&mut self) -> Result<()> {
        match self.net_view {
            NetView::Connections => {
                if let Some(conn) = self.selected_connection() {
                    nm::connection_up(&conn.name)?;
                    self.status = format!("已激活 {}", conn.name);
                    self.refresh_slow();
                }
            }
            NetView::Vpn => {
                if let Some(vpn) = self.vpns.get(self.net_sel).cloned() {
                    nm::connection_up(&vpn.name)?;
                    self.status = format!("VPN 已连接：{}", vpn.name);
                    self.refresh_slow();
                }
            }
            NetView::Wifi => self.rescan_wifi(true)?,
        }
        self.status_err = false;
        Ok(())
    }

    fn net_down(&mut self) -> Result<()> {
        match self.net_view {
            NetView::Connections => {
                if let Some(conn) = self.selected_connection() {
                    nm::connection_down(&conn.name)?;
                    self.status = format!("已断开 {}", conn.name);
                    self.refresh_slow();
                }
            }
            NetView::Vpn => {
                if let Some(vpn) = self.vpns.get(self.net_sel).cloned() {
                    nm::connection_down(&vpn.name)?;
                    self.status = format!("VPN 已断开：{}", vpn.name);
                    self.refresh_slow();
                }
            }
            NetView::Wifi => {}
        }
        self.status_err = false;
        Ok(())
    }

    fn net_delete(&mut self) {
        let target = match self.net_view {
            NetView::Connections => self.connections.get(self.net_sel).cloned(),
            NetView::Vpn => self.vpns.get(self.net_sel).cloned(),
            NetView::Wifi => None,
        };
        if let Some(target) = target {
            self.confirm(
                "删除连接",
                format!("删除连接「{}」？此操作不可撤销。", target.name),
                Pending::NetDelete(target.name),
            );
        }
    }

    fn net_enter(&mut self) -> Result<()> {
        match self.net_view {
            NetView::Connections => self.open_ip_form(),
            NetView::Wifi => self.connect_wifi_selected(),
            NetView::Vpn => self.net_up(),
        }
    }

    fn open_ip_form(&mut self) -> Result<()> {
        let Some(conn) = self.selected_connection() else {
            bail!("没有选中连接");
        };
        let settings = nm::connection_ipv4(&conn.name)?;
        self.ip_form = Some(IpForm {
            conn: conn.name,
            method: settings.method,
            address: settings.addresses,
            gateway: settings.gateway,
            dns: settings.dns,
            dot: settings.dot,
            sel: 0,
        });
        self.status = "编辑 IPv4：j/k 选择，直接输入，a 应用，Esc 取消".into();
        self.status_err = false;
        Ok(())
    }

    fn connect_wifi_selected(&mut self) -> Result<()> {
        let Some(ap) = self.wifi.get(self.net_sel).cloned() else {
            bail!("没有选中 Wi-Fi");
        };
        if ap.in_use {
            self.status = format!("{} 已连接", ap.ssid);
            self.status_err = false;
            return Ok(());
        }
        if let Some(existing) = nm::wifi_profile(&ap.ssid)? {
            nm::connection_up(&existing.name)?;
            self.status = format!("已连接 Wi-Fi：{}", ap.ssid);
            self.status_err = false;
            self.refresh_slow();
            return Ok(());
        }

        if ap.security.trim().is_empty() || ap.security.trim() == "--" {
            nm::wifi_connect(&ap.ssid, None, "", true)?;
            self.status = format!("已连接开放网络：{}", ap.ssid);
            self.status_err = false;
            self.refresh_slow();
            return Ok(());
        }
        let key_mgmt = if ap.security.contains("WPA3") && !ap.security.contains("WPA2") {
            "sae"
        } else {
            "wpa-psk"
        };
        self.modal = Some(Modal::Input {
            title: "连接 Wi-Fi".into(),
            prompt: format!("{}（{}）的密码", ap.ssid, ap.security),
            value: String::new(),
            masked: true,
            action: InputAction::Wifi {
                ssid: ap.ssid,
                key_mgmt: key_mgmt.into(),
            },
        });
        Ok(())
    }

    fn open_ping_input(&mut self) {
        let target = self
            .net_detail
            .as_ref()
            .map(|d| d.gateway.clone())
            .filter(|v| !v.is_empty())
            .or_else(|| {
                self.cache_stats
                    .as_ref()
                    .and_then(|c| c.dns_servers.first().cloned())
            })
            .unwrap_or_else(|| "1.1.1.1".into());
        self.modal = Some(Modal::Input {
            title: "Ping 测试".into(),
            prompt: "目标 IP / 域名".into(),
            value: target,
            masked: false,
            action: InputAction::Ping,
        });
    }

    fn open_dns_input(&mut self) {
        self.modal = Some(Modal::Input {
            title: "DNS 查询".into(),
            prompt: "域名".into(),
            value: "example.com".into(),
            masked: false,
            action: InputAction::DnsLookup,
        });
    }

    fn open_cache(&mut self) {
        self.cache_stats = nm::cache_stats().ok();
        self.cache_open = true;
        self.status = "网络缓存：f 清空 DNS，F 清空邻居，e 设置上限".into();
        self.status_err = false;
    }

    fn handle_cache_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc => self.cache_open = false,
            KeyCode::Char('f') => self.confirm(
                "清空 DNS 缓存",
                "清空 systemd-resolved 的 DNS 缓存？",
                Pending::FlushDns,
            ),
            KeyCode::Char('F') => self.confirm(
                "清空邻居缓存",
                "清空内核 ARP/邻居缓存？",
                Pending::FlushNeighbors,
            ),
            KeyCode::Char('e') => {
                self.modal = Some(Modal::Input {
                    title: "DNS 缓存上限".into(),
                    prompt: "CacheSize（条，0-65536；重启 resolved 会短暂中断 DNS）".into(),
                    value: "4096".into(),
                    masked: false,
                    action: InputAction::DnsCacheSize,
                });
            }
            KeyCode::Char('r') => self.cache_stats = nm::cache_stats().ok(),
            _ => {}
        }
        Ok(())
    }

    fn toggle_hotspot(&mut self) {
        if nm::hotspot_active() {
            self.confirm("关闭热点", "关闭 rsztui-hotspot？", Pending::HotspotOff);
        } else {
            self.modal = Some(Modal::Input {
                title: "开启热点".into(),
                prompt: "热点 SSID：rsztui-hotspot，密码（可改）".into(),
                value: random_password(),
                masked: false,
                action: InputAction::Hotspot,
            });
        }
    }

    fn handle_ip_form_key(&mut self, key: KeyEvent) -> Result<()> {
        let mut apply = None;
        let mut close = false;

        if let Some(form) = self.ip_form.as_mut() {
            match key.code {
                KeyCode::Esc => close = true,
                KeyCode::Down | KeyCode::Char('j') => {
                    form.sel = (form.sel + 1) % IpForm::LABELS.len();
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    form.sel = (form.sel + IpForm::LABELS.len() - 1) % IpForm::LABELS.len();
                }
                KeyCode::Enter | KeyCode::Char(' ') => match form.sel {
                    0 => {
                        form.method = if form.method == "manual" {
                            "auto".into()
                        } else {
                            "manual".into()
                        };
                    }
                    4 => {
                        form.dot = match form.dot {
                            -1 => 0,
                            0 => 1,
                            1 => 2,
                            _ => -1,
                        };
                    }
                    1..=3 => {
                        form.sel = (form.sel + 1) % IpForm::LABELS.len();
                    }
                    _ => {}
                },
                KeyCode::Backspace => {
                    if form.editable(form.sel) {
                        match form.sel {
                            1 => {
                                form.address.pop();
                            }
                            2 => {
                                form.gateway.pop();
                            }
                            3 => {
                                form.dns.pop();
                            }
                            _ => {}
                        }
                    }
                }
                KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    if form.editable(form.sel) {
                        match form.sel {
                            1 => form.address.clear(),
                            2 => form.gateway.clear(),
                            3 => form.dns.clear(),
                            _ => {}
                        }
                    }
                }
                KeyCode::Char('a') => apply = Some(form.clone()),
                KeyCode::Char(c)
                    if form.editable(form.sel)
                        && (c.is_ascii_digit() || matches!(c, '.' | '/' | ',' | ' ')) =>
                {
                    match form.sel {
                        1 => form.address.push(c),
                        2 => form.gateway.push(c),
                        3 => form.dns.push(c),
                        _ => {}
                    }
                }
                _ => {}
            }
        }

        if close {
            self.ip_form = None;
        }
        if let Some(form) = apply {
            self.confirm(
                "应用网络设置",
                ip_form_summary(&form),
                Pending::NetApply(form),
            );
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
            Pending::NetApply(form) => {
                let settings = nm::Ipv4Settings {
                    method: form.method.clone(),
                    addresses: form.address.clone(),
                    gateway: form.gateway.clone(),
                    dns: form.dns.clone(),
                    dot: form.dot,
                };
                nm::apply_ipv4(&form.conn, &settings)?;
                self.ip_form = None;
                self.status = format!("{} 的 IPv4 设置已应用", form.conn);
            }
            Pending::NetDelete(name) => {
                nm::connection_delete(&name)?;
                self.status = format!("已删除连接：{name}");
            }
            Pending::Radio(on) => {
                nm::set_radio(on)?;
                self.status = if on {
                    "Wi-Fi 已启用"
                } else {
                    "Wi-Fi 已禁用"
                }
                .into();
            }
            Pending::HotspotOff => {
                nm::hotspot_off()?;
                self.status = "热点已关闭".into();
            }
            Pending::FanApply(index) => {
                let preset = self
                    .cfg
                    .fan_presets
                    .get(index)
                    .cloned()
                    .ok_or_else(|| anyhow!("没有这个风扇预设"))?;
                asus::fan_apply(preset.profile, &preset.cpu, &preset.gpu)?;
                self.status = format!("已套用风扇预设：{}", preset.name);
            }
            Pending::FanSave => {
                let profile = self.profile.map(|p| p.active).unwrap_or(Profile::Balanced);
                let Some((cpu, gpu)) = asus::fan_data(&self.fan_curves) else {
                    bail!("没有读到完整的 CPU/GPU 风扇曲线");
                };
                let name = format!("我的风扇 {}", profile.zh());
                let preset = FanPreset {
                    name: name.clone(),
                    profile,
                    cpu,
                    gpu,
                };
                if let Some(existing) = self.cfg.fan_presets.iter_mut().find(|p| p.name == name) {
                    *existing = preset;
                } else {
                    self.cfg.fan_presets.push(preset);
                }
                self.cfg.save()?;
                self.fan_sel = self
                    .fan_sel
                    .min(self.cfg.fan_presets.len().saturating_sub(1));
                self.status = format!("已保存风扇预设：{name}");
            }
            Pending::FanDelete(index) => {
                if index < self.cfg.fan_presets.len() {
                    let removed = self.cfg.fan_presets.remove(index);
                    self.cfg.save()?;
                    self.fan_sel = self
                        .fan_sel
                        .min(self.cfg.fan_presets.len().saturating_sub(1));
                    self.status = format!("已删除风扇预设：{}", removed.name);
                }
            }
            Pending::FanDefault => {
                let profile = self.profile.map(|p| p.active).unwrap_or(Profile::Balanced);
                asus::fan_default(profile)?;
                self.status = format!("{} 的风扇曲线已恢复默认", profile.zh());
            }
            Pending::FanCurveApply(edit) => {
                let profile = self.profile.map(|p| p.active).unwrap_or(Profile::Balanced);
                asus::fan_set_curve(profile, &edit.fan, &edit.data())?;
                if let Some(current) = self.fan_edit.as_mut() {
                    current.dirty = false;
                }
                self.status = format!("{} 风扇曲线已写入", edit.fan.to_uppercase());
            }
            Pending::FanEditClose => {
                self.fan_edit = None;
                self.status = "已放弃风扇曲线修改".into();
            }
            Pending::FlushDns => {
                nm::flush_dns_cache()?;
                self.cache_stats = nm::cache_stats().ok();
                self.status = "DNS 缓存已清空".into();
            }
            Pending::FlushNeighbors => {
                nm::flush_neighbor_cache()?;
                self.cache_stats = nm::cache_stats().ok();
                self.status = "邻居缓存已清空".into();
            }
            Pending::Service { unit, action } => {
                crate::services::apply(&unit, action)?;
                self.deps = deps::check();
                self.status = format!("{} {}", action.label(), unit);
            }
            Pending::FanToggle(enabled) => {
                let profile = self.profile.map(|p| p.active).unwrap_or(Profile::Balanced);
                asus::fan_set_enabled(profile, enabled)?;
                self.status = format!(
                    "{} 的风扇曲线已{}",
                    profile.zh(),
                    if enabled { "启用" } else { "停用" }
                );
            }
        }

        self.refresh_slow();
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
        self.request_core();
        self.refresh_current_tab();
        if self.tab != Tab::Network && self.last_net_header.elapsed() >= Duration::from_secs(15) {
            self.request_net_snapshot();
            self.last_net_header = Instant::now();
        }
    }

    fn request_core(&mut self) {
        if self.core_loading {
            self.core_refresh_again = true;
            return;
        }
        self.core_loading = true;
        let tx = self.core_tx.clone();
        std::thread::spawn(move || {
            let snapshot = CoreSnapshot {
                profile: asus::profile_get().map_err(|err| format!("{err:#}")),
                battery: asus::battery_info().map_err(|err| format!("{err:#}")),
                attrs: asus::armoury().map_err(|err| format!("{err:#}")),
                tuning: asus::profile_tuning().unwrap_or(false),
            };
            let _ = tx.send(snapshot);
        });
    }

    fn poll_core(&mut self) -> bool {
        let mut updated = false;
        loop {
            match self.core_rx.try_recv() {
                Ok(snapshot) => {
                    self.core_loading = false;
                    let mut errors = Vec::new();
                    match snapshot.profile {
                        Ok(profile) => self.profile = Some(profile),
                        Err(err) => errors.push(format!("性能模式：{err}")),
                    }
                    match snapshot.battery {
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
                    match snapshot.attrs {
                        Ok(attrs) => {
                            self.gpu_mode = asus::gpu_mode(&attrs);
                            if !self.power_dirty {
                                self.power = asus::power_fields(&attrs);
                                self.power_sel =
                                    self.power_sel.min(self.power.len().saturating_sub(1));
                            }
                            self.attrs = attrs;
                        }
                        Err(err) => errors.push(format!("功耗：{err}")),
                    }
                    self.tuning = snapshot.tuning;
                    if !errors.is_empty() {
                        self.status = format!("刷新失败：{}", errors.join("；"));
                        self.status_err = true;
                    }
                    updated = true;
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => break,
            }
        }
        if self.core_refresh_again {
            self.core_refresh_again = false;
            self.request_core();
        }
        updated
    }

    fn refresh_core_sync(&mut self) {
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

        if !errors.is_empty() {
            self.status = format!("刷新失败：{}", errors.join("；"));
            self.status_err = true;
        }
    }

    fn refresh_current_tab(&mut self) {
        match self.tab {
            Tab::Fan => {
                let profile = self.profile.map(|p| p.active).unwrap_or(Profile::Balanced);
                if let Ok(curves) = asus::fan_curves(profile) {
                    self.fan_curves = curves;
                }
                if let Ok(summary) = asus::fan_enabled_summary() {
                    self.fan_enabled = summary;
                }
                self.fan_sel = self
                    .fan_sel
                    .min(self.cfg.fan_presets.len().saturating_sub(1));
            }
            Tab::Lighting => match asus::aura_status() {
                Ok(aura) => {
                    self.led = aura.brightness_name.clone();
                    self.aura = Some(aura);
                }
                Err(_) => {
                    self.aura = None;
                    if let Ok(led) = asus::led_status() {
                        self.led = led;
                    }
                }
            },
            Tab::Network => {
                self.request_net_snapshot();
                if self.cache_open {
                    self.cache_stats = nm::cache_stats().ok();
                }
            }
            Tab::Services => {
                self.services = crate::services::status_all();
                self.service_sel = self.service_sel.min(self.services.len().saturating_sub(1));
            }
            Tab::Settings => {
                self.tuning = asus::profile_tuning().unwrap_or(false);
            }
            Tab::Dashboard | Tab::Performance | Tab::Battery => {}
        }
    }
}

fn point_in(rect: Rect, x: u16, y: u16) -> bool {
    x >= rect.x
        && x < rect.x.saturating_add(rect.width)
        && y >= rect.y
        && y < rect.y.saturating_add(rect.height)
}

fn push_hist(hist: &mut VecDeque<u64>, value: u64) {
    if hist.len() >= HISTORY {
        hist.pop_front();
    }
    hist.push_back(value);
}

pub const AURA_COLORS: [(&str, &str); 6] = [
    ("红", "ff0000"),
    ("橙", "ff8000"),
    ("绿", "00ff00"),
    ("青", "00ffff"),
    ("蓝", "0000ff"),
    ("白", "ffffff"),
];

fn pretty_fan_data(data: &str) -> String {
    data.split(',')
        .map(|point| match point.split_once('c') {
            Some((temp, pwm)) => format!("{temp}°C:{pwm}"),
            None => point.to_string(),
        })
        .collect::<Vec<_>>()
        .join("  ")
}

fn random_password() -> String {
    use std::fs::File;
    use std::io::Read;

    let mut buf = [0u8; 16];
    if let Ok(mut file) = File::open("/dev/urandom")
        && file.read_exact(&mut buf).is_ok()
    {
        const CHARS: &[u8] = b"abcdefghijkmnpqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ23456789";
        return buf
            .iter()
            .take(12)
            .map(|b| CHARS[*b as usize % CHARS.len()] as char)
            .collect();
    }
    "rsztui2026".into()
}

fn ip_form_summary(form: &IpForm) -> String {
    format!(
        "连接：{}\n方法：{}\n地址：{}\n网关：{}\nDNS：{}\nDoT：{}",
        form.conn,
        form.method,
        if form.address.is_empty() {
            "-"
        } else {
            &form.address
        },
        if form.gateway.is_empty() {
            "-"
        } else {
            &form.gateway
        },
        if form.dns.is_empty() { "-" } else { &form.dns },
        match form.dot {
            -1 => "默认",
            0 => "关闭",
            1 => "机会性",
            2 => "强制",
            _ => "默认",
        }
    )
}

fn help_text() -> String {
    [
        "全局",
        "  Tab / Shift+Tab  切换页面     1-8  直接跳转",
        "  鼠标：点击导航/列表/操作栏，双击执行，滚轮选择",
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
        "风扇",
        "  i 编辑 CPU   I 编辑 GPU   e 启用/停用   d 恢复默认",
        "  编辑中：j/k 选点  h/l PWM±1  H/L ±10  [ ] 温度  a 写入  Esc 取消",
        "  j/k 选择预设   p 应用   s 保存当前曲线   x 删除",
        "",
        "灯光",
        "  b 切换键盘亮度   e/E 下一个/上一个灯效",
        "  c 切换静态颜色   S 应用静态色",
        "",
        "服务",
        "  j/k 选择   s 启动   t 停止   R 重启",
        "  e 启用自启   d 禁用自启   E 启用并启动   D 禁用并停止",
        "",
        "网络",
        "  v 切换 连接/Wi-Fi/VPN   j/k 选择   Enter 连接或编辑",
        "  u 激活   d 断开   x 删除   o Wi-Fi 开关   t 热点",
        "  p Ping   n DNS 查询   c 网络缓存（DNS/邻居）",
        "  IPv4 编辑：Enter 切换方法/下一项，直接输入，a 应用",
        "",
        "说明",
        "  修改直接写入 asusd/NM，不生成备份文件",
        "  功耗值写入当前性能模式的 tuning，重启后由 asusd 自动恢复",
    ]
    .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

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
