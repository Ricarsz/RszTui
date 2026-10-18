use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Clear, Gauge, Paragraph, Sparkline, Wrap};

use crate::app::{AURA_COLORS, App, IpForm, Modal, NetView, Tab};
use crate::asus::{GpuMode, Profile};
use crate::nm;
use crate::util;

const ACCENT: Color = Color::Rgb(90, 200, 220);
const DIM: Color = Color::Rgb(110, 118, 130);
const GOOD: Color = Color::Rgb(120, 210, 140);
const WARN: Color = Color::Rgb(240, 190, 90);
const BAD: Color = Color::Rgb(255, 110, 90);
const PANEL_BORDER: Color = Color::Rgb(70, 78, 90);

pub fn draw(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let chunks = Layout::vertical([
        Constraint::Length(3),
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

    if let Some(modal) = &app.modal {
        draw_modal(frame, area, modal);
    }
}

fn draw_header(frame: &mut Frame, area: Rect, app: &App) {
    let profile = app
        .profile
        .map(|p| p.active)
        .map(|p| p.zh())
        .unwrap_or("未知");
    let profile_color = app.profile.map(|p| profile_color(p.active)).unwrap_or(DIM);

    let battery = app
        .battery
        .capacity
        .map(|v| format!("{v}%"))
        .unwrap_or_else(|| "?%".into());
    let limit = app
        .battery
        .limit
        .map(|v| format!("上限 {v}%"))
        .unwrap_or_else(|| "上限未知".into());

    let line = Line::from(vec![
        Span::styled(" ROGTUI ", Style::new().fg(Color::Black).bg(ACCENT).bold()),
        Span::raw("  ASUS TUF 终端控制台"),
        Span::raw("    "),
        Span::styled(
            format!(" ● {profile} "),
            Style::new().fg(Color::Black).bg(profile_color).bold(),
        ),
        Span::raw(format!("  {battery}  {limit}")),
    ]);

    let block = Block::bordered()
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
    frame.render_widget(Paragraph::new(line).block(block), area);
}

fn draw_sidebar(frame: &mut Frame, area: Rect, app: &App) {
    let mut lines: Vec<Line> = Vec::new();
    for (index, tab) in Tab::ALL.iter().enumerate() {
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
    }

    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        " 快速状态",
        Style::new().fg(DIM).bold(),
    )));
    lines.push(Line::from(vec![
        Span::styled("  模式  ", Style::new().fg(DIM)),
        Span::styled(
            app.profile.map(|p| p.active.zh()).unwrap_or("未知"),
            Style::new()
                .fg(app.profile.map(|p| profile_color(p.active)).unwrap_or(DIM))
                .bold(),
        ),
    ]));
    lines.push(Line::from(vec![
        Span::styled("  电池  ", Style::new().fg(DIM)),
        Span::styled(
            app.battery
                .capacity
                .map(|v| format!("{v}%"))
                .unwrap_or_else(|| "?".into()),
            Style::new().fg(battery_color(app)),
        ),
    ]));
    lines.push(Line::from(vec![
        Span::styled("  GPU   ", Style::new().fg(DIM)),
        Span::styled(
            app.gpu_mode.map(|m| m.zh()).unwrap_or("未知"),
            Style::new().fg(Color::Gray),
        ),
    ]));
    lines.push(Line::from(vec![
        Span::styled("  网络  ", Style::new().fg(DIM)),
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
        .title(Span::styled(" 导航 ", Style::new().fg(ACCENT).bold()))
        .border_style(Style::new().fg(PANEL_BORDER));
    frame.render_widget(Paragraph::new(Text::from(lines)).block(block), area);
}

fn draw_body(frame: &mut Frame, area: Rect, app: &App) {
    match app.tab {
        Tab::Dashboard => draw_dashboard(frame, area, app),
        Tab::Performance => draw_performance(frame, area, app),
        Tab::Battery => draw_battery(frame, area, app),
        Tab::Fan => draw_fan(frame, area, app),
        Tab::Lighting => draw_lighting(frame, area, app),
        Tab::Network => draw_network(frame, area, app),
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
        Span::styled("  CPU  ", Style::new().fg(DIM)),
        Span::styled(power.join(" / "), Style::new().fg(Color::Gray)),
    ]));

    if let Some(mode) = app.gpu_mode {
        lines.push(Line::from(vec![
            Span::styled("  GPU  ", Style::new().fg(DIM)),
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
    let cols =
        Layout::horizontal([Constraint::Percentage(38), Constraint::Percentage(62)]).split(area);

    let left = Layout::vertical([Constraint::Min(0), Constraint::Length(9)]).split(cols[0]);
    draw_profiles(frame, left[0], app);
    draw_gpu_picker(frame, left[1], app);

    let right = Layout::vertical([Constraint::Length(12), Constraint::Min(0)]).split(cols[1]);
    draw_presets(frame, right[0], app);
    draw_power_fields(frame, right[1], app);
}

fn draw_profiles(frame: &mut Frame, area: Rect, app: &App) {
    let focused = app.perf_focus == crate::app::PerfFocus::Profiles;
    let block = panel_focused(" 性能模式 ", focused);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let state = app.profile;
    let mut lines = Vec::new();
    for (index, profile) in Profile::ALL.iter().enumerate() {
        let selected = index == app.profile_sel;
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

        let style = if selected {
            Style::new().bg(Color::Rgb(45, 52, 62)).bold()
        } else {
            Style::new()
        };
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
        let selected = index == app.gpu_sel;
        let current = app.gpu_mode == Some(*mode);
        let marker = if selected { "▶" } else { " " };
        let badge = if current { "  [当前]" } else { "" };
        lines.push(Line::from(vec![
            Span::styled(
                format!(" {marker} {}", mode.zh()),
                if selected {
                    Style::new().fg(ACCENT).bold()
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
        let selected = index == app.preset_sel;
        let marker = if selected { "▶" } else { " " };
        let style = if selected {
            Style::new().fg(ACCENT).bold()
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

    frame.render_widget(
        Paragraph::new(Text::from(vec![
            Line::raw(""),
            Line::from(Span::styled(
                " o 单次充满（忽略上限）",
                Style::new().fg(DIM),
            )),
            Line::from(Span::styled(
                " 上限越低越省电池，长期插电建议 60-80%",
                Style::new().fg(DIM),
            )),
        ])),
        rows[7],
    );
}

fn draw_fan(frame: &mut Frame, area: Rect, app: &App) {
    let cols =
        Layout::horizontal([Constraint::Percentage(62), Constraint::Percentage(38)]).split(area);
    draw_fan_curves(frame, cols[0], app);
    draw_fan_presets(frame, cols[1], app);
}

fn draw_fan_curves(frame: &mut Frame, area: Rect, app: &App) {
    let block = panel(" 当前风扇曲线 ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines = Vec::new();
    for curve in &app.fan_curves {
        let state = if curve.enabled {
            Span::styled("启用", Style::new().fg(GOOD))
        } else {
            Span::styled("停用", Style::new().fg(BAD))
        };
        lines.push(Line::from(vec![
            Span::styled(
                format!("  {:<4} ", curve.fan),
                Style::new().fg(ACCENT).bold(),
            ),
            state,
        ]));

        let points: Vec<String> = curve
            .temp
            .iter()
            .zip(curve.pwm.iter())
            .map(|(temp, pwm)| format!("{temp}°C:{}%", (*pwm as u32 * 100 / 255)))
            .collect();
        lines.push(Line::from(Span::styled(
            format!("    {}", points.join("  ")),
            Style::new().fg(Color::Gray),
        )));
        lines.push(Line::raw(""));
    }

    lines.push(Line::from(Span::styled(
        " e 启用/停用   d 恢复固件默认   s 保存当前为预设",
        Style::new().fg(DIM),
    )));
    lines.push(Line::from(Span::styled(
        " 曲线按性能模式保存，切换模式会套用对应曲线",
        Style::new().fg(DIM),
    )));

    frame.render_widget(
        Paragraph::new(Text::from(lines)).wrap(Wrap { trim: false }),
        inner,
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
            "  按 s 把当前曲线保存为预设",
            Style::new().fg(DIM),
        )));
    } else {
        for (index, preset) in app.cfg.fan_presets.iter().enumerate() {
            let selected = index == app.fan_sel;
            let marker = if selected { "▶" } else { " " };
            lines.push(Line::from(vec![
                Span::styled(
                    format!(" {marker} {}", preset.name),
                    if selected {
                        Style::new().fg(ACCENT).bold()
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
    let block = panel(" 键盘灯光 ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines = vec![
        Line::from(vec![
            Span::styled("  当前亮度  ", Style::new().fg(DIM)),
            Span::styled(app.led.clone(), Style::new().fg(ACCENT).bold()),
        ]),
        Line::raw(""),
        Line::from(Span::styled(
            " b 切换亮度（off → low → med → high）",
            Style::new().fg(DIM),
        )),
        Line::from(Span::styled(
            " e / E 下一个 / 上一个灯效",
            Style::new().fg(DIM),
        )),
        Line::raw(""),
        Line::from(Span::styled("  静态颜色  ", Style::new().fg(DIM))),
    ];

    let mut swatches = Vec::new();
    for (index, (name, hex)) in AURA_COLORS.iter().enumerate() {
        let selected = index == app.aura_color;
        swatches.push(Span::styled(
            format!(" {} {} ", if selected { "▶" } else { " " }, name),
            Style::new().fg(hex_color(hex)),
        ));
        swatches.push(Span::raw(" "));
        if index % 3 == 2 {
            lines.push(Line::from(std::mem::take(&mut swatches)));
        }
    }
    if !swatches.is_empty() {
        lines.push(Line::from(swatches));
    }

    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        " c 切换颜色   S 应用静态色",
        Style::new().fg(DIM),
    )));
    lines.push(Line::from(Span::styled(
        " 灯效列表：static / breathe / rainbow-cycle / rainbow-wave / stars / rain / ripple / pulse",
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
                let selected = index == app.net_sel;
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
                            Style::new().fg(ACCENT).bold()
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
                let selected = index == app.net_sel;
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
                            Style::new().fg(ACCENT).bold()
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
                let selected = index == app.net_sel;
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
                            Style::new().fg(ACCENT).bold()
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
                lines.push(Line::raw(""));
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
                lines.push(Line::from(Span::styled(
                    " Enter 编辑 IPv4 / DNS",
                    Style::new().fg(DIM),
                )));
                lines.push(Line::from(Span::styled(
                    " u 激活   d 断开   x 删除",
                    Style::new().fg(DIM),
                )));
            }
        }
        NetView::Wifi => {
            if let Some(ap) = app.wifi.get(app.net_sel) {
                lines.push(detail("SSID", ap.ssid.clone()));
                lines.push(detail("信号", format!("{}%", ap.signal)));
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
                lines.push(Line::from(Span::styled(
                    " Enter 连接（密码写入 NM keyfile）",
                    Style::new().fg(DIM),
                )));
                lines.push(Line::from(Span::styled(
                    " s 重新扫描",
                    Style::new().fg(DIM),
                )));
            }
        }
        NetView::Vpn => {
            if let Some(vpn) = app.vpns.get(app.net_sel) {
                lines.push(detail("名称", vpn.name.clone()));
                lines.push(detail(
                    "状态",
                    if vpn.active { "已连接" } else { "未连接" }.into(),
                ));
                lines.push(Line::raw(""));
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
        " v 切换视图   o 无线开关   t 热点",
        Style::new().fg(DIM),
    )));

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

fn draw_settings(frame: &mut Frame, area: Rect, app: &App) {
    let block = panel(" 设置与关于 ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let lines = vec![
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
                format!("rogtui {}", env!("CARGO_PKG_VERSION")),
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
        Line::from(Span::styled(" 说明", Style::new().fg(ACCENT).bold())),
        Line::from(Span::styled(
            "  性能模式、功耗、电池上限写入 /etc/asusd/asusd.ron，",
            Style::new().fg(DIM),
        )),
        Line::from(Span::styled(
            "  由 asusd 在开机和 AC/电池切换时自动恢复。",
            Style::new().fg(DIM),
        )),
        Line::raw(""),
        Line::from(Span::styled("  按 ? 查看完整快捷键", Style::new().fg(DIM))),
    ];

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
            Span::styled(" ", Style::new()),
            Span::styled(
                util::truncate(&app.status, (area.width as usize).saturating_sub(2)),
                status_style,
            ),
        ])),
        rows[0],
    );

    let hint = match app.tab {
        Tab::Dashboard => " Tab 切换  r 刷新  ? 帮助  q 退出",
        Tab::Performance => " f 焦点  j/k 选择  Enter 应用  a/b 设默认  g/G GPU  ? 帮助",
        Tab::Battery => " h/l 上限  Enter 应用  o 单次充满  ? 帮助",
        Tab::Fan => " j/k 预设  p 应用  s 保存  d 默认  e 启用/停用  ? 帮助",
        Tab::Lighting => " b 亮度  e/E 灯效  c 颜色  S 静态色  ? 帮助",
        Tab::Network => " v 视图  j/k 选择  Enter 连接/编辑  u/d 启停  x 删除  o 无线  t 热点",
        _ => " Tab 切换  1-7 跳转  r 刷新  ? 帮助  q 退出",
    };
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            hint,
            Style::new().fg(DIM).bg(Color::Rgb(28, 32, 38)),
        ))),
        rows[1],
    );
}

fn draw_modal(frame: &mut Frame, area: Rect, modal: &Modal) {
    let vertical = Layout::vertical([
        Constraint::Percentage(30),
        Constraint::Percentage(40),
        Constraint::Percentage(30),
    ])
    .split(area);
    let horizontal = Layout::horizontal([
        Constraint::Percentage(18),
        Constraint::Percentage(64),
        Constraint::Percentage(18),
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
        .title(Span::styled(
            format!(" {title} "),
            Style::new().fg(ACCENT).bold(),
        ))
        .border_style(Style::new().fg(ACCENT));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let parts = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).split(inner);
    frame.render_widget(Paragraph::new(body).wrap(Wrap { trim: false }), parts[0]);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            if confirm {
                " Enter 确认    Esc 取消"
            } else {
                " Enter 关闭"
            },
            Style::new().fg(DIM),
        )))
        .alignment(Alignment::Right),
        parts[1],
    );
}

fn panel(title: impl Into<Line<'static>>) -> Block<'static> {
    Block::bordered()
        .title(title.into())
        .title_style(Style::new().fg(ACCENT).bold())
        .border_style(Style::new().fg(PANEL_BORDER))
}

fn panel_focused(title: impl Into<Line<'static>>, focused: bool) -> Block<'static> {
    let border = if focused { ACCENT } else { PANEL_BORDER };
    Block::bordered()
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
