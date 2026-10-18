use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, BorderType, Clear, Gauge, Paragraph, Sparkline, Wrap};
use unicode_width::UnicodeWidthStr;

use crate::app::{AURA_COLORS, App, IpForm, Modal, MouseAction, MouseTarget, NetView, Tab};
use crate::asus::{self, GpuMode, Profile};
use crate::nm;
use crate::util;

const ACCENT: Color = Color::Rgb(90, 200, 220);
const DIM: Color = Color::Rgb(110, 118, 130);
const GOOD: Color = Color::Rgb(120, 210, 140);
const WARN: Color = Color::Rgb(240, 190, 90);
const BAD: Color = Color::Rgb(255, 110, 90);
const PANEL_BORDER: Color = Color::Rgb(62, 70, 82);
const SURFACE: Color = Color::Rgb(38, 45, 56);
const SURFACE_HOVER: Color = Color::Rgb(30, 36, 45);

pub fn draw(frame: &mut Frame, app: &App) {
    app.mouse_targets.borrow_mut().clear();
    let area = frame.area();
    let chunks = Layout::vertical([
        Constraint::Length(4),
        Constraint::Min(0),
        Constraint::Length(2),
    ])
    .split(area);

    draw_header(frame, chunks[0], app);

    let body = Layout::horizontal([Constraint::Length(23), Constraint::Min(0)]).split(chunks[1]);
    draw_sidebar(frame, body[0], app);
    draw_body(frame, body[1], app);
    let main_area = body[1];
    draw_footer(frame, chunks[2], app);

    if let Some(form) = &app.ip_form {
        draw_ip_form(frame, main_area, form);
    }

    if app.cache_open {
        draw_cache_modal(frame, area, app);
    }

    if let Some(modal) = &app.modal {
        draw_modal(frame, area, app, modal);
    }
}

fn draw_header(frame: &mut Frame, area: Rect, app: &App) {
    let profile = app
        .profile
        .map(|p| p.active)
        .map(|p| p.zh())
        .unwrap_or("未知");
    let profile_color = app.profile.map(|p| profile_color(p.active)).unwrap_or(DIM);
    let model = app
        .deps
        .product
        .replace("ASUS ", "")
        .split('_')
        .next()
        .unwrap_or("ASUS")
        .to_string();
    let battery = app
        .battery
        .capacity
        .map(|v| format!("{v}%"))
        .unwrap_or_else(|| "?".into());
    let limit = app
        .battery
        .limit
        .map(|v| format!("上限 {v}%"))
        .unwrap_or_else(|| "上限未知".into());
    let gpu = app.gpu_mode.map(|m| m.zh()).unwrap_or("未知");
    let net = app
        .ssid
        .clone()
        .or_else(|| {
            app.devices
                .iter()
                .find(|d| d.state.contains("connected") && d.kind != "loopback")
                .map(|d| d.connection.clone())
                .filter(|v| !v.is_empty())
        })
        .unwrap_or_else(|| "未连接".into());
    let temp = app
        .snap
        .max_temp
        .map(|v| format!("{v:.0}°C"))
        .unwrap_or_else(|| "-".into());

    let lines = vec![
        Line::from(vec![
            Span::styled(" RszTui ", Style::new().fg(Color::Black).bg(ACCENT).bold()),
            Span::raw("  "),
            Span::styled(model, Style::new().fg(Color::Gray).bold()),
            Span::styled(
                format!("   up {}", format_uptime(app.snap.uptime)),
                Style::new().fg(DIM),
            ),
        ]),
        Line::from(vec![
            Span::styled("  ● ", Style::new().fg(profile_color)),
            Span::styled(
                format!("{profile} "),
                Style::new().fg(Color::Black).bg(profile_color).bold(),
            ),
            Span::styled("  GPU ", Style::new().fg(DIM)),
            Span::styled(gpu, Style::new().fg(Color::Gray)),
            Span::styled("   电池 ", Style::new().fg(DIM)),
            Span::styled(battery, Style::new().fg(battery_color(app))),
            Span::styled(format!("  {limit}"), Style::new().fg(DIM)),
            Span::styled("   温度 ", Style::new().fg(DIM)),
            Span::styled(
                temp,
                Style::new().fg(temp_color(app.snap.max_temp.unwrap_or(0.0))),
            ),
            Span::styled("   网络 ", Style::new().fg(DIM)),
            Span::styled(net, Style::new().fg(Color::Gray)),
        ]),
    ];

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(PANEL_BORDER))
        .title(Line::from(vec![
            Span::styled(" 状态 ", Style::new().fg(ACCENT).bold()),
            Span::styled(
                match app.battery.ac_online {
                    Some(true) => "AC 供电",
                    Some(false) => "电池供电",
                    None => "",
                },
                Style::new().fg(DIM),
            ),
        ]));
    frame.render_widget(Paragraph::new(Text::from(lines)).block(block), area);
}

fn draw_sidebar(frame: &mut Frame, area: Rect, app: &App) {
    let mut lines: Vec<Line> = Vec::new();
    let mut line_idx: u16 = 0;

    for (index, tab) in Tab::ALL.iter().enumerate() {
        if index == 0 {
            lines.push(Line::from(Span::styled(
                " 控制",
                Style::new().fg(DIM).bold(),
            )));
            line_idx += 1;
        } else if index == 5 {
            lines.push(Line::raw(""));
            lines.push(Line::from(Span::styled(
                " 系统",
                Style::new().fg(DIM).bold(),
            )));
            line_idx += 2;
        }

        record_mouse(
            app,
            Rect::new(
                area.x + 1,
                area.y + 1 + line_idx,
                area.width.saturating_sub(2),
                1,
            ),
            MouseAction::Tab(*tab),
        );
        let selected = *tab == app.tab;
        let marker = if selected { "▶" } else { " " };
        let style = if selected {
            Style::new().fg(Color::Black).bg(ACCENT).bold()
        } else {
            Style::new().fg(Color::Gray)
        };
        lines.push(Line::from(Span::styled(
            format!(" {marker} {}  {}", index + 1, tab.zh()),
            style,
        )));
        line_idx += 1;
    }

    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        " 状态",
        Style::new().fg(DIM).bold(),
    )));
    lines.push(Line::from(vec![
        Span::styled(
            "  ● ",
            Style::new().fg(app.profile.map(|p| profile_color(p.active)).unwrap_or(DIM)),
        ),
        Span::styled(
            app.profile.map(|p| p.active.zh()).unwrap_or("未知"),
            Style::new().fg(Color::Gray),
        ),
    ]));
    lines.push(Line::from(vec![
        Span::styled("  ▰ ", Style::new().fg(battery_color(app))),
        Span::styled(
            app.battery
                .capacity
                .map(|v| format!("{v}%"))
                .unwrap_or_else(|| "?".into()),
            Style::new().fg(Color::Gray),
        ),
        Span::styled(
            app.battery
                .limit
                .map(|v| format!("  上限 {v}%"))
                .unwrap_or_default(),
            Style::new().fg(DIM),
        ),
    ]));
    lines.push(Line::from(vec![
        Span::styled("  ▣ ", Style::new().fg(ACCENT)),
        Span::styled(gpu_short(app), Style::new().fg(Color::Gray)),
    ]));
    lines.push(Line::from(vec![
        Span::styled("  ⌁ ", Style::new().fg(DIM)),
        Span::styled(
            app.ssid
                .clone()
                .or_else(|| {
                    app.devices
                        .iter()
                        .find(|d| d.state.contains("connected") && d.kind != "loopback")
                        .map(|d| d.connection.clone())
                        .filter(|v| !v.is_empty())
                })
                .unwrap_or_else(|| "未连接".into()),
            Style::new().fg(Color::Gray),
        ),
    ]));

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .title(Span::styled(" 导航 ", Style::new().fg(ACCENT).bold()))
        .border_style(Style::new().fg(PANEL_BORDER));
    frame.render_widget(Paragraph::new(Text::from(lines)).block(block), area);
}

fn gpu_short(app: &App) -> &'static str {
    match app.gpu_mode {
        Some(GpuMode::Integrated) => "集显",
        Some(GpuMode::Hybrid) => "混合",
        Some(GpuMode::Ultimate) => "独显",
        None => "未知",
    }
}

fn format_uptime(seconds: u64) -> String {
    let days = seconds / 86400;
    let hours = (seconds % 86400) / 3600;
    let mins = (seconds % 3600) / 60;
    if days > 0 {
        format!("{days}d {hours}h")
    } else {
        format!("{hours}h {mins}m")
    }
}

fn draw_body(frame: &mut Frame, area: Rect, app: &App) {
    match app.tab {
        Tab::Dashboard => draw_dashboard(frame, area, app),
        Tab::Performance => draw_performance(frame, area, app),
        Tab::Battery => draw_battery(frame, area, app),
        Tab::Fan => draw_fan(frame, area, app),
        Tab::Lighting => draw_lighting(frame, area, app),
        Tab::Network => draw_network(frame, area, app),
        Tab::Services => draw_services(frame, area, app),
        Tab::Settings => draw_settings(frame, area, app),
    }
}

fn draw_dashboard(frame: &mut Frame, area: Rect, app: &App) {
    let rows = Layout::vertical([
        Constraint::Ratio(1, 3),
        Constraint::Ratio(1, 3),
        Constraint::Ratio(1, 3),
    ])
    .split(area);

    let top = Layout::horizontal([Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)]).split(rows[0]);
    draw_cpu(frame, top[0], app);
    draw_memory(frame, top[1], app);

    let middle =
        Layout::horizontal([Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)]).split(rows[1]);
    draw_network_graph(frame, middle[0], app);
    draw_temps(frame, middle[1], app);

    let bottom =
        Layout::horizontal([Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)]).split(rows[2]);
    draw_battery_panel(frame, bottom[0], app);
    draw_asus_panel(frame, bottom[1], app);
}

