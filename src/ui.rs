use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Clear, Gauge, Paragraph, Sparkline, Wrap};

use crate::app::{App, Modal, NetSummary, Tab};
use crate::asus::{GpuMode, Profile};
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
    draw_footer(frame, chunks[2], app);

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
            app.net
                .ssid
                .clone()
                .or_else(|| {
                    app.net
                        .devices
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
    let block = panel(" 风扇曲线（只读） ");
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

    if app.fan_enabled.is_empty() {
        lines.push(Line::from(Span::styled(
            "  asusctl 没有返回风扇状态",
            Style::new().fg(DIM),
        )));
    } else {
        lines.push(Line::from(Span::styled(
            " 当前启用曲线",
            Style::new().fg(DIM).bold(),
        )));
        lines.push(Line::from(Span::styled(
            format!(
                " {}",
                util::truncate(&app.fan_enabled, (inner.width as usize).saturating_sub(2))
            ),
            Style::new().fg(DIM),
        )));
    }

    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        " 下一批支持：套用/保存预设、导入导出曲线",
        Style::new().fg(DIM),
    )));

    frame.render_widget(
        Paragraph::new(Text::from(lines)).wrap(Wrap { trim: false }),
        inner,
    );
}

fn draw_lighting(frame: &mut Frame, area: Rect, app: &App) {
    let block = panel(" 键盘灯光 ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let lines = vec![
        Line::from(vec![
            Span::styled("  当前亮度  ", Style::new().fg(DIM)),
            Span::styled(app.led.clone(), Style::new().fg(ACCENT).bold()),
        ]),
        Line::raw(""),
        Line::from(Span::styled(
            " b 切换亮度（off → low → med → high）",
            Style::new().fg(DIM),
        )),
        Line::raw(""),
        Line::from(Span::styled(
            " Aura 灯效：static / breathe / rainbow-cycle / rainbow-wave",
            Style::new().fg(DIM),
        )),
        Line::from(Span::styled(
            "          stars / rain / ripple / pulse / laser",
            Style::new().fg(DIM),
        )),
        Line::from(Span::styled(
            " 下一批支持：灯效切换与颜色/速度调整",
            Style::new().fg(WARN),
        )),
    ];

    frame.render_widget(Paragraph::new(Text::from(lines)), inner);
}

fn draw_network(frame: &mut Frame, area: Rect, app: &App) {
    let block = panel(" 网络（只读） ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines = Vec::new();
    lines.push(Line::from(vec![
        Span::styled("  Wi-Fi  ", Style::new().fg(DIM)),
        Span::styled(
            match app.net.wifi_enabled {
                Some(true) => "已启用",
                Some(false) => "已禁用",
                None => "未知",
            },
            Style::new().fg(if app.net.wifi_enabled == Some(true) {
                GOOD
            } else {
                WARN
            }),
        ),
        Span::styled("   当前 SSID  ", Style::new().fg(DIM)),
        Span::styled(
            app.net.ssid.clone().unwrap_or_else(|| "-".into()),
            Style::new().fg(Color::Gray),
        ),
    ]));
    lines.push(Line::raw(""));

    for device in &app.net.devices {
        if device.kind == "loopback" {
            continue;
        }
        let state_color = if device.state.contains("connected") && device.kind != "wifi-p2p" {
            GOOD
        } else if device.state.contains("unavailable") {
            BAD
        } else {
            DIM
        };
        lines.push(Line::from(vec![
            Span::styled(
                format!("  {}", util::truncate(&device.name, 12)),
                Style::new().fg(ACCENT),
            ),
            Span::styled(
                format!("  {:<10}", util::truncate(&device.kind, 10)),
                Style::new().fg(DIM),
            ),
            Span::styled(
                format!("  {:<24}", util::truncate(&device.state, 24)),
                Style::new().fg(state_color),
            ),
            Span::styled(
                util::truncate(&device.connection, 22),
                Style::new().fg(Color::Gray),
            ),
        ]));
    }

    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        " 下一批支持：Wi-Fi 扫描连接、手动 IP/网关/DNS、VPN、热点",
        Style::new().fg(WARN),
    )));

    frame.render_widget(Paragraph::new(Text::from(lines)), inner);
}

fn draw_settings(frame: &mut Frame, area: Rect, app: &App) {
    let block = panel(" 设置与关于 ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let backup_dir = "/var/backups/rogtui";
    let lines = vec![
        Line::from(vec![
            Span::styled("  配置文件  ", Style::new().fg(DIM)),
            Span::styled(
                crate::config::Config::path().display().to_string(),
                Style::new().fg(Color::Gray),
            ),
        ]),
        Line::from(vec![
            Span::styled("  备份目录  ", Style::new().fg(DIM)),
            Span::styled(backup_dir, Style::new().fg(Color::Gray)),
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
        Tab::Lighting => " b 切换亮度  ? 帮助",
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

    let (title, body, confirm) = match modal {
        Modal::Confirm { title, body, .. } => (title.as_str(), body.as_str(), true),
        Modal::Message { title, body } => (title.as_str(), body.as_str(), false),
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
    frame.render_widget(
        Paragraph::new(body.to_string()).wrap(Wrap { trim: false }),
        parts[0],
    );
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

#[allow(dead_code)]
fn net_state_color(summary: &NetSummary, device: &str) -> Color {
    if summary
        .devices
        .iter()
        .any(|d| d.name == device && d.state.contains("connected"))
    {
        GOOD
    } else {
        DIM
    }
}
