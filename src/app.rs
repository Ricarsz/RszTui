use std::cell::{Cell, RefCell};
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
use crate::config::{self, Config, FanPreset};
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
    Gpu,
    Services,
    Settings,
}

impl Tab {
    pub const ALL: [Tab; 9] = [
        Tab::Dashboard,
        Tab::Performance,
        Tab::Battery,
        Tab::Fan,
        Tab::Lighting,
        Tab::Network,
        Tab::Gpu,
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
            Tab::Gpu => "显卡",
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
    Presets,
    Power,
    Gpu,
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
        if self.sel >= self.pwm.len() {
            return;
        }
        // asusd 要求 PWM 逐点不降，这里就地夹住，避免写出会被固件拒绝的曲线。
        let lower = if self.sel == 0 {
            0
        } else {
            self.pwm[self.sel - 1] as i32
        };
        let upper = if self.sel + 1 >= self.pwm.len() {
            255
        } else {
            self.pwm[self.sel + 1] as i32
        };
        let upper = upper.max(lower);
        let current = self.pwm[self.sel] as i32;
        let next = (current + delta).clamp(lower, upper);
        if next != current {
            self.pwm[self.sel] = next as u8;
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
        let next = (current + delta).clamp(lower, upper);
        if next != current {
            self.temp[self.sel] = next as u8;
            self.dirty = true;
        }
    }
}

#[derive(Debug, Clone)]
pub enum InputAction {
    Wifi { ssid: String, key_mgmt: String },
    WifiPassword { conn: String },
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
    /// 自定义取色圆盘：按下/拖动时按坐标算色相与饱和度。
    AuraDisc,
    /// 风扇曲线作图区：按下/拖动时移动最近的温控点。
    FanPlot,
    /// 充电上限滑条：按下/拖动直接设百分比。
    BatteryLimit,
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
    PerfPanel(PerfFocus),
    SetAcDefault(usize),
    SetBatteryDefault(usize),
    SetTheme(usize),
    SetRateStyle(usize),
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
    FanSwitch(String),
    FanEditClose,
    Service {
        unit: String,
        action: crate::services::ServiceAction,
    },
    HotspotOff,
    Quit,
}

#[derive(Debug, Clone)]
pub struct HotspotForm {
    pub ssid: String,
    pub password: String,
    /// 0 = SSID，1 = 密码
    pub focus: usize,
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
    Hotspot(HotspotForm),
}

struct CoreSnapshot {
    profile: Result<ProfileState, String>,
    battery: Result<BatteryInfo, String>,
    attrs: Result<Vec<asus::Attr>, String>,
    pstate: crate::sys::Pstate,
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
    /// 自定义颜色：色相 0–360 度、饱和度 0–1。
    pub aura_pick: (f32, f32),
    /// 风扇曲线作图区的实际矩形，拖动时用它换算 PWM。
    pub fan_plot: RefCell<Option<Rect>>,
    /// 取色圆盘与充电上限滑条的实际矩形，鼠标换算要用。
    pub aura_disc: RefCell<Option<Rect>>,
    pub limit_bar: RefCell<Option<Rect>>,
    fan_drag: Option<usize>,
    pub tuning: bool,
    pub pstate: crate::sys::Pstate,
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
    pub modal_scroll: Cell<u16>,
    pub hotspot_ssid: String,
    pub mouse_targets: RefCell<Vec<MouseTarget>>,
    pub quit: bool,
    last_slow: Instant,
    snap_rx: Receiver<Snapshot>,
    pub snap_loading: bool,
    hw_rx: Receiver<hw::HardwareInfo>,
    core_initialized: bool,
    pending_tab_refresh: bool,
}

pub fn run(cfg: Config, report: deps::Report) -> Result<()> {
    let mut app = App::new(cfg, report);
    let mut terminal = ratatui::init();
    // 启用 Kitty 键盘协议，让终端把数字小键盘标记为 KEYPAD（配合 NumLock）。
    let _ = execute!(
        std::io::stdout(),
        PushKeyboardEnhancementFlags(
            KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                | KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS
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
        let (snap_tx, snap_rx) = mpsc::channel();
        let (hw_tx, hw_rx) = mpsc::channel();

        // 硬件信息与实时采样都放到后台，先让界面出来。
        std::thread::spawn(move || {
            let _ = hw_tx.send(hw::collect());
        });
        {
            let tx = snap_tx;
            std::thread::spawn(move || {
                let mut collector = Collector::default();
                loop {
                    if tx.send(collector.sample()).is_err() {
                        break;
                    }
                    std::thread::sleep(Duration::from_secs(1));
                }
            });
        }
        let tab = Tab::from_index(cfg.ui.last_tab);
        let mut app = Self {
            cfg,
            deps: report,
            hw: hw::HardwareInfo::default(),
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
            aura_pick: (0.0, 0.9),
            fan_plot: RefCell::new(None),
            aura_disc: RefCell::new(None),
            limit_bar: RefCell::new(None),
            fan_drag: None,
            tuning: false,
            pstate: crate::sys::Pstate::default(),
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
            status: "正在读取系统信息…".into(),
            status_err: false,
            modal: None,
            modal_scroll: Cell::new(0),
            hotspot_ssid: "rsztui-hotspot".into(),
            mouse_targets: RefCell::new(Vec::new()),
            quit: false,
            last_slow: Instant::now(),
            snap_rx,
            snap_loading: true,
            hw_rx,
            core_initialized: false,
            pending_tab_refresh: true,
        };
        app.request_core();
        app.request_net_snapshot();
        if app.deps.has_warnings() {
            let body = app.deps.summary_text();
            app.show_message("依赖检查", body);
        }
        app
    }

    fn show_message(&mut self, title: impl Into<String>, body: impl Into<String>) {
        self.modal_scroll.set(0);
        self.modal = Some(Modal::Message {
            title: title.into(),
            body: body.into(),
        });
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
                    Event::Resize(_, _) => {
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
            if self.poll_hw() {
                dirty = true;
            }
            if self.poll_snap() {
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
        key = normalize_shift(key);

        if key.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(key.code, KeyCode::Char('c') | KeyCode::Char('C'))
        {
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
                let dirty = self.unsaved_changes();
                if dirty.is_empty() {
                    self.quit = true;
                } else {
                    let body = format!(
                        "以下更改尚未应用：\n  • {}\n\n退出将丢弃这些更改，确定退出？",
                        dirty.join("\n  • ")
                    );
                    self.confirm("退出", body, Pending::Quit);
                }
            }
            KeyCode::Char('r') => {
                self.deps = deps::check();
                self.hw = hw::collect();
                self.refresh_slow();
                self.status = "已刷新".into();
                self.status_err = false;
            }
            KeyCode::Char('?') => {
                let body = help_text();
                self.show_message("快捷键", body);
            }
            KeyCode::Tab => self.switch_tab(Tab::next(self.tab)),
            KeyCode::BackTab => self.switch_tab(Tab::prev(self.tab)),
            KeyCode::Char(c @ '1'..='9') => {
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
                    self.activate_mouse(action, double, (mouse.column, mouse.row))?;
                }
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                let action = self.mouse_action_at(mouse.column, mouse.row);
                if action.is_some() {
                    self.hover = action;
                }
                self.handle_drag(mouse.column, mouse.row, action)?;
            }
            MouseEventKind::Up(MouseButton::Left) => {
                self.fan_drag = None;
            }
            MouseEventKind::ScrollUp => self.scroll_at(mouse.column, mouse.row, -1)?,
            MouseEventKind::ScrollDown => self.scroll_at(mouse.column, mouse.row, 1)?,
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

    /// 滚轮带坐标：即使终端没上报「仅移动」事件，也按指针位置判断调值对象。
    fn scroll_at(&mut self, x: u16, y: u16, direction: i32) -> Result<()> {
        self.hover = self.mouse_action_at(x, y);
        self.scroll(direction)
    }

    fn scroll(&mut self, direction: i32) -> Result<()> {
        let code = if direction < 0 {
            KeyCode::Up
        } else {
            KeyCode::Down
        };
        if self.modal.is_some() {
            return self.handle_modal_key(KeyEvent::new(code, KeyModifiers::NONE));
        }
        if self.ip_form.is_some() {
            return Ok(());
        }
        // 指针停在可调数值上时，滚轮调值而不是移动光标。
        let step = if direction < 0 { -1 } else { 1 };
        match self.hover {
            Some(MouseAction::PowerField(index)) => {
                self.perf_focus = PerfFocus::Power;
                self.power_sel = index.min(self.power.len().saturating_sub(1));
                self.adjust_power(step);
                return Ok(());
            }
            Some(MouseAction::BatteryLimit) => {
                self.limit_edit = (self.limit_edit as i32 + step * 5).clamp(20, 100) as u8;
                self.limit_dirty = true;
                self.status = format!("充电上限 → {}%（Enter 应用）", self.limit_edit);
                self.status_err = false;
                return Ok(());
            }
            Some(MouseAction::FanPoint(index)) => {
                if let Some(edit) = self.fan_edit.as_mut() {
                    let target = index.min(edit.pwm.len().saturating_sub(1));
                    if let Some(slot) = edit.pwm.get_mut(target) {
                        let next = (*slot as i32 + step * 8).clamp(0, 255) as u8;
                        if next != *slot {
                            *slot = next;
                            edit.dirty = true;
                            edit.sel = target;
                            self.status = format!(
                                "{}:{}°C → PWM {next}",
                                edit.fan.to_uppercase(),
                                edit.temp[target]
                            );
                            self.status_err = false;
                        }
                    }
                }
                return Ok(());
            }
            Some(MouseAction::FanPlot) => {
                if let Some(edit) = self.fan_edit.as_mut() {
                    let target = edit.sel.min(edit.pwm.len().saturating_sub(1));
                    if let Some(slot) = edit.pwm.get_mut(target) {
                        let next = (*slot as i32 + step * 8).clamp(0, 255) as u8;
                        *slot = next;
                        edit.dirty = true;
                        self.status = format!(
                            "{}:{}°C → PWM {next}",
                            edit.fan.to_uppercase(),
                            edit.temp[target]
                        );
                        self.status_err = false;
                    }
                }
                return Ok(());
            }
            _ => {}
        }
        self.handle_tab_key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    /// 拖动：旋钮/滑条/曲线跟着指针走，指针移出控件也继续用最后方向。
    fn handle_drag(&mut self, column: u16, row: u16, action: Option<MouseAction>) -> Result<()> {
        match action {
            Some(MouseAction::AuraDisc) => self.pick_aura(column, row),
            Some(MouseAction::BatteryLimit) => self.set_limit_from(column),
            Some(MouseAction::FanPlot) | Some(MouseAction::FanPoint(_)) => {
                if self.fan_drag.is_none() {
                    self.fan_drag = self.fan_index_at(column);
                }
                self.fan_drag_to(row);
            }
            None if self.fan_drag.is_some() => self.fan_drag_to(row),
            _ => {}
        }
        Ok(())
    }

    fn activate_mouse(&mut self, action: MouseAction, double: bool, at: (u16, u16)) -> Result<()> {
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
                self.perf_focus = PerfFocus::Presets;
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
                self.perf_focus = PerfFocus::Gpu;
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
            MouseAction::AuraDisc => {
                self.pick_aura(at.0, at.1);
                if double {
                    self.apply_aura_pick()?;
                }
            }
            MouseAction::BatteryLimit => {
                self.set_limit_from(at.0);
            }
            MouseAction::FanPlot => {
                if self.fan_edit.is_none() {
                    self.status = "先按 i 或 I 进入曲线编辑，再用鼠标拖点".into();
                    self.status_err = false;
                    return Ok(());
                }
                let index = self.fan_index_at(at.0);
                if self.fan_edit.is_some() {
                    self.fan_drag = index;
                    self.fan_drag_to(at.1);
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
            MouseAction::PerfPanel(focus) => {
                self.perf_focus = focus;
            }
            MouseAction::SetAcDefault(index) => {
                let profile = Profile::ALL[index.min(Profile::ALL.len() - 1)];
                self.confirm(
                    "设置 AC 默认模式",
                    format!("AC 供电时默认使用{}模式？", profile.zh()),
                    Pending::DefaultAc(profile),
                );
            }
            MouseAction::SetBatteryDefault(index) => {
                let profile = Profile::ALL[index.min(Profile::ALL.len() - 1)];
                self.confirm(
                    "设置电池默认模式",
                    format!("电池供电时默认使用{}模式？", profile.zh()),
                    Pending::DefaultBattery(profile),
                );
            }
            MouseAction::SetTheme(index) => self.set_theme(index)?,
            MouseAction::SetRateStyle(index) => self.set_rate_style(index)?,
            MouseAction::ModalConfirm | MouseAction::ModalCancel => {}
        }
        Ok(())
    }

    fn modal_confirm(&mut self) -> Result<()> {
        match self.modal.take() {
            Some(Modal::Confirm { pending, .. }) => self.apply_pending(pending)?,
            Some(Modal::Input { value, action, .. }) => self.submit_input(action, value)?,
            Some(Modal::Hotspot(form)) => self.start_hotspot(form)?,
            Some(Modal::Message { .. }) | None => {}
        }
        Ok(())
    }

    fn handle_modal_key(&mut self, key: KeyEvent) -> Result<()> {
        if matches!(self.modal, Some(Modal::Message { .. })) {
            match key.code {
                KeyCode::Esc | KeyCode::Enter => self.modal = None,
                KeyCode::Down | KeyCode::Char('j') => {
                    self.modal_scroll
                        .set(self.modal_scroll.get().saturating_add(1));
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    self.modal_scroll
                        .set(self.modal_scroll.get().saturating_sub(1));
                }
                KeyCode::PageDown => {
                    self.modal_scroll
                        .set(self.modal_scroll.get().saturating_add(10));
                }
                KeyCode::PageUp => {
                    self.modal_scroll
                        .set(self.modal_scroll.get().saturating_sub(10));
                }
                KeyCode::Home => self.modal_scroll.set(0),
                _ => {}
            }
            return Ok(());
        }

        if matches!(self.modal, Some(Modal::Hotspot(_))) {
            match key.code {
                KeyCode::Esc => self.modal = None,
                KeyCode::Enter => {
                    if let Some(Modal::Hotspot(form)) = self.modal.take() {
                        self.start_hotspot(form)?;
                    }
                }
                KeyCode::Tab | KeyCode::Up | KeyCode::Down => {
                    if let Some(Modal::Hotspot(form)) = self.modal.as_mut() {
                        form.focus = 1 - form.focus;
                    }
                }
                KeyCode::Backspace => {
                    if let Some(Modal::Hotspot(form)) = self.modal.as_mut() {
                        let field = if form.focus == 0 {
                            &mut form.ssid
                        } else {
                            &mut form.password
                        };
                        field.pop();
                    }
                }
                KeyCode::Char(c) if !c.is_control() => {
                    if let Some(Modal::Hotspot(form)) = self.modal.as_mut() {
                        let field = if form.focus == 0 {
                            &mut form.ssid
                        } else {
                            &mut form.password
                        };
                        let max = if form.focus == 0 { 32 } else { 63 };
                        if field.chars().count() < max {
                            field.push(c);
                        }
                    }
                }
                _ => {}
            }
            return Ok(());
        }

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
            KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                self.modal = None;
            }
            KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => self.modal_confirm()?,
            _ => {}
        }
        Ok(())
    }

    fn start_hotspot(&mut self, form: HotspotForm) -> Result<()> {
        let ssid = form.ssid.trim();
        if ssid.is_empty() {
            bail!("热点 SSID 不能为空");
        }
        if form.password.chars().count() < 8 {
            bail!("热点密码至少 8 位");
        }
        nm::hotspot(&form.password, ssid)?;
        self.hotspot_ssid = ssid.to_string();
        self.status = format!("热点已开启：{ssid}");
        self.status_err = false;
        self.refresh_slow();
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
            InputAction::WifiPassword { conn } => {
                nm::set_wifi_password(&conn, &value)?;
                self.status = format!("已更新 Wi-Fi 密码：{conn}");
            }
            InputAction::Ping => {
                let output = nm::ping(&value)?;
                self.show_message(format!("Ping {value}"), output);
            }
            InputAction::DnsLookup => {
                let output = nm::dns_query(&value)?;
                self.show_message(format!("DNS 查询 {value}"), output);
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

    fn unsaved_changes(&self) -> Vec<&'static str> {
        let mut dirty = Vec::new();
        if self.limit_dirty {
            dirty.push("电池充电上限");
        }
        if self.power_dirty {
            dirty.push("功耗 / TGP");
        }
        if self.fan_edit.as_ref().is_some_and(|edit| edit.dirty) {
            dirty.push("风扇曲线");
        }
        if self.ip_form.is_some() {
            dirty.push("IPv4 编辑");
        }
        dirty
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
            Tab::Settings => self.handle_settings_key(key)?,
            _ => {}
        }
        Ok(())
    }

    fn handle_settings_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Char('t') => {
                let next = (config::theme_index(&self.cfg.ui.theme) + 1) % config::THEMES.len();
                self.set_theme(next)?;
            }
            KeyCode::Char('y') => {
                let next = (config::rate_style_index(&self.cfg.ui.rate_style) + 1)
                    % config::RATE_STYLES.len();
                self.set_rate_style(next)?;
            }
            _ => {}
        }
        Ok(())
    }

    fn set_theme(&mut self, index: usize) -> Result<()> {
        if let Some((key, label)) = config::THEMES.get(index) {
            self.cfg.ui.theme = (*key).to_string();
            self.cfg.save()?;
            self.status = format!("主题：{label}");
            self.status_err = false;
        }
        Ok(())
    }

    fn set_rate_style(&mut self, index: usize) -> Result<()> {
        if let Some((key, label)) = config::RATE_STYLES.get(index) {
            self.cfg.ui.rate_style = (*key).to_string();
            self.cfg.save()?;
            self.status = format!("速率格式：{label}");
            self.status_err = false;
        }
        Ok(())
    }

    fn handle_performance_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Char('f') => {
                self.perf_focus = match self.perf_focus {
                    PerfFocus::Profiles => PerfFocus::Presets,
                    PerfFocus::Presets => PerfFocus::Power,
                    PerfFocus::Power => PerfFocus::Gpu,
                    PerfFocus::Gpu => PerfFocus::Profiles,
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
                PerfFocus::Gpu => match key.code {
                    KeyCode::Down | KeyCode::Char('j') => {
                        self.gpu_sel = (self.gpu_sel + 1) % GpuMode::ALL.len();
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        self.gpu_sel = (self.gpu_sel + GpuMode::ALL.len() - 1) % GpuMode::ALL.len();
                    }
                    KeyCode::Enter | KeyCode::Char('G') => {
                        let mode = GpuMode::ALL[self.gpu_sel % GpuMode::ALL.len()];
                        self.confirm(
                            "切换 GPU 模式",
                            format!("将 GPU 模式设为{}？修改后需要重启。", mode.zh()),
                            Pending::Gpu(mode),
                        );
                    }
                    _ => {}
                },
                PerfFocus::Presets => match key.code {
                    KeyCode::Down | KeyCode::Char('j') => {
                        if !self.cfg.power_presets.is_empty() {
                            self.preset_sel = (self.preset_sel + 1) % self.cfg.power_presets.len();
                        }
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        if !self.cfg.power_presets.is_empty() {
                            self.preset_sel = (self.preset_sel + self.cfg.power_presets.len() - 1)
                                % self.cfg.power_presets.len();
                        }
                    }
                    KeyCode::Home => self.preset_sel = 0,
                    KeyCode::End => {
                        self.preset_sel = self.cfg.power_presets.len().saturating_sub(1)
                    }
                    KeyCode::Enter | KeyCode::Char('p') if !self.cfg.power_presets.is_empty() => {
                        let preset = self.cfg.power_presets[self.preset_sel].clone();
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

    /// 圆盘坐标 → (色相, 饱和度)：横向是色相环，离圆心越远越饱和。
    fn pick_aura(&mut self, column: u16, row: u16) {
        let Some(rect) = *self.aura_disc.borrow() else {
            return;
        };
        if rect.width < 4 || rect.height < 3 {
            return;
        }
        let cx = rect.x as f32 + rect.width as f32 / 2.0;
        let cy = rect.y as f32 + rect.height as f32 / 2.0;
        let rx = (rect.width as f32 / 2.0 - 0.5).max(0.5);
        let ry = (rect.height as f32 / 2.0 - 0.5).max(0.5);
        let sx = (column as f32 + 0.5 - cx) / rx;
        let sy = (row as f32 + 0.5 - cy) / ry;
        let radius = (sx * sx + sy * sy).sqrt();
        let saturation = radius.min(1.0);
        let hue = sy.atan2(sx).to_degrees();
        self.aura_pick = (hue, saturation);
    }

    pub fn aura_pick_hex(&self) -> String {
        crate::util::hsv_to_hex(self.aura_pick.0, self.aura_pick.1, 1.0)
    }

    fn apply_aura_pick(&mut self) -> Result<()> {
        let hex = self.aura_pick_hex();
        asus::aura_static(&hex)?;
        self.refresh_current_tab();
        self.status = format!("灯效：静态自定义色 {hex}");
        self.status_err = false;
        Ok(())
    }

    /// 充电上限滑条坐标 → 百分比（20–100），按 5 的倍数取整。
    fn set_limit_from(&mut self, column: u16) {
        let Some(rect) = *self.limit_bar.borrow() else {
            return;
        };
        if rect.width == 0 {
            return;
        }
        let ratio =
            ((column.saturating_sub(rect.x)) as f32 / rect.width.max(1) as f32).clamp(0.0, 1.0);
        let percent = (20.0 + ratio * 80.0).round() as u8;
        let percent = ((percent as i32 + 2) / 5 * 5).clamp(20, 100) as u8;
        if self.limit_edit != percent {
            self.limit_edit = percent;
            self.limit_dirty = true;
            self.status = format!("充电上限 → {percent}%（Enter 应用）");
            self.status_err = false;
        }
    }

    /// 作图区横坐标 → 最近的温控点下标。
    fn fan_index_at(&self, column: u16) -> Option<usize> {
        let edit = self.fan_edit.as_ref()?;
        let rect = (*self.fan_plot.borrow())?;
        if rect.width == 0 || edit.temp.is_empty() {
            return None;
        }
        let ratio =
            ((column.saturating_sub(rect.x)) as f32 / rect.width.max(1) as f32).clamp(0.0, 1.0);
        let temp = 20.0 + ratio * 80.0;
        edit.temp
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| {
                (**a as f32 - temp)
                    .abs()
                    .total_cmp(&(**b as f32 - temp).abs())
            })
            .map(|(index, _)| index)
    }

    /// 作图区纵坐标 → PWM，写进正在拖的那个点。
    fn fan_drag_to(&mut self, row: u16) {
        let Some(index) = self.fan_drag else {
            return;
        };
        let Some(rect) = *self.fan_plot.borrow() else {
            return;
        };
        if rect.height == 0 {
            return;
        }
        let ratio =
            ((row.saturating_sub(rect.y)) as f32 / (rect.height - 1).max(1) as f32).clamp(0.0, 1.0);
        let pwm = ((1.0 - ratio) * 255.0).round() as u8;
        let Some(edit) = self.fan_edit.as_mut() else {
            return;
        };
        if let Some(slot) = edit.pwm.get_mut(index)
            && *slot != pwm
        {
            *slot = pwm;
            edit.dirty = true;
            edit.sel = index;
            self.status = format!(
                "{}:{}°C → PWM {pwm}",
                edit.fan.to_uppercase(),
                edit.temp[index]
            );
            self.status_err = false;
        }
    }

    fn adjust_power(&mut self, delta: i32) {
        let Some(field) = self.power.get_mut(self.power_sel) else {
            return;
        };
        if !field.writable() {
            self.status = format!("{}：固件不可调（min = max）", field.label);
            self.status_err = true;
            return;
        }
        // 不用 i32::clamp：min > max 时它会 panic，固件偶尔真的这么报。
        let (lo, hi) = (field.min.min(field.max), field.min.max(field.max));
        field.value = (field.value + delta).clamp(lo, hi);
        self.power_dirty = true;
        self.status = format!("{} → {} {}", field.label, field.value, field.unit);
        self.status_err = false;
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
                    "把当前性能模式已写入的 CPU/GPU 曲线保存为预设？\n同名预设会被覆盖。",
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
                        "{}当前性能模式的 CPU/GPU 风扇曲线？",
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
            if self.fan_edit.as_ref().is_some_and(|e| e.dirty) {
                self.confirm(
                    "切换风扇曲线",
                    format!(
                        "当前曲线有未写入的修改，切换到 {} 会丢弃这些修改。继续？",
                        next.to_uppercase()
                    ),
                    Pending::FanSwitch(next.to_string()),
                );
            } else {
                self.open_fan_edit(next)?;
                self.status = format!("已切换到 {} 曲线", next.to_uppercase());
            }
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
            KeyCode::Char('C') => {
                self.apply_aura_pick()?;
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
            KeyCode::Char('P') => self.open_wifi_password()?,
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
            self.modal = Some(Modal::Hotspot(HotspotForm {
                ssid: self.hotspot_ssid.clone(),
                password: random_password(),
                focus: 0,
            }));
        }
    }

    fn open_wifi_password(&mut self) -> Result<()> {
        let (conn, label) = match self.net_view {
            NetView::Connections => {
                let Some(conn) = self.selected_connection() else {
                    bail!("没有选中连接");
                };
                if !conn.kind.contains("wireless") {
                    bail!("只有 Wi-Fi 连接支持修改密码");
                }
                (conn.name.clone(), conn.name)
            }
            NetView::Wifi => {
                let Some(ap) = self.wifi.get(self.net_sel).cloned() else {
                    bail!("没有选中 Wi-Fi");
                };
                let Some(profile) = nm::wifi_profile(&ap.ssid)? else {
                    bail!("「{}」还没有保存的连接，请先连接一次", ap.ssid);
                };
                (profile.name, ap.ssid)
            }
            NetView::Vpn => bail!("VPN 暂不支持在此修改密码"),
        };
        self.modal = Some(Modal::Input {
            title: "修改 Wi-Fi 密码".into(),
            prompt: format!("{label} 的新密码（8–63 位）"),
            value: String::new(),
            masked: true,
            action: InputAction::WifiPassword { conn },
        });
        Ok(())
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
                // 预设里的值来自别的机型也很常见，逐项写入并跳过固件不接受的项。
                let mut problems = Vec::new();
                for (key, value) in [
                    ("ppt_pl1_spl", preset.pl1),
                    ("ppt_pl2_sppt", preset.pl2),
                    ("ppt_pl3_fppt", preset.pl3),
                    ("nv_tgp", preset.gpu_tgp),
                    ("nv_dynamic_boost", preset.gpu_boost),
                    ("nv_temp_target", preset.gpu_temp),
                ] {
                    let Some(field) = self.power.iter().find(|field| field.key == key) else {
                        continue;
                    };
                    let candidate = asus::PowerField {
                        value,
                        ..field.clone()
                    };
                    match asus::set_power_field(&candidate) {
                        asus::SetOutcome::Applied(_) => {}
                        asus::SetOutcome::Skipped(why) => {
                            problems.push(format!("{} 跳过（{why}）", field.label))
                        }
                        asus::SetOutcome::Failed(err) => {
                            problems.push(format!("{} 失败（{err}）", field.label))
                        }
                    }
                }
                self.power_dirty = false;
                self.request_core();
                if problems.is_empty() {
                    self.status_err = false;
                    self.status =
                        format!("已套用「{}」预设到{}模式", preset.name, preset.profile.zh());
                } else {
                    self.status_err = true;
                    self.status =
                        format!("已套用「{}」预设，但：{}", preset.name, problems.join("；"));
                }
            }
            Pending::PowerApply => {
                if self.power.is_empty() {
                    bail!("没有可写的功耗属性");
                }
                let mut applied = Vec::new();
                let mut problems = Vec::new();
                let mut max_pl3 = 0;
                for field in self.power.clone() {
                    match asus::set_power_field(&field) {
                        asus::SetOutcome::Applied(value) => {
                            applied.push(format!("{} {}", field.label, value));
                            if field.key == "ppt_pl3_fppt" {
                                max_pl3 = value;
                            }
                        }
                        asus::SetOutcome::Skipped(why) => {
                            problems.push(format!("{} 跳过（{why}）", field.label));
                        }
                        asus::SetOutcome::Failed(err) => {
                            problems.push(format!("{} 失败（{err}）", field.label));
                        }
                    }
                }
                self.power_dirty = false;
                if applied.is_empty() {
                    bail!("所有字段都没写进去：{}", problems.join("；"));
                }
                let mut status = format!("已写入 {} 项：{}", applied.len(), applied.join("、"));
                if max_pl3 > 65 {
                    status.push_str(&format!(
                        "
PL3 {max_pl3}W 高于 65W，注意温度"
                    ));
                }
                if !problems.is_empty() {
                    status.push('\n');
                    status.push_str(&problems.join("\n"));
                    self.status_err = true;
                } else {
                    self.status_err = false;
                }
                self.status = status;
                self.request_core();
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
                self.refresh_fan()?;
                self.status = format!(
                    "已套用风扇预设：{}（{}，已启用）",
                    preset.name,
                    preset.profile.zh()
                );
            }
            Pending::FanSave => {
                let profile = self.profile.map(|p| p.active).unwrap_or(Profile::Balanced);
                // 保存前重新读一次硬件，避免把界面上的旧曲线存成预设。
                self.fan_curves = asus::fan_curves(profile)?;
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
                self.refresh_fan()?;
                self.status = format!("{} 的风扇曲线已恢复默认", profile.zh());
            }
            Pending::FanCurveApply(edit) => {
                let profile = self.profile.map(|p| p.active).unwrap_or(Profile::Balanced);
                let result = asus::fan_set_curve(profile, &edit.fan, &edit.data());
                let refreshed = self.refresh_fan();
                result?;
                refreshed?;
                if let Some(current) = self.fan_edit.as_mut() {
                    if let Some(curve) = self
                        .fan_curves
                        .iter()
                        .find(|c| c.fan.eq_ignore_ascii_case(&edit.fan))
                    {
                        current.temp = curve.temp.clone();
                        current.pwm = curve.pwm.clone();
                    }
                    current.dirty = false;
                }
                self.status = format!(
                    "{} 风扇曲线已写入并启用（{}）",
                    edit.fan.to_uppercase(),
                    profile.zh()
                );
            }
            Pending::FanSwitch(fan) => {
                self.open_fan_edit(&fan)?;
                self.status = format!("已切换到 {} 曲线（未写入的修改已丢弃）", fan.to_uppercase());
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
                self.refresh_fan()?;
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

    fn poll_hw(&mut self) -> bool {
        match self.hw_rx.try_recv() {
            Ok(info) => {
                self.hw = info;
                true
            }
            Err(_) => false,
        }
    }

    /// 后台快照里的营销型号优先，硬件枚举（lspci）名字兜底。
    pub fn gpu_names(&self) -> Vec<String> {
        if self.snap.gpus.is_empty() {
            self.hw.gpus.clone()
        } else {
            self.snap.gpus.iter().map(|g| g.gpu.name.clone()).collect()
        }
    }

    fn poll_snap(&mut self) -> bool {
        let mut latest = None;
        while let Ok(snap) = self.snap_rx.try_recv() {
            latest = Some(snap);
        }
        let Some(snap) = latest else {
            return false;
        };
        push_hist(&mut self.cpu_hist, snap.cpu_pct as u64);
        let mem_pct = if snap.mem_total > 0 {
            (snap.mem_used as f64 / snap.mem_total as f64 * 100.0) as u64
        } else {
            0
        };
        push_hist(&mut self.mem_hist, mem_pct);
        push_hist(&mut self.rx_hist, snap.rx_bps as u64);
        push_hist(&mut self.tx_hist, snap.tx_bps as u64);
        self.snap = snap;
        if self.snap_loading {
            self.snap_loading = false;
            if self.status == "正在读取系统信息…" {
                self.status = "就绪".into();
            }
        }
        if self.pending_tab_refresh {
            self.pending_tab_refresh = false;
            self.refresh_current_tab();
        }
        true
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
                pstate: crate::sys::pstate(),
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
                    self.pstate = snapshot.pstate;
                    self.tuning = snapshot.tuning;
                    if !self.core_initialized {
                        self.core_initialized = true;
                        if let Some(profile) = self.profile {
                            self.profile_sel = Profile::ALL
                                .iter()
                                .position(|p| *p == profile.active)
                                .unwrap_or(0);
                        }
                        if let Some(mode) = self.gpu_mode {
                            self.gpu_sel =
                                GpuMode::ALL.iter().position(|m| *m == mode).unwrap_or(1);
                        }
                        if let Some(limit) = self.battery.limit {
                            self.limit_edit = limit;
                        }
                    }
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

    /// 立刻重读当前性能模式的风扇曲线；写操作后调用，避免界面停留在旧值。
    fn refresh_fan(&mut self) -> Result<()> {
        let profile = self.profile.map(|p| p.active).unwrap_or(Profile::Balanced);
        self.fan_curves = asus::fan_curves(profile)?;
        if let Ok(summary) = asus::fan_enabled_summary() {
            self.fan_enabled = summary;
        }
        self.fan_sel = self
            .fan_sel
            .min(self.cfg.fan_presets.len().saturating_sub(1));
        Ok(())
    }

    fn refresh_current_tab(&mut self) {
        match self.tab {
            Tab::Fan => {
                let _ = self.refresh_fan();
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
            Tab::Dashboard | Tab::Performance | Tab::Battery | Tab::Gpu => {}
        }
    }
}

fn point_in(rect: Rect, x: u16, y: u16) -> bool {
    x >= rect.x
        && x < rect.x.saturating_add(rect.width)
        && y >= rect.y
        && y < rect.y.saturating_add(rect.height)
}

/// Kitty 协议终端（未启用 alternate keys 时）把 Shift+i 报成 `Char('i') + SHIFT`，
/// 而应用里的大写绑定是 `Char('I')`；这里在入口统一归一化，文本输入同样受益。
fn normalize_shift(mut key: KeyEvent) -> KeyEvent {
    if key.modifiers.contains(KeyModifiers::SHIFT)
        && let KeyCode::Char(c) = key.code
    {
        if c.is_ascii_lowercase() {
            key.code = KeyCode::Char(c.to_ascii_uppercase());
        }
        key.modifiers.remove(KeyModifiers::SHIFT);
    }
    key
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
            Some((temp, pwm)) => format!("{temp}°C:{}", pwm.trim_start_matches(':')),
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
        "  Tab / Shift+Tab  切换页面     1-9  直接跳转",
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
        "  编辑中：j/k 选点  h/l PWM±1  H/L ±10  [ ] 温度  c 切换  a 写入  Esc 取消",
        "  a 写入并启用后会回读校验，失败会直接报错",
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
        "显卡",
        "  只读监控：核心/热点/显存温度、负载、显存、功耗与 PCIe 链路",
        "",
        "网络",
        "  v 切换 连接/Wi-Fi/VPN   j/k 选择   Enter 连接或编辑",
        "  u 激活   d 断开   x 删除   o Wi-Fi 开关   t 热点",
        "  P 修改 Wi-Fi 密码（需已保存该连接）",
        "  p Ping   n DNS 查询   c 网络缓存（DNS/邻居）",
        "  IPv4 编辑：Enter 切换方法/下一项，直接输入，a 应用",
        "  热点：Tab/↑↓ 切换 SSID 与密码，Enter 开启",
        "",
        "设置",
        "  t 下一个主题   y 下一种速率格式   也可直接点击设置页的选项",
        "",
        "说明",
        "  硬件修改直接写入 asusd/NM；配置文件写入前自动备份为 config.toml.bak",
        "  功耗值写入当前性能模式的 tuning，重启后由 asusd 自动恢复",
        "  确认弹窗按 Enter/y，取消按 Esc/n",
        "  长文本弹窗用 j/k 或 PgUp/PgDn 滚动，Enter/Esc 关闭",
    ]
    .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shift_letter_normalizes_to_uppercase() {
        let key = normalize_shift(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::SHIFT));
        assert_eq!(key.code, KeyCode::Char('I'));
        assert!(key.modifiers.is_empty());
    }

    #[test]
    fn shift_without_letter_keeps_code() {
        let key = normalize_shift(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::SHIFT));
        assert_eq!(key.code, KeyCode::Char('?'));
        assert!(key.modifiers.is_empty());
    }

    #[test]
    fn fan_edit_pwm_stays_monotonic() {
        let curve = asus::FanCurve {
            fan: "CPU".into(),
            temp: vec![40, 50, 60],
            pwm: vec![10, 20, 30],
            enabled: true,
        };
        let mut edit = FanEdit::from_curve(&curve, "cpu").unwrap();
        edit.adjust_pwm(200);
        assert_eq!(edit.pwm, vec![20, 20, 30]);
        edit.sel = 2;
        edit.adjust_pwm(-200);
        assert_eq!(edit.pwm, vec![20, 20, 20]);
    }

    #[test]
    fn pretty_fan_data_has_single_colon() {
        assert_eq!(pretty_fan_data("43c:30,46c:35"), "43°C:30  46°C:35");
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

#[cfg(test)]
mod mouse_tests {
    use super::*;

    fn app() -> App {
        let cfg = Config::default();
        let report = deps::Report {
            distro: String::new(),
            vendor: String::new(),
            product: String::new(),
            laptop: false,
            asus_hardware: false,
            checks: Vec::new(),
        };
        App::new(cfg, report)
    }

    #[test]
    fn wheel_over_battery_bar_steps_limit() {
        let mut app = app();
        app.limit_edit = 60;
        app.mouse_targets.borrow_mut().push(MouseTarget {
            rect: Rect::new(10, 10, 20, 1),
            action: MouseAction::BatteryLimit,
        });
        app.scroll_at(15, 10, 1).unwrap();
        assert_eq!(app.limit_edit, 65);
        assert!(app.limit_dirty);
        assert_eq!(app.hover, Some(MouseAction::BatteryLimit));

        app.scroll_at(15, 10, -1).unwrap();
        assert_eq!(app.limit_edit, 60);

        // 指针在别处时不调值，只走普通滚动
        app.scroll_at(5, 30, 1).unwrap();
        assert_eq!(app.limit_edit, 60);
    }

    #[test]
    fn wheel_over_power_row_adjusts_value() {
        let mut app = app();
        app.tab = Tab::Performance;
        app.power = vec![asus::PowerField {
            key: "ppt_pl1_spl",
            label: "PL1",
            unit: "W",
            value: 30,
            min: 15,
            max: 65,
        }];
        app.mouse_targets.borrow_mut().push(MouseTarget {
            rect: Rect::new(40, 20, 30, 1),
            action: MouseAction::PowerField(0),
        });
        app.scroll_at(45, 20, 1).unwrap();
        assert_eq!(app.power[0].value, 31);
        assert_eq!(app.power_sel, 0);
        assert_eq!(app.perf_focus, PerfFocus::Power);
        app.scroll_at(45, 20, -1).unwrap();
        assert_eq!(app.power[0].value, 30);
    }

    #[test]
    fn unwritable_power_field_ignores_wheel() {
        let mut app = app();
        app.power = vec![asus::PowerField {
            key: "nv_tgp",
            label: "TGP",
            unit: "W",
            value: 55,
            min: 0,
            max: 0,
        }];
        app.mouse_targets.borrow_mut().push(MouseTarget {
            rect: Rect::new(0, 0, 40, 1),
            action: MouseAction::PowerField(0),
        });
        app.scroll_at(5, 0, 1).unwrap();
        assert_eq!(app.power[0].value, 55, "min == max 时不能改值");
        assert!(app.status.contains("固件不可调"));
    }

    #[test]
    fn fan_plot_drag_sets_pwm_from_row() {
        let mut app = app();
        app.fan_edit = Some(FanEdit {
            fan: "cpu".into(),
            temp: vec![30, 50, 70, 85],
            pwm: vec![30, 60, 120, 200],
            sel: 1,
            dirty: false,
        });
        *app.fan_plot.borrow_mut() = Some(Rect::new(10, 10, 40, 21));
        app.fan_drag = Some(1);
        app.fan_drag_to(10); // 顶行 → PWM 255
        assert_eq!(app.fan_edit.as_ref().unwrap().pwm[1], 255);
        app.fan_drag_to(30); // 底行 → PWM 0
        assert_eq!(app.fan_edit.as_ref().unwrap().pwm[1], 0);
        assert!(app.fan_edit.as_ref().unwrap().dirty);
    }

    #[test]
    fn aura_disc_maps_angle_and_radius() {
        let mut app = app();
        *app.aura_disc.borrow_mut() = Some(Rect::new(0, 0, 40, 20));
        // 圆心偏右 → 0 度附近；越靠外饱和度越高
        app.pick_aura(38, 10);
        let (hue, sat) = app.aura_pick;
        assert!(hue < 10.0 || hue > 350.0, "右侧应为 0 度附近，实际 {hue}");
        assert!(sat > 0.8, "圆盘边缘接近满饱和，实际 {sat}");
        app.pick_aura(20, 10);
        assert!(app.aura_pick.1 < 0.2, "圆心附件饱和度应很低");
    }
}