fn draw_cpu(frame: &mut Frame, area: Rect, app: &App) {
    let block = panel(" CPU ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let parts = Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).split(inner);
    let line = Line::from(vec![
        Span::styled(
            format!(" {:.1}%", app.snap.cpu_pct),
            Style::new().fg(GOOD).bold(),
        ),
        Span::styled(
            format!("  负载 {:.2}", app.snap.load1),
            Style::new().fg(DIM),
        ),
    ]);
    frame.render_widget(Paragraph::new(line), parts[0]);

    let data: Vec<u64> = app.cpu_hist.iter().copied().collect();
    let spark = Sparkline::default()
        .data(&data)
        .max(100)
        .style(Style::new().fg(GOOD));
    frame.render_widget(spark, parts[1]);
}

fn draw_memory(frame: &mut Frame, area: Rect, app: &App) {
    let block = panel(" 内存 ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let ratio = if app.snap.mem_total > 0 {
        app.snap.mem_used as f64 / app.snap.mem_total as f64
    } else {
        0.0
    };
    let label = format!(
        "{} / {}",
        util::human_bytes(app.snap.mem_used as f64),
        util::human_bytes(app.snap.mem_total as f64)
    );
    let gauge = Gauge::default()
        .gauge_style(Style::new().fg(Color::Rgb(140, 160, 255)))
        .ratio(ratio.clamp(0.0, 1.0))
        .label(label);
    let parts = Layout::vertical([Constraint::Length(3), Constraint::Min(1)]).split(inner);
    frame.render_widget(gauge, parts[0]);

    let data: Vec<u64> = app.mem_hist.iter().copied().collect();
    let spark = Sparkline::default()
        .data(&data)
        .max(100)
        .style(Style::new().fg(Color::Rgb(140, 160, 255)));
    frame.render_widget(spark, parts[1]);
}

fn draw_network_graph(frame: &mut Frame, area: Rect, app: &App) {
    let block = panel(" 网络 ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let parts = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
        Constraint::Min(1),
    ])
    .split(inner);

    let header = Line::from(vec![
        Span::styled(" ↓ ", Style::new().fg(ACCENT)),
        Span::styled(
            util::human_rate(app.snap.rx_bps),
            Style::new().fg(Color::Gray),
        ),
        Span::raw("   "),
        Span::styled(" ↑ ", Style::new().fg(WARN)),
        Span::styled(
            util::human_rate(app.snap.tx_bps),
            Style::new().fg(Color::Gray),
        ),
    ]);
    frame.render_widget(Paragraph::new(header), parts[0]);

    let rx: Vec<u64> = app.rx_hist.iter().copied().collect();
    frame.render_widget(
        Sparkline::default()
            .data(&rx)
            .style(Style::new().fg(ACCENT)),
        parts[1],
    );

    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            " 上传",
            Style::new().fg(WARN).bold(),
        ))),
        parts[2],
    );
    let tx: Vec<u64> = app.tx_hist.iter().copied().collect();
    frame.render_widget(
        Sparkline::default().data(&tx).style(Style::new().fg(WARN)),
        parts[3],
    );
}

fn draw_temps(frame: &mut Frame, area: Rect, app: &App) {
    let block = panel(" 温度 ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if app.snap.temps.is_empty() {
        frame.render_widget(
            Paragraph::new(Span::styled("  没有可读的温度传感器", Style::new().fg(DIM))),
            inner,
        );
        return;
    }

    let lines: Vec<Line> = app
        .snap
        .temps
        .iter()
        .take(inner.height as usize)
        .map(|(label, value)| {
            Line::from(vec![
                Span::styled(
                    format!("  {:<28}", util::truncate(label, 28)),
                    Style::new().fg(Color::Gray),
                ),
                Span::styled(
                    format!("{value:>5.1}°C"),
                    Style::new().fg(temp_color(*value)).bold(),
                ),
            ])
        })
        .collect();
    frame.render_widget(Paragraph::new(Text::from(lines)), inner);
}

fn draw_battery_panel(frame: &mut Frame, area: Rect, app: &App) {
    let block = panel(" 电池 ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let parts = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .split(inner);

    let capacity = app.battery.capacity.unwrap_or(0);
    let gauge = Gauge::default()
        .gauge_style(Style::new().fg(battery_color(app)))
        .ratio(capacity as f64 / 100.0)
        .label(format!(
            "{}%  {}  {}",
            capacity,
            app.battery.status_zh(),
            match app.battery.ac_online {
                Some(true) => "AC",
                Some(false) => "电池",
                None => "",
            }
        ));
    frame.render_widget(gauge, parts[0]);

    let power = app
        .battery
        .power_w
        .map(|v| format!("{v:.1} W"))
        .unwrap_or_else(|| "-".into());
    let health = app
        .battery
        .health_pct()
        .map(|v| format!("{v:.1}%"))
        .unwrap_or_else(|| "-".into());
    let limit = app
        .battery
        .limit
        .map(|v| format!("{v}%"))
        .unwrap_or_else(|| "-".into());

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("  功率 ", Style::new().fg(DIM)),
            Span::styled(power, Style::new().fg(Color::Gray)),
            Span::styled("   健康 ", Style::new().fg(DIM)),
            Span::styled(health, Style::new().fg(Color::Gray)),
            Span::styled("   上限 ", Style::new().fg(DIM)),
            Span::styled(limit, Style::new().fg(Color::Gray)),
        ])),
        parts[1],
    );

    if let (Some(full), Some(design)) = (
        app.battery.energy_full_wh,
        app.battery.energy_full_design_wh,
    ) {
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("  容量 ", Style::new().fg(DIM)),
                Span::styled(
                    format!("{full:.1} / {design:.1} Wh"),
                    Style::new().fg(Color::Gray),
                ),
            ])),
            parts[2],
        );
    }

    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            format!(
                "  循环 {} 次   电压 {}",
                app.battery
                    .cycle_count
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "-".into()),
                app.battery
                    .voltage_v
                    .map(|v| format!("{v:.2} V"))
                    .unwrap_or_else(|| "-".into())
            ),
            Style::new().fg(DIM),
        ))),
        parts[3],
    );
}

fn draw_asus_panel(frame: &mut Frame, area: Rect, app: &App) {
    let block = panel(" ASUS ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines = Vec::new();
    if let Some(state) = app.profile {
        lines.push(Line::from(vec![
            Span::styled("  当前 ", Style::new().fg(DIM)),
            Span::styled(
                state.active.zh(),
                Style::new().fg(profile_color(state.active)).bold(),
            ),
            Span::styled(
                format!("   AC {} / 电池 {}", state.ac.zh(), state.battery.zh()),
                Style::new().fg(DIM),
            ),
        ]));
    }

    let power: Vec<String> = app
        .power
        .iter()
        .take(3)
        .map(|f| format!("{} {}", f.value, f.unit))
        .collect();
    lines.push(Line::from(vec![
        Span::styled("  功耗  ", Style::new().fg(DIM)),
        Span::styled(power.join(" / "), Style::new().fg(Color::Gray)),
    ]));

    let width = inner.width as usize;
    let cpu = format!(
        "{} ({}C/{}T)",
        app.hw.cpu_model, app.hw.cores, app.hw.threads
    );
    lines.push(Line::from(vec![
        Span::styled("  CPU   ", Style::new().fg(DIM)),
        Span::styled(
            util::truncate(&cpu, width.saturating_sub(8)),
            Style::new().fg(Color::Gray),
        ),
    ]));
    lines.push(Line::from(vec![
        Span::styled("  显卡  ", Style::new().fg(DIM)),
        Span::styled(
            util::truncate(&app.hw.gpus.join(" / "), width.saturating_sub(8)),
            Style::new().fg(Color::Gray),
        ),
    ]));

    if let Some(mode) = app.gpu_mode {
        lines.push(Line::from(vec![
            Span::styled("  模式  ", Style::new().fg(DIM)),
            Span::styled(mode.zh(), Style::new().fg(Color::Gray)),
        ]));
    }

    lines.push(Line::from(vec![
        Span::styled("  调参 ", Style::new().fg(DIM)),
        Span::styled(
            if app.tuning {
                "持久化开启"
            } else {
                "持久化关闭"
            },
            Style::new().fg(if app.tuning { GOOD } else { WARN }),
        ),
    ]));

    frame.render_widget(Paragraph::new(Text::from(lines)), inner);
}

fn draw_performance(frame: &mut Frame, area: Rect, app: &App) {
    let rows = Layout::vertical([Constraint::Length(3), Constraint::Min(0)]).split(area);
    draw_perf_summary(frame, rows[0], app);

    let cols =
        Layout::horizontal([Constraint::Percentage(38), Constraint::Percentage(62)]).split(rows[1]);

    let left = Layout::vertical([Constraint::Min(0), Constraint::Length(9)]).split(cols[0]);
    draw_profiles(frame, left[0], app);
    draw_gpu_picker(frame, left[1], app);

    let right = Layout::vertical([Constraint::Length(12), Constraint::Min(0)]).split(cols[1]);
    draw_presets(frame, right[0], app);
    draw_power_fields(frame, right[1], app);
}

fn draw_perf_summary(frame: &mut Frame, area: Rect, app: &App) {
    let block = panel(" 概览 ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let (active, ac, bat) = app.profile.map(|p| (p.active, p.ac, p.battery)).unwrap_or((
        Profile::Balanced,
        Profile::Balanced,
        Profile::Balanced,
    ));
    let gpu = app.gpu_mode.map(|m| m.zh()).unwrap_or("未知");
    let tuning = if app.tuning {
        "持久化开启"
    } else {
        "持久化关闭"
    };

    let line = Line::from(vec![
        Span::styled("  当前 ", Style::new().fg(DIM)),
        Span::styled(
            format!(" {} ", active.zh()),
            Style::new()
                .fg(Color::Black)
                .bg(profile_color(active))
                .bold(),
        ),
        Span::styled("   AC 默认 ", Style::new().fg(DIM)),
        Span::styled(ac.zh(), Style::new().fg(profile_color(ac)).bold()),
        Span::styled("   电池 ", Style::new().fg(DIM)),
        Span::styled(bat.zh(), Style::new().fg(profile_color(bat)).bold()),
        Span::styled("   GPU ", Style::new().fg(DIM)),
        Span::styled(gpu, Style::new().fg(Color::Gray)),
        Span::styled("   ", Style::new()),
        Span::styled(
            tuning,
            Style::new().fg(if app.tuning { GOOD } else { WARN }),
        ),
    ]);
    frame.render_widget(Paragraph::new(line), inner);
}

fn draw_profiles(frame: &mut Frame, area: Rect, app: &App) {
    let focused = app.perf_focus == crate::app::PerfFocus::Profiles;
    let block = panel_focused(" 性能模式 ", focused);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let state = app.profile;
    let mut lines = Vec::new();
    for (index, profile) in Profile::ALL.iter().enumerate() {
        record_mouse(
            app,
            Rect::new(inner.x, inner.y + index as u16, inner.width, 1),
            MouseAction::Profile(index),
        );
        let selected = index == app.profile_sel;
        let hovered = app.hover == Some(MouseAction::Profile(index));
        let active = state.map(|s| s.active == *profile).unwrap_or(false);
        let ac = state.map(|s| s.ac == *profile).unwrap_or(false);
        let battery = state.map(|s| s.battery == *profile).unwrap_or(false);

        let marker = if selected { "▶" } else { " " };
        let mut badges = Vec::new();
        if active {
            badges.push("当前");
        }
        if ac {
            badges.push("AC");
        }
        if battery {
            badges.push("电池");
        }
        let badge = if badges.is_empty() {
            String::new()
        } else {
            format!("  [{}]", badges.join(" "))
        };

        let style = row_style(selected, hovered);
        lines.push(Line::from(vec![
            Span::styled(format!(" {marker} "), style.fg(profile_color(*profile))),
            Span::styled(profile.zh().to_string(), style.fg(profile_color(*profile))),
            Span::styled(badge, style.fg(DIM)),
        ]));
    }

    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        " Enter 切换   a AC 默认   b 电池默认",
        Style::new().fg(DIM),
    )));
    lines.push(Line::from(Span::styled(
        " f 切换焦点到功耗",
        Style::new().fg(DIM),
    )));
    let y = inner.y + lines.len() as u16;
    if let Some(line) = action_bar(
        app,
        inner,
        y,
        &[
            ("切换", MouseAction::PerfSwitch),
            ("AC", MouseAction::PerfAc),
            ("电池", MouseAction::PerfBat),
        ],
    ) {
        lines.push(line);
    }

    frame.render_widget(
        Paragraph::new(Text::from(lines)).wrap(Wrap { trim: false }),
        inner,
    );
}

fn draw_gpu_picker(frame: &mut Frame, area: Rect, app: &App) {
    let block = panel(" GPU 模式 ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines = Vec::new();
    for (index, mode) in GpuMode::ALL.iter().enumerate() {
        record_mouse(
            app,
            Rect::new(inner.x, inner.y + index as u16, inner.width, 1),
            MouseAction::Gpu(index),
        );
        let selected = index == app.gpu_sel;
        let hovered = app.hover == Some(MouseAction::Gpu(index));
        let current = app.gpu_mode == Some(*mode);
        let marker = if selected { "▶" } else { " " };
        let badge = if current { "  [当前]" } else { "" };
        lines.push(Line::from(vec![
            Span::styled(
                format!(" {marker} {}", mode.zh()),
                if selected {
                    Style::new().bg(SURFACE).fg(ACCENT).bold()
                } else if hovered {
                    Style::new().bg(SURFACE_HOVER).fg(Color::Gray)
                } else {
                    Style::new().fg(Color::Gray)
                },
            ),
            Span::styled(badge, Style::new().fg(DIM)),
        ]));
    }
    lines.push(Line::from(Span::styled(
        " g 选择   G 应用（需重启）",
        Style::new().fg(DIM),
    )));

    frame.render_widget(Paragraph::new(Text::from(lines)), inner);
}

fn draw_presets(frame: &mut Frame, area: Rect, app: &App) {
    let block = panel(" 功耗预设 ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines = Vec::new();
    for (index, preset) in app.cfg.power_presets.iter().enumerate() {
        record_mouse(
            app,
            Rect::new(inner.x, inner.y + index as u16, inner.width, 1),
            MouseAction::PerfPreset(index),
        );
        let selected = index == app.preset_sel;
        let hovered = app.hover == Some(MouseAction::PerfPreset(index));
        let marker = if selected { "▶" } else { " " };
        let style = if selected {
            Style::new().bg(SURFACE).fg(ACCENT).bold()
        } else if hovered {
            Style::new().bg(SURFACE_HOVER).fg(Color::Gray)
        } else {
            Style::new().fg(Color::Gray)
        };
        lines.push(Line::from(vec![
            Span::styled(format!(" {marker} {:<4}", preset.name), style),
            Span::styled(
                format!(
                    "PL {} / {} / {} W   dGPU {} W   {}°C",
                    preset.pl1, preset.pl2, preset.pl3, preset.gpu_tgp, preset.gpu_temp
                ),
                if selected {
                    Style::new().fg(Color::Gray)
                } else {
                    Style::new().fg(DIM)
                },
            ),
        ]));
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        " [ ] 选择预设   p 套用",
        Style::new().fg(DIM),
    )));

    frame.render_widget(Paragraph::new(Text::from(lines)), inner);
}

fn draw_power_fields(frame: &mut Frame, area: Rect, app: &App) {
    let focused = app.perf_focus == crate::app::PerfFocus::Power;
    let block = panel_focused(" 功耗与 TGP ", focused);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines = Vec::new();
    for (index, field) in app.power.iter().enumerate() {
        record_mouse(
            app,
            Rect::new(inner.x, inner.y + index as u16, inner.width, 1),
            MouseAction::PowerField(index),
        );
        let selected = index == app.power_sel && focused;
        let style = if selected {
            Style::new()
                .bg(Color::Rgb(45, 52, 62))
                .fg(Color::White)
                .bold()
        } else {
            Style::new().fg(Color::Gray)
        };
        let marker = if selected { "▶" } else { " " };
        let range = format!("[{}..{}]", field.min, field.max);
        lines.push(Line::from(vec![
            Span::styled(format!(" {marker} {:<16}", field.label), style),
            Span::styled(
                format!("{:>4} {}", field.value, field.unit),
                style.fg(if selected { ACCENT } else { Color::Gray }),
            ),
            Span::styled(format!("  {range}"), Style::new().fg(DIM)),
        ]));
    }

    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        " h/l 调整（H/L 每次 5）   Enter/a 应用",
        Style::new().fg(DIM),
    )));
    lines.push(Line::from(Span::styled(
        " 写入当前供电模式的 tuning，重启由 asusd 恢复",
        Style::new().fg(DIM),
    )));
    let y = inner.y + lines.len() as u16;
    if let Some(line) = action_bar(
        app,
        inner,
        y,
        &[
            ("应用", MouseAction::PerfPowerApply),
            ("套用预设", MouseAction::PerfPresetApply),
        ],
    ) {
        lines.push(line);
    }

    frame.render_widget(
        Paragraph::new(Text::from(lines)).wrap(Wrap { trim: false }),
        inner,
    );
}

fn draw_battery(frame: &mut Frame, area: Rect, app: &App) {
    let block = panel(" 电池 ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let rows = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .split(inner);

    let capacity = app.battery.capacity.unwrap_or(0);
    frame.render_widget(
        Gauge::default()
            .gauge_style(Style::new().fg(battery_color(app)))
            .ratio(capacity as f64 / 100.0)
            .label(format!(
                "{}%  {}  {}",
                capacity,
                app.battery.status_zh(),
                match app.battery.ac_online {
                    Some(true) => "AC",
                    Some(false) => "电池",
                    None => "",
                }
            )),
        rows[0],
    );

    let info = |label: &'static str, value: String, color: Color| {
        Line::from(vec![
            Span::styled(format!("  {label:<8}"), Style::new().fg(DIM)),
            Span::styled(value, Style::new().fg(color)),
        ])
    };

    frame.render_widget(
        Paragraph::new(info(
            "上限",
            app.battery
                .limit
                .map(|v| format!("{v}%"))
                .unwrap_or_else(|| "未知".into()),
            Color::Gray,
        )),
        rows[1],
    );
    frame.render_widget(
        Paragraph::new(info(
            "功率",
            app.battery
                .power_w
                .map(|v| format!("{v:.1} W"))
                .unwrap_or_else(|| "-".into()),
            Color::Gray,
        )),
        rows[2],
    );
    frame.render_widget(
        Paragraph::new(info(
            "健康度",
            app.battery
                .health_pct()
                .map(|v| format!("{v:.1}%"))
                .unwrap_or_else(|| "-".into()),
            Color::Gray,
        )),
        rows[3],
    );
    frame.render_widget(
        Paragraph::new(info(
            "循环",
            app.battery
                .cycle_count
                .map(|v| v.to_string())
                .unwrap_or_else(|| "-".into()),
            Color::Gray,
        )),
        rows[4],
    );
    if let (Some(full), Some(design)) = (
        app.battery.energy_full_wh,
        app.battery.energy_full_design_wh,
    ) {
        frame.render_widget(
            Paragraph::new(info(
                "容量",
                format!("{full:.1} / {design:.1} Wh"),
                Color::Gray,
            )),
            rows[5],
        );
    }

    let limit_line = Line::from(vec![
        Span::styled("  充电上限  ", Style::new().fg(DIM)),
        Span::styled(
            format!("{}%", app.limit_edit),
            Style::new()
                .fg(if app.limit_dirty { WARN } else { GOOD })
                .bold(),
        ),
        Span::styled(
            if app.limit_dirty {
                "  未应用（h/l 调整，Enter 应用）"
            } else {
                "  h/l 调整，Enter 应用"
            },
            Style::new().fg(DIM),
        ),
    ]);
    frame.render_widget(Paragraph::new(limit_line), rows[6]);

    let mut hints = vec![
        Line::raw(""),
        Line::from(Span::styled(
            " o 单次充满（忽略上限）",
            Style::new().fg(DIM),
        )),
        Line::from(Span::styled(
            " 上限越低越省电池，长期插电建议 60-80%",
            Style::new().fg(DIM),
        )),
    ];
    if let Some(line) = action_bar(
        app,
        rows[7],
        rows[7].y + 3,
        &[
            (
                "-5",
                MouseAction::Key(KeyCode::Char('h'), KeyModifiers::NONE),
            ),
            (
                "+5",
                MouseAction::Key(KeyCode::Char('l'), KeyModifiers::NONE),
            ),
            ("应用", MouseAction::Key(KeyCode::Enter, KeyModifiers::NONE)),
            (
                "单次充满",
                MouseAction::Key(KeyCode::Char('o'), KeyModifiers::NONE),
            ),
        ],
    ) {
        hints.push(line);
    }
    frame.render_widget(Paragraph::new(Text::from(hints)), rows[7]);
}

fn draw_fan(frame: &mut Frame, area: Rect, app: &App) {
    let rows =
        Layout::vertical([Constraint::Percentage(56), Constraint::Percentage(44)]).split(area);
    draw_fan_graph(frame, rows[0], app);

    let cols =
        Layout::horizontal([Constraint::Percentage(58), Constraint::Percentage(42)]).split(rows[1]);
    if let Some(edit) = &app.fan_edit {
        draw_fan_editor(frame, cols[0], app, edit);
    } else {
        draw_fan_detail(frame, cols[0], app);
    }
    draw_fan_presets(frame, cols[1], app);
}

fn draw_fan_graph(frame: &mut Frame, area: Rect, app: &App) {
    use ratatui::widgets::canvas::{Canvas, Line as CanvasLine, Points};

    let cpu_color = Color::Rgb(90, 210, 230);
    let gpu_color = Color::Rgb(220, 130, 240);
    let grid = Color::Rgb(55, 62, 72);

    type FanPlot = (String, Vec<(u8, u8)>, Color, bool);
    let curves: Vec<FanPlot> = ["cpu", "gpu"]
        .iter()
        .filter_map(|fan| {
            let editing = app
                .fan_edit
                .as_ref()
                .is_some_and(|e| e.fan.eq_ignore_ascii_case(fan));
            let points: Vec<(u8, u8)> = if editing {
                let edit = app.fan_edit.as_ref()?;
                edit.temp
                    .iter()
                    .copied()
                    .zip(edit.pwm.iter().copied())
                    .collect()
            } else {
                let curve = app
                    .fan_curves
                    .iter()
                    .find(|c| c.fan.eq_ignore_ascii_case(fan))?;
                curve
                    .temp
                    .iter()
                    .copied()
                    .zip(curve.pwm.iter().copied())
                    .collect()
            };
            let color = if *fan == "cpu" { cpu_color } else { gpu_color };
            Some((fan.to_uppercase(), points, color, editing))
        })
        .collect();

    let title = Line::from(vec![
        Span::styled(" 风扇曲线 ", Style::new().fg(ACCENT).bold()),
        Span::styled("CPU ", Style::new().fg(cpu_color)),
        Span::styled("GPU ", Style::new().fg(gpu_color)),
        Span::styled(" 横轴温度 / 纵轴 PWM", Style::new().fg(DIM)),
    ]);

    let canvas = Canvas::default()
        .block(panel(title))
        .x_bounds([20.0, 100.0])
        .y_bounds([0.0, 255.0])
        .paint(|ctx| {
            for y in [0.0, 64.0, 128.0, 192.0, 255.0] {
                ctx.draw(&CanvasLine {
                    x1: 20.0,
                    y1: y,
                    x2: 100.0,
                    y2: y,
                    color: grid,
                });
            }
            for x in [20.0, 40.0, 60.0, 80.0, 100.0] {
                ctx.draw(&CanvasLine {
                    x1: x,
                    y1: 0.0,
                    x2: x,
                    y2: 255.0,
                    color: grid,
                });
            }
            ctx.print(21.5, 238.0, "255");
            ctx.print(21.5, 4.0, "0");
            ctx.print(95.0, 4.0, "°C");

            for (_name, points, color, editing) in &curves {
                let coords: Vec<(f64, f64)> = points
                    .iter()
                    .map(|(temp, pwm)| (*temp as f64, *pwm as f64))
                    .collect();
                for pair in coords.windows(2) {
                    ctx.draw(&CanvasLine {
                        x1: pair[0].0,
                        y1: pair[0].1,
                        x2: pair[1].0,
                        y2: pair[1].1,
                        color: *color,
                    });
                }
                ctx.draw(&Points {
                    coords: &coords,
                    color: if *editing { Color::White } else { *color },
                });
            }
        });

    frame.render_widget(canvas, area);
}

fn draw_fan_detail(frame: &mut Frame, area: Rect, app: &App) {
    let block = panel(" 曲线明细 ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines = Vec::new();
    for curve in &app.fan_curves {
        let color = if curve.fan.eq_ignore_ascii_case("cpu") {
            Color::Rgb(90, 210, 230)
        } else {
            Color::Rgb(220, 130, 240)
        };
        lines.push(Line::from(vec![
            Span::styled(
                format!("  {:<4} ", curve.fan),
                Style::new().fg(color).bold(),
            ),
            Span::styled(
                if curve.enabled { "启用" } else { "停用" },
                Style::new().fg(if curve.enabled { GOOD } else { BAD }),
            ),
        ]));
        let points: Vec<String> = curve
            .temp
            .iter()
            .zip(curve.pwm.iter())
            .map(|(temp, pwm)| format!("{temp}°C {pwm}/{}%", (*pwm as u32 * 100 / 255)))
            .collect();
        for chunk in points.chunks(4) {
            lines.push(Line::from(Span::styled(
                format!("    {}", chunk.join("   ")),
                Style::new().fg(Color::Gray),
            )));
        }
        lines.push(Line::raw(""));
    }

    lines.push(Line::from(Span::styled(
        " i 编辑 CPU   I 编辑 GPU   e 启用/停用",
        Style::new().fg(DIM),
    )));
    lines.push(Line::from(Span::styled(
        " d 恢复固件默认   s 保存当前为预设",
        Style::new().fg(DIM),
    )));
    let y = inner.y + lines.len() as u16;
    if let Some(line) = action_bar(
        app,
        inner,
        y,
        &[
            (
                "编辑CPU",
                MouseAction::Key(KeyCode::Char('i'), KeyModifiers::NONE),
            ),
            (
                "编辑GPU",
                MouseAction::Key(KeyCode::Char('I'), KeyModifiers::NONE),
            ),
            (
                "应用预设",
                MouseAction::Key(KeyCode::Char('p'), KeyModifiers::NONE),
            ),
            (
                "保存",
                MouseAction::Key(KeyCode::Char('s'), KeyModifiers::NONE),
            ),
            (
                "启停",
                MouseAction::Key(KeyCode::Char('e'), KeyModifiers::NONE),
            ),
        ],
    ) {
        lines.push(line);
    }

    frame.render_widget(
        Paragraph::new(Text::from(lines)).wrap(Wrap { trim: false }),
        inner,
    );
}

fn draw_fan_editor(frame: &mut Frame, area: Rect, app: &App, edit: &crate::app::FanEdit) {
    let title = format!(
        " 编辑 {} 曲线{} ",
        edit.fan.to_uppercase(),
        if edit.dirty { " *" } else { "" }
    );
    let block = panel(title);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let rows = Layout::vertical([Constraint::Min(0), Constraint::Length(2)]).split(inner);
    let mut lines = Vec::new();
    for index in 0..edit.temp.len() {
        record_mouse(
            app,
            Rect::new(rows[0].x, rows[0].y + index as u16, rows[0].width, 1),
            MouseAction::FanPoint(index),
        );
        let selected = index == edit.sel;
        let hovered = app.hover == Some(MouseAction::FanPoint(index));
        let style =
            row_style(selected, hovered).fg(if selected { Color::White } else { Color::Gray });
        let marker = if selected { "▶" } else { " " };
        lines.push(Line::from(vec![
            Span::styled(format!(" {marker} 点{}  ", index + 1), style),
            Span::styled(format!("{:>3}°C", edit.temp[index]), style),
            Span::styled(format!("  PWM {:>3}", edit.pwm[index]), style),
            Span::styled(
                format!("  {:>3}%", edit.percent(index)),
                style.fg(if selected { ACCENT } else { DIM }),
            ),
        ]));
    }

    frame.render_widget(
        Paragraph::new(Text::from(lines)).wrap(Wrap { trim: false }),
        rows[0],
    );
    frame.render_widget(
        Paragraph::new(Text::from(vec![
            Line::from(Span::styled(
                " j/k 选点   h/l PWM ±1   H/L ±10   [ ] 温度 ±1",
                Style::new().fg(DIM),
            )),
            Line::from(Span::styled(
                " c 切 CPU/GPU   a 写入   Esc 取消",
                Style::new().fg(DIM),
            )),
        ])),
        rows[1],
    );
}

fn draw_fan_presets(frame: &mut Frame, area: Rect, app: &App) {
    let block = panel(" 风扇预设 ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines = Vec::new();
    if app.cfg.fan_presets.is_empty() {
        lines.push(Line::from(Span::styled(
            "  还没有预设",
            Style::new().fg(DIM),
        )));
        lines.push(Line::raw(""));
        lines.push(Line::from(Span::styled(
            "  按 s 把当前已写入的曲线保存为预设",
            Style::new().fg(DIM),
        )));
    } else {
        for (index, preset) in app.cfg.fan_presets.iter().enumerate() {
            record_mouse(
                app,
                Rect::new(inner.x, inner.y + index as u16, inner.width, 1),
                MouseAction::FanPreset(index),
            );
            let selected = index == app.fan_sel;
            let hovered = app.hover == Some(MouseAction::FanPreset(index));
            let marker = if selected { "▶" } else { " " };
            lines.push(Line::from(vec![
                Span::styled(
                    format!(" {marker} {}", preset.name),
                    if selected {
                        Style::new().bg(SURFACE).fg(ACCENT).bold()
                    } else if hovered {
                        Style::new().bg(SURFACE_HOVER).fg(Color::Gray)
                    } else {
                        Style::new().fg(Color::Gray)
                    },
                ),
                Span::styled(format!("  [{}]", preset.profile.zh()), Style::new().fg(DIM)),
            ]));
        }
        lines.push(Line::raw(""));
        lines.push(Line::from(Span::styled(
            " p 应用   x 删除   s 覆盖保存",
            Style::new().fg(DIM),
        )));
    }

    frame.render_widget(Paragraph::new(Text::from(lines)), inner);
}

fn draw_lighting(frame: &mut Frame, area: Rect, app: &App) {
    let rows = Layout::vertical([Constraint::Length(11), Constraint::Min(0)]).split(area);
    let cols =
        Layout::horizontal([Constraint::Percentage(46), Constraint::Percentage(54)]).split(rows[0]);

    draw_lighting_status(frame, cols[0], app);
    draw_lighting_colors(frame, cols[1], app);
    draw_lighting_modes(frame, rows[1], app);
}

fn draw_lighting_status(frame: &mut Frame, area: Rect, app: &App) {
    let block = panel(" 当前灯效 ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines = Vec::new();
    if let Some(aura) = &app.aura {
        lines.push(Line::from(vec![
            Span::styled("  模式  ", Style::new().fg(DIM)),
            Span::styled(
                format!("{}（{}）", aura.mode_name, aura.mode_zh),
                Style::new().fg(ACCENT).bold(),
            ),
        ]));
        lines.push(Line::from(vec![
            Span::styled("  亮度  ", Style::new().fg(DIM)),
            Span::styled(aura.brightness_name.clone(), Style::new().fg(GOOD)),
        ]));
        lines.push(Line::from(vec![
            Span::styled("  颜色  ", Style::new().fg(DIM)),
            Span::styled(format!("{} ", aura.color1), Style::new().fg(Color::Gray)),
            Span::styled(
                "████",
                Style::new().fg(hex_color(aura.color1.trim_start_matches('#'))),
            ),
        ]));
        if aura.color2 != "#000000" {
            lines.push(Line::from(vec![
                Span::styled("  副色  ", Style::new().fg(DIM)),
                Span::styled(format!("{} ", aura.color2), Style::new().fg(Color::Gray)),
                Span::styled(
                    "████",
                    Style::new().fg(hex_color(aura.color2.trim_start_matches('#'))),
                ),
            ]));
        }
        if !aura.speed.is_empty() {
            lines.push(Line::from(vec![
                Span::styled("  速度  ", Style::new().fg(DIM)),
                Span::styled(aura.speed.clone(), Style::new().fg(Color::Gray)),
            ]));
        }
        if !aura.zone.is_empty() {
            lines.push(Line::from(vec![
                Span::styled("  区域  ", Style::new().fg(DIM)),
                Span::styled(aura.zone.clone(), Style::new().fg(Color::Gray)),
            ]));
        }
    } else {
        lines.push(Line::from(vec![
            Span::styled("  当前亮度  ", Style::new().fg(DIM)),
            Span::styled(app.led.clone(), Style::new().fg(ACCENT).bold()),
        ]));
        lines.push(Line::from(Span::styled(
            "  无法读取 Aura 状态，键盘可能只支持亮度。",
            Style::new().fg(WARN),
        )));
    }

    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        "  b 亮度   e/E 灯效   c 选色   S 应用静态色",
        Style::new().fg(DIM),
    )));
    let y = inner.y + lines.len() as u16;
    if let Some(line) = action_bar(
        app,
        inner,
        y,
        &[
            (
                "亮度",
                MouseAction::Key(KeyCode::Char('b'), KeyModifiers::NONE),
            ),
            (
                "上个",
                MouseAction::Key(KeyCode::Char('E'), KeyModifiers::NONE),
            ),
            (
                "下个",
                MouseAction::Key(KeyCode::Char('e'), KeyModifiers::NONE),
            ),
            (
                "应用色",
                MouseAction::Key(KeyCode::Char('S'), KeyModifiers::NONE),
            ),
        ],
    ) {
        lines.push(line);
    }

    frame.render_widget(
        Paragraph::new(Text::from(lines)).wrap(Wrap { trim: false }),
        inner,
    );
}

fn draw_lighting_colors(frame: &mut Frame, area: Rect, app: &App) {
    let block = panel(" 静态颜色 ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines = Vec::new();
    let mut row = Vec::new();
    for (index, (name, hex)) in AURA_COLORS.iter().enumerate() {
        record_mouse(
            app,
            Rect::new(
                inner.x + (index % 3) as u16 * 10,
                inner.y + (index / 3) as u16,
                10,
                1,
            ),
            MouseAction::Color(index),
        );
        let selected = index == app.aura_color;
        let hovered = app.hover == Some(MouseAction::Color(index));
        row.push(Span::styled(
            format!(" {} ", if selected { "▶" } else { " " }),
            if selected {
                Style::new().bg(SURFACE).fg(ACCENT)
            } else if hovered {
                Style::new().bg(SURFACE_HOVER).fg(DIM)
            } else {
                Style::new().fg(DIM)
            },
        ));
        row.push(Span::styled("██ ", Style::new().fg(hex_color(hex))));
        row.push(Span::styled(
            format!("{:<4}", name),
            if selected {
                Style::new().fg(ACCENT).bold()
            } else {
                Style::new().fg(Color::Gray)
            },
        ));
        if row.len() >= 9 {
            lines.push(Line::from(std::mem::take(&mut row)));
        }
    }
    if !row.is_empty() {
        lines.push(Line::from(row));
    }

    lines.push(Line::raw(""));
    if let Some(aura) = &app.aura {
        lines.push(Line::from(vec![
            Span::styled("  当前颜色  ", Style::new().fg(DIM)),
            Span::styled(format!("{} ", aura.color1), Style::new().fg(Color::Gray)),
            Span::styled(
                "████████",
                Style::new().fg(hex_color(aura.color1.trim_start_matches('#'))),
            ),
        ]));
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        "  c 切换选中颜色   S 应用为静态色",
        Style::new().fg(DIM),
    )));
    lines.push(Line::from(Span::styled(
        "  静态色只在 Static 灯效下生效",
        Style::new().fg(DIM),
    )));

    frame.render_widget(
        Paragraph::new(Text::from(lines)).wrap(Wrap { trim: false }),
        inner,
    );
}

fn draw_lighting_modes(frame: &mut Frame, area: Rect, app: &App) {
    let block = panel(" 支持的灯效 ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let supported: Vec<u32> = app
        .aura
        .as_ref()
        .map(|aura| {
            if aura.supported.is_empty() {
                vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 10, 11, 12]
            } else {
                aura.supported.clone()
            }
        })
        .unwrap_or_else(|| vec![0, 1, 2, 3, 10]);
    let current = app.aura.as_ref().map(|aura| aura.mode);

    let mut lines = Vec::new();
    for mode in supported {
        let active = current == Some(mode);
        lines.push(Line::from(vec![
            Span::styled(
                format!(" {} ", if active { "▶" } else { " " }),
                Style::new().fg(if active { ACCENT } else { DIM }),
            ),
            Span::styled(
                format!("{:<14}", asus::aura_mode_name(mode)),
                if active {
                    Style::new().fg(ACCENT).bold()
                } else {
                    Style::new().fg(Color::Gray)
                },
            ),
            Span::styled(
                asus::aura_mode_zh(mode),
                if active {
                    Style::new().fg(ACCENT)
                } else {
                    Style::new().fg(DIM)
                },
            ),
            if active {
                Span::styled("   [当前]", Style::new().fg(GOOD))
            } else {
                Span::raw("")
            },
        ]));
    }

    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        "  e 下一个   E 上一个",
        Style::new().fg(DIM),
    )));
    lines.push(Line::from(Span::styled(
        "  灯效由键盘固件决定，部分模式不支持颜色/速度调节",
        Style::new().fg(DIM),
    )));

    frame.render_widget(
        Paragraph::new(Text::from(lines)).wrap(Wrap { trim: false }),
        inner,
    );
}

fn draw_network(frame: &mut Frame, area: Rect, app: &App) {
    let block = panel(" 网络 ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let rows = Layout::vertical([Constraint::Length(2), Constraint::Min(0)]).split(inner);
    let header = Line::from(vec![
        Span::styled(" Wi-Fi ", Style::new().fg(DIM)),
        Span::styled(
            match app.wifi_enabled {
                Some(true) => "已启用",
                Some(false) => "已禁用",
                None => "未知",
            },
            Style::new().fg(if app.wifi_enabled == Some(true) {
                GOOD
            } else {
                WARN
            }),
        ),
        Span::styled("   SSID ", Style::new().fg(DIM)),
        Span::styled(
            app.ssid.clone().unwrap_or_else(|| "-".into()),
            Style::new().fg(Color::Gray),
        ),
        if nm::hotspot_active() {
            Span::styled("   热点已开启", Style::new().fg(WARN))
        } else {
            Span::raw("")
        },
        if app.net_loading {
            Span::styled("   刷新中…", Style::new().fg(WARN))
        } else {
            Span::raw("")
        },
    ]);
    frame.render_widget(Paragraph::new(header), rows[0]);

    let cols =
        Layout::horizontal([Constraint::Percentage(56), Constraint::Percentage(44)]).split(rows[1]);
    draw_net_list(frame, cols[0], app);
    draw_net_details(frame, cols[1], app);
}

fn draw_net_list(frame: &mut Frame, area: Rect, app: &App) {
    let tabs: Vec<Span> = NetView::ALL
        .iter()
        .map(|view| {
            Span::styled(
                format!(" {} ", view.zh()),
                if *view == app.net_view {
                    Style::new().fg(Color::Black).bg(ACCENT).bold()
                } else {
                    Style::new().fg(DIM)
                },
            )
        })
        .collect();

    let block = panel(" 列表 ");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let rows = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).split(inner);
    frame.render_widget(Paragraph::new(Line::from(tabs)), rows[0]);

    let mut lines = Vec::new();
    match app.net_view {
        NetView::Connections => {
            for (index, conn) in app.connections.iter().enumerate() {
                record_mouse(
                    app,
                    Rect::new(rows[1].x, rows[1].y + index as u16, rows[1].width, 1),
                    MouseAction::Net(index),
                );
                let selected = index == app.net_sel;
                let hovered = app.hover == Some(MouseAction::Net(index));
                let marker = if selected { "▶" } else { " " };
                let active = if conn.active {
                    Span::styled(" [已连接]", Style::new().fg(GOOD))
                } else {
                    Span::raw("")
                };
                lines.push(Line::from(vec![
                    Span::styled(
                        format!(" {marker} {}", util::truncate(&conn.name, 20)),
                        if selected {
                            Style::new().bg(SURFACE).fg(ACCENT).bold()
                        } else if hovered {
                            Style::new().bg(SURFACE_HOVER).fg(Color::Gray)
                        } else {
                            Style::new().fg(Color::Gray)
                        },
                    ),
                    Span::styled(
                        format!("  {}", util::truncate(short_kind(&conn.kind), 14)),
                        Style::new().fg(DIM),
                    ),
                    active,
                ]));
            }
        }
        NetView::Wifi => {
            for (index, ap) in app.wifi.iter().enumerate() {
                record_mouse(
                    app,
                    Rect::new(rows[1].x, rows[1].y + index as u16, rows[1].width, 1),
                    MouseAction::Net(index),
                );
                let selected = index == app.net_sel;
                let hovered = app.hover == Some(MouseAction::Net(index));
                let marker = if selected { "▶" } else { " " };
                let bars = signal_bar(ap.signal);
                let in_use = if ap.in_use {
                    Span::styled(" [已连接]", Style::new().fg(GOOD))
                } else {
                    Span::raw("")
                };
                lines.push(Line::from(vec![
                    Span::styled(
                        format!(" {marker} {}", util::truncate(&ap.ssid, 24)),
                        if selected {
                            Style::new().bg(SURFACE).fg(ACCENT).bold()
                        } else if hovered {
                            Style::new().bg(SURFACE_HOVER).fg(Color::Gray)
                        } else {
                            Style::new().fg(Color::Gray)
                        },
                    ),
                    Span::styled(
                        format!("  {bars} {:>3}%", ap.signal),
                        Style::new().fg(signal_color(ap.signal)),
                    ),
                    Span::styled(
                        format!("  {}", util::truncate(&ap.security, 12)),
                        Style::new().fg(DIM),
                    ),
                    in_use,
                ]));
            }
        }
        NetView::Vpn => {
            for (index, vpn) in app.vpns.iter().enumerate() {
                record_mouse(
                    app,
                    Rect::new(rows[1].x, rows[1].y + index as u16, rows[1].width, 1),
                    MouseAction::Net(index),
                );
                let selected = index == app.net_sel;
                let hovered = app.hover == Some(MouseAction::Net(index));
                let marker = if selected { "▶" } else { " " };
                let active = if vpn.active {
                    Span::styled(" [已连接]", Style::new().fg(GOOD))
                } else {
                    Span::styled(" [未连接]", Style::new().fg(DIM))
                };
                lines.push(Line::from(vec![
                    Span::styled(
                        format!(" {marker} {}", util::truncate(&vpn.name, 26)),
                        if selected {
                            Style::new().bg(SURFACE).fg(ACCENT).bold()
                        } else if hovered {
                            Style::new().bg(SURFACE_HOVER).fg(Color::Gray)
                        } else {
                            Style::new().fg(Color::Gray)
                        },
                    ),
                    active,
                ]));
            }
        }
    }

    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            "  没有条目，按 s 扫描或检查 NetworkManager",
            Style::new().fg(DIM),
        )));
    }

    frame.render_widget(
        Paragraph::new(Text::from(lines)).wrap(Wrap { trim: false }),
        rows[1],
    );
}

fn draw_net_details(frame: &mut Frame, area: Rect, app: &App) {
    let block = panel(" 详情 ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines = Vec::new();
    match app.net_view {
        NetView::Connections => {
            if let Some(conn) = app.connections.get(app.net_sel) {
                lines.push(section("链路"));
                lines.push(detail("名称", conn.name.clone()));
                lines.push(detail("类型", short_kind(&conn.kind).to_string()));
                lines.push(detail(
                    "设备",
                    if conn.device.is_empty() {
                        "-".into()
                    } else {
                        conn.device.clone()
                    },
                ));
                lines.push(detail(
                    "状态",
                    if conn.active {
                        "已连接"
                    } else {
                        "未连接"
                    }
                    .into(),
                ));
                if let Some(dev) = &app.net_detail {
                    if !dev.hwaddr.is_empty() {
                        lines.push(detail("MAC", dev.hwaddr.clone()));
                    }
                    if !dev.mtu.is_empty() {
                        lines.push(detail("MTU", dev.mtu.clone()));
                    }
                    if !dev.speed.is_empty() {
                        lines.push(detail("速率", format!("{} Mbps", dev.speed)));
                    }
                    if !dev.routes.is_empty() {
                        lines.push(detail("路由", util::truncate(&dev.routes, 60)));
                    }
                }
                lines.push(Line::raw(""));
                lines.push(section("IPv4"));
                if let Some(ip) = &app.net_ipv4 {
                    lines.push(detail(
                        "IPv4",
                        if ip.method == "manual" {
                            format!(
                                "手动  {}",
                                if ip.addresses.is_empty() {
                                    "-"
                                } else {
                                    &ip.addresses
                                }
                            )
                        } else {
                            "自动 (DHCP)".into()
                        },
                    ));
                    if !ip.gateway.is_empty() {
                        lines.push(detail("网关", ip.gateway.clone()));
                    }
                    if !ip.dns.is_empty() {
                        lines.push(detail("DNS", ip.dns.clone()));
                    }
                    if ip.dot != -1 {
                        lines.push(detail(
                            "DoT",
                            match ip.dot {
                                0 => "关闭",
                                1 => "机会性",
                                2 => "强制",
                                _ => "默认",
                            }
                            .into(),
                        ));
                    }
                }
                lines.push(Line::raw(""));
                lines.push(section("操作"));
                lines.push(Line::from(Span::styled(
                    " Enter 编辑 IPv4 / DNS",
                    Style::new().fg(DIM),
                )));
                lines.push(Line::from(Span::styled(
                    " u 激活   d 断开   x 删除",
                    Style::new().fg(DIM),
                )));
                lines.push(Line::from(Span::styled(
                    " p Ping   n DNS 查询   c 缓存",
                    Style::new().fg(DIM),
                )));
            }
        }
        NetView::Wifi => {
            if let Some(ap) = app.wifi.get(app.net_sel) {
                lines.push(section("Wi-Fi"));
                lines.push(detail("SSID", ap.ssid.clone()));
                lines.push(detail("BSSID", ap.bssid.clone()));
                lines.push(detail("信号", format!("{}%", ap.signal)));
                lines.push(detail("信道", format!("{}  {}", ap.chan, ap.freq)));
                lines.push(detail("速率", ap.rate.clone()));
                lines.push(detail(
                    "加密",
                    if ap.security.is_empty() {
                        "开放".into()
                    } else {
                        ap.security.clone()
                    },
                ));
                lines.push(detail(
                    "状态",
                    if ap.in_use { "已连接" } else { "未连接" }.into(),
                ));
                lines.push(Line::raw(""));
                lines.push(section("操作"));
                lines.push(Line::from(Span::styled(
                    " Enter 连接（密码写入 NM keyfile）",
                    Style::new().fg(DIM),
                )));
                lines.push(Line::from(Span::styled(
                    " s 重新扫描   p Ping   n DNS 查询   c 缓存",
                    Style::new().fg(DIM),
                )));
            }
        }
        NetView::Vpn => {
            if let Some(vpn) = app.vpns.get(app.net_sel) {
                lines.push(section("VPN"));
                lines.push(detail("名称", vpn.name.clone()));
                lines.push(detail(
                    "状态",
                    if vpn.active { "已连接" } else { "未连接" }.into(),
                ));
                lines.push(Line::raw(""));
                lines.push(section("操作"));
                lines.push(Line::from(Span::styled(
                    " Enter/u 连接   d 断开",
                    Style::new().fg(DIM),
                )));
            } else {
                lines.push(Line::from(Span::styled(
                    " 没有 VPN 连接",
                    Style::new().fg(DIM),
                )));
            }
        }
    }

    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        " v 视图   o 无线   t 热点",
        Style::new().fg(DIM),
    )));

    let bar_y = inner.y + lines.len() as u16;
    if bar_y < inner.bottom() {
        for (offset, width, action) in [
            (0, 8, MouseAction::NetPing),
            (8, 8, MouseAction::NetDns),
            (16, 10, MouseAction::NetCache),
            (26, 10, MouseAction::NetRefresh),
        ] {
            record_mouse(app, Rect::new(inner.x + offset, bar_y, width, 1), action);
        }
        lines.push(Line::from(vec![
            Span::styled(" [Ping]", Style::new().fg(ACCENT)),
            Span::styled(" [DNS]", Style::new().fg(ACCENT)),
            Span::styled(" [缓存]", Style::new().fg(ACCENT)),
            Span::styled(" [刷新]", Style::new().fg(ACCENT)),
        ]));
    }

    frame.render_widget(
        Paragraph::new(Text::from(lines)).wrap(Wrap { trim: false }),
        inner,
    );
}

fn draw_ip_form(frame: &mut Frame, area: Rect, form: &IpForm) {
    let vertical = Layout::vertical([
        Constraint::Percentage(22),
        Constraint::Percentage(56),
        Constraint::Percentage(22),
    ])
    .split(area);
    let horizontal = Layout::horizontal([
        Constraint::Percentage(10),
        Constraint::Percentage(80),
        Constraint::Percentage(10),
    ])
    .split(vertical[1]);
    let popup = horizontal[1];

    frame.render_widget(Clear, popup);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .title(Span::styled(
            format!(" 编辑 IPv4 · {} ", form.conn),
            Style::new().fg(ACCENT).bold(),
        ))
        .border_style(Style::new().fg(ACCENT));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let rows = Layout::vertical([Constraint::Min(0), Constraint::Length(2)]).split(inner);
    let mut lines = Vec::new();

    for (index, label) in IpForm::LABELS.iter().enumerate() {
        let selected = index == form.sel;
        let marker = if selected { "▶" } else { " " };
        let style = if selected {
            Style::new()
                .bg(Color::Rgb(45, 52, 62))
                .fg(Color::White)
                .bold()
        } else {
            Style::new().fg(Color::Gray)
        };

        let mut value = form.value(index);
        if index == 0 {
            value = if value == "manual" {
                "手动 (manual)".into()
            } else {
                "自动 (DHCP)".into()
            };
        }
        if form.editable(index) && selected {
            value.push('▏');
        }

        lines.push(Line::from(vec![
            Span::styled(format!(" {marker} {:<10}", label), style),
            Span::styled(
                format!("  {value}"),
                style.fg(if selected { ACCENT } else { Color::Gray }),
            ),
        ]));
        if form.method == "auto" && (1..=3).contains(&index) {
            lines.push(Line::from(Span::styled(
                "              切换到手动后可编辑",
                Style::new().fg(DIM),
            )));
        }
    }

    frame.render_widget(
        Paragraph::new(Text::from(lines)).wrap(Wrap { trim: false }),
        rows[0],
    );
    frame.render_widget(
        Paragraph::new(Text::from(vec![
            Line::from(Span::styled(
                " j/k 选择   Enter 切换/下一项   直接输入编辑   Backspace 删除",
                Style::new().fg(DIM),
            )),
            Line::from(Span::styled(
                " a 应用   Esc 取消   Ctrl+U 清空当前项",
                Style::new().fg(DIM),
            )),
        ])),
        rows[1],
    );
}

fn action_bar(
    app: &App,
    inner: Rect,
    y: u16,
    items: &[(&str, MouseAction)],
) -> Option<Line<'static>> {
    if y >= inner.bottom() {
        return None;
    }
    let mut x = inner.x;
    let mut spans = Vec::new();
    for (label, action) in items {
        let width = UnicodeWidthStr::width(*label) as u16;
        if x + width + 2 > inner.right() {
            break;
        }
        record_mouse(app, Rect::new(x, y, width + 2, 1), *action);
        spans.push(Span::styled(format!(" {label} "), Style::new().fg(ACCENT)));
        x += width + 2;
    }
    Some(Line::from(spans))
}

fn row_style(selected: bool, hovered: bool) -> Style {
    if selected {
        Style::new().bg(SURFACE).fg(Color::White).bold()
    } else if hovered {
        Style::new().bg(SURFACE_HOVER)
    } else {
        Style::new()
    }
}

fn record_mouse(app: &App, rect: Rect, action: MouseAction) {
    app.mouse_targets
        .borrow_mut()
        .push(MouseTarget { rect, action });
}

fn section(title: &'static str) -> Line<'static> {
    Line::from(Span::styled(
        format!("  {title}"),
        Style::new().fg(ACCENT).bold(),
    ))
}

fn detail(label: &'static str, value: String) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("  {label:<6}"), Style::new().fg(DIM)),
        Span::styled(util::truncate(&value, 34), Style::new().fg(Color::Gray)),
    ])
}

fn short_kind(kind: &str) -> &str {
    if kind.contains("ethernet") {
        "ethernet"
    } else if kind.contains("wireless") {
        "wifi"
    } else if kind.contains("wireguard") {
        "wireguard"
    } else if kind.contains("vpn") {
        "vpn"
    } else {
        kind
    }
}

fn signal_bar(signal: u8) -> String {
    let filled = ((signal as usize + 9) / 20).min(5);
    format!("{}{}", "▮".repeat(filled), "▯".repeat(5 - filled))
}

fn signal_color(signal: u8) -> Color {
    if signal >= 70 {
        GOOD
    } else if signal >= 40 {
        WARN
    } else {
        BAD
    }
}

fn hex_color(hex: &str) -> Color {
    let value = u32::from_str_radix(hex, 16).unwrap_or(0xffffff);
    Color::Rgb(
        ((value >> 16) & 0xff) as u8,
        ((value >> 8) & 0xff) as u8,
        (value & 0xff) as u8,
    )
}

fn draw_services(frame: &mut Frame, area: Rect, app: &App) {
    let cols =
        Layout::horizontal([Constraint::Percentage(58), Constraint::Percentage(42)]).split(area);

    let block = panel(" 系统服务 ");
    let inner = block.inner(cols[0]);
    frame.render_widget(block, cols[0]);

    let mut lines = Vec::new();
    for (index, info) in app.services.iter().enumerate() {
        record_mouse(
            app,
            Rect::new(inner.x, inner.y + index as u16, inner.width, 1),
            MouseAction::Service(index),
        );
        let selected = index == app.service_sel;
        let hovered = app.hover == Some(MouseAction::Service(index));
        let marker = if selected { "▶" } else { " " };
        let active_color = match info.active.as_str() {
            "active" => GOOD,
            "failed" => BAD,
            "activating" | "reloading" => WARN,
            _ => DIM,
        };
        let enabled_color = match info.enabled.as_str() {
            "enabled" => GOOD,
            "static" => WARN,
            "masked" => BAD,
            _ => DIM,
        };
        lines.push(Line::from(vec![
            Span::styled(
                format!(" {marker} {:<24}", util::truncate(info.display_name(), 24)),
                if selected {
                    Style::new().bg(SURFACE).fg(ACCENT).bold()
                } else if hovered {
                    Style::new().bg(SURFACE_HOVER).fg(Color::Gray)
                } else {
                    Style::new().fg(Color::Gray)
                },
            ),
            Span::styled(
                format!("{:<10}", active_zh(&info.active)),
                Style::new().fg(active_color),
            ),
            Span::styled(
                format!("{:<12}", enabled_zh(&info.enabled)),
                Style::new().fg(enabled_color),
            ),
        ]));
    }

    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        " s 启动   t 停止   R 重启",
        Style::new().fg(DIM),
    )));
    lines.push(Line::from(Span::styled(
        " e 启用自启   d 禁用自启   E 启用并启动   D 禁用并停止",
        Style::new().fg(DIM),
    )));
    let y = inner.y + lines.len() as u16;
    if let Some(line) = action_bar(
        app,
        inner,
        y,
        &[
            (
                "启动",
                MouseAction::Key(KeyCode::Char('s'), KeyModifiers::NONE),
            ),
            (
                "停止",
                MouseAction::Key(KeyCode::Char('t'), KeyModifiers::NONE),
            ),
            (
                "重启",
                MouseAction::Key(KeyCode::Char('R'), KeyModifiers::NONE),
            ),
            (
                "启用",
                MouseAction::Key(KeyCode::Char('e'), KeyModifiers::NONE),
            ),
            (
                "禁用",
                MouseAction::Key(KeyCode::Char('d'), KeyModifiers::NONE),
            ),
        ],
    ) {
        lines.push(line);
    }

    frame.render_widget(
        Paragraph::new(Text::from(lines)).wrap(Wrap { trim: false }),
        inner,
    );

    let block = panel(" 服务详情 ");
    let inner = block.inner(cols[1]);
    frame.render_widget(block, cols[1]);

    let mut details = Vec::new();
    if let Some(info) = app.services.get(app.service_sel) {
        details.push(detail("服务", info.unit.clone()));
        details.push(detail("加载", info.load.clone()));
        details.push(detail("状态", active_zh(&info.active).into()));
        details.push(detail("自启", enabled_zh(&info.enabled).into()));
        details.push(Line::raw(""));
        details.push(Line::from(Span::styled(
            " s 启动   t 停止   R 重启",
            Style::new().fg(DIM),
        )));
        details.push(Line::from(Span::styled(
            " e 启用自启   d 禁用自启",
            Style::new().fg(DIM),
        )));
        details.push(Line::from(Span::styled(
            " E 启用并立即启动   D 禁用并停止",
            Style::new().fg(DIM),
        )));
        details.push(Line::raw(""));
        details.push(Line::from(Span::styled(
            " 停止或重启 NetworkManager 会断网；",
            Style::new().fg(WARN),
        )));
        details.push(Line::from(Span::styled(
            " 停止 asusd 会失去 ASUS 硬件控制。",
            Style::new().fg(WARN),
        )));
        if info.load == "not-found" {
            details.push(Line::raw(""));
            details.push(Line::from(Span::styled(
                " 该服务不存在",
                Style::new().fg(BAD),
            )));
        }
    } else {
        details.push(Line::from(Span::styled(" 没有服务", Style::new().fg(DIM))));
    }

    frame.render_widget(
        Paragraph::new(Text::from(details)).wrap(Wrap { trim: false }),
        inner,
    );
}

fn active_zh(state: &str) -> &'static str {
    match state {
        "active" => "运行中",
        "inactive" => "已停止",
        "failed" => "失败",
        "activating" => "启动中",
        "deactivating" => "停止中",
        "reloading" => "重载中",
        _ => "未知",
    }
}

fn enabled_zh(state: &str) -> &'static str {
    match state {
        "enabled" => "已启用",
        "disabled" => "已禁用",
        "static" => "static",
        "masked" => "已屏蔽",
        _ => "未知",
    }
}

fn draw_cache_modal(frame: &mut Frame, area: Rect, app: &App) {
    let vertical = Layout::vertical([
        Constraint::Percentage(18),
        Constraint::Percentage(64),
        Constraint::Percentage(18),
    ])
    .split(area);
    let horizontal = Layout::horizontal([
        Constraint::Percentage(20),
        Constraint::Percentage(60),
        Constraint::Percentage(20),
    ])
    .split(vertical[1]);
    let popup = horizontal[1];

    frame.render_widget(Clear, popup);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .title(Span::styled(" 网络缓存 ", Style::new().fg(ACCENT).bold()))
        .border_style(Style::new().fg(ACCENT));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let mut lines = Vec::new();
    if let Some(stats) = &app.cache_stats {
        lines.push(Line::from(Span::styled(
            " DNS 缓存（systemd-resolved）",
            Style::new().fg(ACCENT).bold(),
        )));
        lines.push(detail("当前条目", stats.current_size.to_string()));
        lines.push(detail("命中", stats.hits.to_string()));
        lines.push(detail("未命中", stats.misses.to_string()));
        lines.push(detail("总事务", stats.transactions.to_string()));
        lines.push(Line::raw(""));
        lines.push(detail(
            "DNS",
            if stats.dns_servers.is_empty() {
                "-".into()
            } else {
                stats.dns_servers.join(", ")
            },
        ));
        lines.push(detail("邻居条目", stats.neighbors.to_string()));
    } else {
        lines.push(Line::from(Span::styled(
            " 无法读取网络缓存统计",
            Style::new().fg(WARN),
        )));
    }

    lines.push(Line::raw(""));

    let y = inner.y + lines.len() as u16;
    record_mouse(
        app,
        Rect::new(inner.x, y, inner.width, 1),
        MouseAction::CacheFlushDns,
    );
    lines.push(Line::from(Span::styled(
        " [f] 清空 DNS 缓存",
        Style::new().fg(DIM),
    )));
    record_mouse(
        app,
        Rect::new(inner.x, y + 1, inner.width, 1),
        MouseAction::CacheFlushNeighbors,
    );
    lines.push(Line::from(Span::styled(
        " [F] 清空邻居缓存",
        Style::new().fg(DIM),
    )));
    record_mouse(
        app,
        Rect::new(inner.x, y + 2, inner.width, 1),
        MouseAction::CacheEditSize,
    );
    lines.push(Line::from(Span::styled(
        " [e] 修改 DNS 缓存上限（重启 resolved）",
        Style::new().fg(DIM),
    )));
    record_mouse(
        app,
        Rect::new(inner.x, y + 3, 12, 1),
        MouseAction::CacheRefresh,
    );
    record_mouse(
        app,
        Rect::new(inner.x + 12, y + 3, 12, 1),
        MouseAction::CacheClose,
    );
    lines.push(Line::from(vec![
        Span::styled(" [r] 刷新统计", Style::new().fg(DIM)),
        Span::styled("   [Esc] 关闭", Style::new().fg(DIM)),
    ]));
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        " 修改缓存上限会写入 /etc/systemd/resolved.conf.d/rsztui-cache.conf",
        Style::new().fg(DIM),
    )));

    frame.render_widget(
        Paragraph::new(Text::from(lines)).wrap(Wrap { trim: false }),
        inner,
    );
}

fn draw_settings(frame: &mut Frame, area: Rect, app: &App) {
    let block = panel(" 设置与关于 ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines = vec![
        Line::from(vec![
            Span::styled("  配置文件  ", Style::new().fg(DIM)),
            Span::styled(
                crate::config::Config::path().display().to_string(),
                Style::new().fg(Color::Gray),
            ),
        ]),
        Line::from(vec![
            Span::styled("  asusd 配置 ", Style::new().fg(DIM)),
            Span::styled("/etc/asusd/asusd.ron", Style::new().fg(Color::Gray)),
        ]),
        Line::from(vec![
            Span::styled("  版本      ", Style::new().fg(DIM)),
            Span::styled(
                format!("RszTui {}", env!("CARGO_PKG_VERSION")),
                Style::new().fg(Color::Gray),
            ),
        ]),
        Line::from(vec![
            Span::styled("  持久化    ", Style::new().fg(DIM)),
            Span::styled(
                if app.tuning {
                    "asusd 接管，开启"
                } else {
                    "关闭"
                },
                Style::new().fg(if app.tuning { GOOD } else { WARN }),
            ),
        ]),
        Line::raw(""),
        Line::from(Span::styled(" 硬件与系统", Style::new().fg(ACCENT).bold())),
        Line::from(vec![
            Span::styled("  CPU       ", Style::new().fg(DIM)),
            Span::styled(
                util::truncate(&app.hw.cpu_model, 58),
                Style::new().fg(Color::Gray),
            ),
        ]),
        Line::from(vec![
            Span::styled("  核心      ", Style::new().fg(DIM)),
            Span::styled(
                format!("{} 核 / {} 线程", app.hw.cores, app.hw.threads),
                Style::new().fg(Color::Gray),
            ),
        ]),
        Line::from(vec![
            Span::styled("  显卡      ", Style::new().fg(DIM)),
            Span::styled(
                util::truncate(&app.hw.gpus.join(" / "), 58),
                Style::new().fg(Color::Gray),
            ),
        ]),
        Line::from(vec![
            Span::styled("  内存      ", Style::new().fg(DIM)),
            Span::styled(
                util::human_bytes(app.hw.memory_total as f64),
                Style::new().fg(Color::Gray),
            ),
        ]),
        Line::from(vec![
            Span::styled("  内核      ", Style::new().fg(DIM)),
            Span::styled(app.hw.kernel.clone(), Style::new().fg(Color::Gray)),
        ]),
        Line::from(vec![
            Span::styled("  BIOS      ", Style::new().fg(DIM)),
            Span::styled(app.hw.bios.clone(), Style::new().fg(Color::Gray)),
        ]),
        Line::raw(""),
        Line::from(Span::styled(" 依赖检查", Style::new().fg(ACCENT).bold())),
    ];

    for check in &app.deps.checks {
        lines.push(Line::from(vec![
            Span::styled(
                format!(
                    "  {} {:<14}",
                    if check.ok { "✅" } else { "❌" },
                    check.name
                ),
                Style::new().fg(if check.ok { GOOD } else { BAD }),
            ),
            Span::styled(
                util::truncate(&check.detail, 52),
                Style::new().fg(Color::Gray),
            ),
        ]));
    }
    if app.deps.has_warnings() {
        lines.push(Line::from(Span::styled(
            "  运行 rsztui doctor 查看安装/启动命令",
            Style::new().fg(WARN),
        )));
    }

    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        "  按 ? 查看完整快捷键，持久化说明见 docs/CONFIG.md",
        Style::new().fg(DIM),
    )));

    frame.render_widget(Paragraph::new(Text::from(lines)), inner);
}

fn draw_footer(frame: &mut Frame, area: Rect, app: &App) {
    let rows = Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).split(area);

    let status_style = if app.status_err {
        Style::new().fg(BAD)
    } else {
        Style::new().fg(GOOD)
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" ▌ ", Style::new().fg(ACCENT)),
            Span::styled(
                util::truncate(&app.status, (area.width as usize).saturating_sub(3)),
                status_style,
            ),
        ]))
        .style(Style::new().bg(SURFACE)),
        rows[0],
    );

    let hint = match app.tab {
        Tab::Dashboard => " Tab 切换  r 刷新  ? 帮助  q 退出",
        Tab::Performance => " f 焦点  j/k 选择  Enter 应用  a/b 设默认  g/G GPU  ? 帮助",
        Tab::Battery => " h/l 上限  Enter 应用  o 单次充满  ? 帮助",
        Tab::Fan => " i/I 编辑曲线  j/k 预设  p 应用  s 保存  e 启用  ? 帮助",
        Tab::Lighting => " b 亮度  e/E 灯效  c 颜色  S 静态色  ? 帮助",
        Tab::Network => " v 视图  j/k 选择  Enter 编辑  u/d 启停  p Ping  n DNS  c 缓存",
        Tab::Services => " j/k 选择  s 启动  t 停止  R 重启  e/d 自启  E/D 立即  ? 帮助",
        _ => " Tab 切换  1-8 跳转  r 刷新  ? 帮助  q 退出",
    };
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            hint,
            Style::new().fg(DIM).bg(Color::Rgb(28, 32, 38)),
        ))),
        rows[1],
    );
}

fn draw_modal(frame: &mut Frame, area: Rect, app: &App, modal: &Modal) {
    let (v, h) = match modal {
        Modal::Message { .. } => ([8, 84, 8], [8, 84, 8]),
        _ => ([30, 40, 30], [18, 64, 18]),
    };
    let vertical = Layout::vertical([
        Constraint::Percentage(v[0]),
        Constraint::Percentage(v[1]),
        Constraint::Percentage(v[2]),
    ])
    .split(area);
    let horizontal = Layout::horizontal([
        Constraint::Percentage(h[0]),
        Constraint::Percentage(h[1]),
        Constraint::Percentage(h[2]),
    ])
    .split(vertical[1]);
    let popup = horizontal[1];

    frame.render_widget(Clear, popup);

    let (title, body, confirm): (String, String, bool) = match modal {
        Modal::Confirm { title, body, .. } => (title.clone(), body.clone(), true),
        Modal::Message { title, body } => (title.clone(), body.clone(), false),
        Modal::Input {
            title,
            prompt,
            value,
            masked,
            ..
        } => {
            let shown = if *masked {
                "●".repeat(value.chars().count())
            } else {
                value.clone()
            };
            (title.clone(), format!("{prompt}\n\n  {shown}▏"), true)
        }
    };

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .title(Span::styled(
            format!(" {title} "),
            Style::new().fg(ACCENT).bold(),
        ))
        .border_style(Style::new().fg(ACCENT));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let parts = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).split(inner);
    frame.render_widget(Paragraph::new(body).wrap(Wrap { trim: false }), parts[0]);

    let buttons: Vec<(&str, MouseAction)> = match modal {
        Modal::Message { .. } => vec![("关闭", MouseAction::ModalConfirm)],
        Modal::Input { .. } => vec![
            ("确定", MouseAction::ModalConfirm),
            ("取消", MouseAction::ModalCancel),
        ],
        Modal::Confirm { .. } => vec![
            ("确认", MouseAction::ModalConfirm),
            ("取消", MouseAction::ModalCancel),
        ],
    };

    let mut spans = Vec::new();
    let mut x = parts[1].x;
    for (label, action) in &buttons {
        let width = UnicodeWidthStr::width(*label) as u16;
        record_mouse(app, Rect::new(x, parts[1].y, width + 2, 1), *action);
        spans.push(Span::styled(
            format!(" {label} "),
            Style::new().fg(ACCENT).bold(),
        ));
        x += width + 2;
    }
    spans.push(Span::styled(
        if confirm {
            "   Enter 确认 / Esc 取消"
        } else {
            "   Enter 关闭"
        },
        Style::new().fg(DIM),
    ));
    frame.render_widget(Paragraph::new(Line::from(spans)), parts[1]);
}

fn panel(title: impl Into<Line<'static>>) -> Block<'static> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .title(title.into())
        .title_style(Style::new().fg(ACCENT).bold())
        .border_style(Style::new().fg(PANEL_BORDER))
}

fn panel_focused(title: impl Into<Line<'static>>, focused: bool) -> Block<'static> {
    let border = if focused { ACCENT } else { PANEL_BORDER };
    Block::bordered()
        .border_type(BorderType::Rounded)
        .title(title.into())
        .title_style(Style::new().fg(ACCENT).bold())
        .border_style(Style::new().fg(border))
}

fn profile_color(profile: Profile) -> Color {
    match profile {
        Profile::Quiet => Color::Rgb(110, 170, 255),
        Profile::Balanced => GOOD,
        Profile::Performance => BAD,
    }
}

fn battery_color(app: &App) -> Color {
    match app.battery.capacity {
        Some(v) if v <= 15 => BAD,
        Some(v) if v <= 35 => WARN,
        _ => GOOD,
    }
}

fn temp_color(value: f32) -> Color {
    if value >= 90.0 {
        BAD
    } else if value >= 75.0 {
        WARN
    } else {
        GOOD
    }
}
