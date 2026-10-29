use std::cell::Cell;

use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph, Wrap};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::app::{AURA_COLORS, App, IpForm, Modal, MouseAction, MouseTarget, NetView, Tab};
use crate::asus::{self, GpuMode, Profile};
use crate::gpu;
use crate::nm;
use crate::util;

#[derive(Clone, Copy)]
struct Palette {
    accent: Color,
    chart: Color,
    text: Color,
    dim: Color,
    faint: Color,
    good: Color,
    warn: Color,
    bad: Color,
    border: Color,
    surface: Color,
    surface_hover: Color,
    selected: Color,
}

const OCEAN: Palette = Palette {
    accent: Color::Rgb(90, 200, 220),
    chart: Color::Rgb(130, 150, 240),
    text: Color::Rgb(214, 221, 230),
    dim: Color::Rgb(108, 117, 130),
    faint: Color::Rgb(78, 88, 102),
    good: Color::Rgb(120, 210, 140),
    warn: Color::Rgb(240, 190, 90),
    bad: Color::Rgb(255, 110, 90),
    border: Color::Rgb(72, 82, 96),
    surface: Color::Rgb(40, 47, 58),
    surface_hover: Color::Rgb(33, 40, 50),
    selected: Color::Rgb(29, 46, 56),
};

const FOREST: Palette = Palette {
    accent: Color::Rgb(122, 210, 168),
    chart: Color::Rgb(135, 185, 225),
    text: Color::Rgb(218, 229, 222),
    dim: Color::Rgb(108, 126, 116),
    faint: Color::Rgb(78, 96, 86),
    good: Color::Rgb(130, 215, 150),
    warn: Color::Rgb(235, 190, 100),
    bad: Color::Rgb(250, 110, 90),
    border: Color::Rgb(70, 90, 80),
    surface: Color::Rgb(38, 50, 44),
    surface_hover: Color::Rgb(31, 43, 37),
    selected: Color::Rgb(27, 48, 39),
};

const SUNSET: Palette = Palette {
    accent: Color::Rgb(250, 166, 102),
    chart: Color::Rgb(235, 145, 120),
    text: Color::Rgb(236, 226, 214),
    dim: Color::Rgb(130, 116, 104),
    faint: Color::Rgb(96, 82, 70),
    good: Color::Rgb(150, 205, 125),
    warn: Color::Rgb(245, 185, 85),
    bad: Color::Rgb(255, 100, 85),
    border: Color::Rgb(94, 76, 66),
    surface: Color::Rgb(52, 42, 36),
    surface_hover: Color::Rgb(43, 34, 29),
    selected: Color::Rgb(59, 42, 31),
};

const VIOLET: Palette = Palette {
    accent: Color::Rgb(190, 158, 255),
    chart: Color::Rgb(150, 175, 255),
    text: Color::Rgb(226, 220, 236),
    dim: Color::Rgb(118, 111, 136),
    faint: Color::Rgb(86, 80, 104),
    good: Color::Rgb(130, 210, 170),
    warn: Color::Rgb(240, 190, 110),
    bad: Color::Rgb(255, 115, 130),
    border: Color::Rgb(82, 74, 100),
    surface: Color::Rgb(44, 40, 56),
    surface_hover: Color::Rgb(35, 32, 46),
    selected: Color::Rgb(43, 34, 61),
};

const MONO: Palette = Palette {
    accent: Color::Rgb(198, 204, 212),
    chart: Color::Rgb(170, 176, 186),
    text: Color::Rgb(232, 234, 238),
    dim: Color::Rgb(126, 130, 136),
    faint: Color::Rgb(90, 94, 100),
    good: Color::Rgb(170, 205, 175),
    warn: Color::Rgb(225, 195, 120),
    bad: Color::Rgb(235, 125, 115),
    border: Color::Rgb(86, 90, 96),
    surface: Color::Rgb(44, 46, 50),
    surface_hover: Color::Rgb(36, 38, 42),
    selected: Color::Rgb(50, 52, 58),
};

thread_local! {
    static PALETTE: Cell<Palette> = const { Cell::new(OCEAN) };
}

fn set_palette(palette: Palette) {
    PALETTE.with(|cell| cell.set(palette));
}

fn pal() -> Palette {
    PALETTE.with(|cell| cell.get())
}

fn palette_for(name: &str) -> Palette {
    match name {
        "forest" => FOREST,
        "sunset" => SUNSET,
        "violet" => VIOLET,
        "mono" => MONO,
        _ => OCEAN,
    }
}

fn rate_style(app: &App) -> util::RateStyle {
    util::RateStyle::from_name(&app.cfg.ui.rate_style)
}

pub fn draw(frame: &mut Frame, app: &App) {
    set_palette(palette_for(&app.cfg.ui.theme));
    app.mouse_targets.borrow_mut().clear();
    let area = frame.area();
    let chunks = Layout::vertical([
        Constraint::Length(4),
        Constraint::Min(0),
        Constraint::Length(2),
    ])
    .split(area);

    draw_header(frame, chunks[0], app);

    let sidebar_width = if area.width < 110 { 19 } else { 23 };
    let body = Layout::horizontal([Constraint::Length(sidebar_width), Constraint::Min(0)])
        .split(chunks[1]);
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
    let profile_color = app
        .profile
        .map(|p| profile_color(p.active))
        .unwrap_or(pal().dim);
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
    let cpu_temp = app
        .snap
        .temps
        .iter()
        .find(|(label, _)| label.to_ascii_lowercase().contains("tctl"))
        .map(|(_, value)| *value)
        .or(app.snap.max_temp)
        .map(|v| format!("{v:.1}°C"))
        .unwrap_or_else(|| "-".into());
    let (power, power_color) = match app.battery.ac_online {
        Some(true) => ("AC 供电", pal().good),
        Some(false) => ("电池供电", pal().warn),
        None => ("供电未知", pal().dim),
    };

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(pal().border))
        .title(Line::from(Span::styled(
            " 状态 ",
            Style::new().fg(pal().accent).bold(),
        )));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let left = Line::from(vec![
        Span::styled(
            " RszTui ",
            Style::new().fg(Color::Black).bg(pal().accent).bold(),
        ),
        Span::raw("  "),
        Span::styled(model, Style::new().fg(pal().text).bold()),
    ]);
    let right = Line::from(vec![
        Span::raw(" "),
        Span::styled(power, Style::new().fg(power_color)),
        Span::styled("  ·  ", Style::new().fg(pal().border)),
        Span::styled(
            format!("up {}", format_uptime(app.snap.uptime)),
            Style::new().fg(pal().dim),
        ),
    ]);
    let right_width = right.width().min(inner.width as usize) as u16;
    let rows = Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).split(inner);
    let top =
        Layout::horizontal([Constraint::Min(0), Constraint::Length(right_width)]).split(rows[0]);
    frame.render_widget(Paragraph::new(left), top[0]);
    frame.render_widget(Paragraph::new(right).alignment(Alignment::Right), top[1]);

    let separator = || Span::styled(" │ ", Style::new().fg(pal().border));
    let mut status = vec![
        Span::styled(" ●", Style::new().fg(profile_color)),
        Span::styled(
            format!(" {profile}"),
            Style::new().fg(Color::Black).bg(profile_color).bold(),
        ),
        separator(),
        Span::styled("GPU ", Style::new().fg(pal().dim)),
        Span::styled(gpu, Style::new().fg(pal().text)),
        separator(),
        Span::styled("电池 ", Style::new().fg(pal().dim)),
        Span::styled(battery, Style::new().fg(battery_color(app))),
    ];
    if let Some(temp) = app
        .snap
        .gpus
        .iter()
        .find(|status| status.gpu.vendor == gpu::Vendor::Nvidia)
        .and_then(|status| status.stats.temp)
    {
        status.insert(
            5,
            Span::styled(format!(" {temp:.0}°C"), Style::new().fg(temp_color(temp))),
        );
    }
    if inner.width >= 84 {
        status.push(Span::styled(
            format!("  {limit}"),
            Style::new().fg(pal().dim),
        ));
    }
    status.push(separator());
    status.push(Span::styled("CPU ", Style::new().fg(pal().dim)));
    status.push(Span::styled(
        cpu_temp,
        Style::new().fg(temp_color(
            app.snap
                .temps
                .iter()
                .find(|(label, _)| label.to_ascii_lowercase().contains("tctl"))
                .map(|(_, value)| *value)
                .or(app.snap.max_temp)
                .unwrap_or(0.0),
        )),
    ));
    if inner.width >= 76 {
        status.push(separator());
        status.push(Span::styled("网络 ", Style::new().fg(pal().dim)));
        status.push(Span::styled(net, Style::new().fg(pal().text)));
    }
    frame.render_widget(Paragraph::new(Line::from(status)), rows[1]);
}

fn draw_sidebar(frame: &mut Frame, area: Rect, app: &App) {
    let inner_width = area.width.saturating_sub(2) as usize;
    let mut lines: Vec<Line> = Vec::new();
    let mut line_idx: u16 = 0;

    for (index, tab) in Tab::ALL.iter().enumerate() {
        if index == 0 {
            lines.push(sidebar_heading("控制", inner_width));
            line_idx += 1;
        } else if index == 5 {
            lines.push(Line::raw(""));
            lines.push(sidebar_heading("系统", inner_width));
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
        let marker = if selected { "▌" } else { " " };
        let key = index + 1;
        let label = tab.zh();
        let used = UnicodeWidthStr::width(format!(" {marker} {key} {label}").as_str());
        let pad = " ".repeat(inner_width.saturating_sub(used));

        let hovered = app.hover == Some(MouseAction::Tab(*tab));
        let base = if selected {
            Style::new().bg(pal().selected)
        } else if hovered {
            Style::new().bg(pal().surface_hover)
        } else {
            Style::new()
        };
        if selected {
            lines.push(Line::from(vec![
                Span::styled(format!(" {marker} "), base.fg(pal().accent).bold()),
                Span::styled(format!("{key} "), base.fg(pal().accent)),
                Span::styled(label, base.fg(pal().text).bold()),
                Span::styled(pad, base),
            ]));
        } else {
            lines.push(Line::from(vec![
                Span::styled(format!(" {marker} "), base.fg(pal().faint)),
                Span::styled(format!("{key} "), base.fg(pal().dim)),
                Span::styled(label, base.fg(pal().text)),
                Span::styled(pad, base),
            ]));
        }
        line_idx += 1;
    }

    lines.push(Line::raw(""));
    lines.push(sidebar_heading("状态", inner_width));
    lines.push(Line::from(vec![
        Span::styled(
            "  ● ",
            Style::new()
                .fg(app
                    .profile
                    .map(|p| profile_color(p.active))
                    .unwrap_or(pal().dim))
                .bold(),
        ),
        Span::styled(
            app.profile.map(|p| p.active.zh()).unwrap_or("未知"),
            Style::new().fg(pal().text).bold(),
        ),
    ]));
    lines.push(Line::from(vec![
        Span::styled("  电池 ", Style::new().fg(pal().dim)),
        Span::styled(
            app.battery
                .capacity
                .map(|v| format!("{v}%"))
                .unwrap_or_else(|| "?".into()),
            Style::new().fg(battery_color(app)).bold(),
        ),
        Span::styled(
            app.battery
                .limit
                .map(|v| format!(" 限 {v}%"))
                .unwrap_or_default(),
            Style::new().fg(pal().dim),
        ),
    ]));
    lines.push(Line::from(vec![
        Span::styled("  GPU  ", Style::new().fg(pal().dim)),
        Span::styled(gpu_short(app), Style::new().fg(pal().text)),
    ]));
    lines.push(Line::from(vec![
        Span::styled("  网络 ", Style::new().fg(pal().dim)),
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
            Style::new().fg(pal().text),
        ),
    ]));

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .title(Span::styled(" 导航 ", Style::new().fg(pal().accent).bold()))
        .border_style(Style::new().fg(pal().border));
    frame.render_widget(Paragraph::new(Text::from(lines)).block(block), area);
}

fn sidebar_heading(label: &str, width: usize) -> Line<'static> {
    let label = format!(" {label} ");
    let used = UnicodeWidthStr::width(label.as_str());
    Line::from(vec![
        Span::styled(label, Style::new().fg(pal().dim).bold()),
        Span::styled(
            "─".repeat(width.saturating_sub(used + 1)),
            Style::new().fg(pal().faint),
        ),
    ])
}

fn gpu_short(app: &App) -> &'static str {
    match app.gpu_mode {
        Some(GpuMode::Integrated) => "集显",
        Some(GpuMode::Hybrid) => "混合",
        Some(GpuMode::Ultimate) => "独显",
        None => "未知",
    }
}

fn fan_rpm(app: &App, fan: &str) -> Option<u32> {
    app.snap
        .fans
        .iter()
        .find(|(label, _)| label.eq_ignore_ascii_case(fan))
        .map(|(_, rpm)| *rpm)
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
        Tab::Gpu => draw_gpu(frame, area, &app.snap.gpus, app.snap_loading),
        Tab::Services => draw_services(frame, area, app),
        Tab::Settings => draw_settings(frame, area, app),
    }
}

fn draw_graph_inner(
    frame: &mut Frame,
    area: Rect,
    data: &std::collections::VecDeque<u64>,
    max: u64,
    color: Color,
) {
    use ratatui::widgets::canvas::{Canvas, Line as CanvasLine};

    if data.is_empty() || area.width == 0 || area.height == 0 {
        return;
    }
    let max = max.max(1) as f64;
    let last = (data.len().saturating_sub(1)).max(1) as f64;
    let grid = Color::Rgb(46, 54, 66);
    let fill = mix_color(color, Color::Rgb(8, 12, 16), 0.58);
    let values: Vec<f64> = data
        .iter()
        .map(|value| (*value).min(max as u64) as f64)
        .collect();
    let points: Vec<(f64, f64)> = values
        .iter()
        .enumerate()
        .map(|(i, value)| (i as f64, *value))
        .collect();
    let value_at = |x: f64| -> f64 {
        let index = x.floor().max(0.0) as usize;
        let frac = (x - index as f64).clamp(0.0, 1.0);
        let a = values.get(index).copied().unwrap_or(0.0);
        let b = values.get(index + 1).copied().unwrap_or(a);
        a + (b - a) * frac
    };

    let canvas = Canvas::default()
        .x_bounds([0.0, last])
        .y_bounds([0.0, max])
        .paint(|ctx| {
            for y in [0.25, 0.5, 0.75] {
                ctx.draw(&CanvasLine {
                    x1: 0.0,
                    y1: max * y,
                    x2: last,
                    y2: max * y,
                    color: grid,
                });
            }
            let steps = (last * 4.0).ceil() as usize;
            for i in 0..=steps {
                let x = i as f64 / 4.0;
                let y = value_at(x);
                ctx.draw(&CanvasLine {
                    x1: x,
                    y1: 0.0,
                    x2: x,
                    y2: y,
                    color: fill,
                });
            }
            for pair in points.windows(2) {
                for offset in [-0.5, 0.0, 0.5] {
                    ctx.draw(&CanvasLine {
                        x1: pair[0].0,
                        y1: (pair[0].1 + offset).max(0.0),
                        x2: pair[1].0,
                        y2: (pair[1].1 + offset).max(0.0),
                        color,
                    });
                }
            }
            if let Some((x, y)) = points.last() {
                ctx.draw(&CanvasLine {
                    x1: *x,
                    y1: 0.0,
                    x2: *x,
                    y2: *y,
                    color,
                });
            }
        });
    frame.render_widget(canvas, area);
}

fn draw_dashboard(frame: &mut Frame, area: Rect, app: &App) {
    let rows = Layout::vertical([
        Constraint::Ratio(1, 3),
        Constraint::Ratio(1, 3),
        Constraint::Ratio(1, 3),
    ])
    .spacing(1)
    .split(area);

    let top = Layout::horizontal([Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)])
        .spacing(1)
        .split(rows[0]);
    draw_cpu(frame, top[0], app);
    draw_memory(frame, top[1], app);

    let middle = Layout::horizontal([Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)])
        .spacing(1)
        .split(rows[1]);
    draw_network_graph(frame, middle[0], app);
    draw_temps(frame, middle[1], app);

    let bottom = Layout::horizontal([Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)])
        .spacing(1)
        .split(rows[2]);
    draw_battery_panel(frame, bottom[0], app);
    draw_asus_panel(frame, bottom[1], app);
}

fn draw_cpu(frame: &mut Frame, area: Rect, app: &App) {
    let title = match app.snap.cpu_mhz {
        Some(mhz) => format!(" CPU {mhz:.2} GHz "),
        None => " CPU ".to_string(),
    };
    let block = panel(title);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let compact = inner.width < 48;
    let cell_width = if compact { 7 } else { 9 };
    let compact_per_row = (inner.width as usize / cell_width).clamp(2, 8);
    let max_core_rows = inner.height.saturating_sub(3).max(1) as usize;
    let compact_rows = app.snap.cpu_cores.len().div_ceil(compact_per_row);
    let (per_row, tiny) = if compact_rows <= max_core_rows {
        (compact_per_row, false)
    } else {
        ((inner.width as usize / 3).clamp(2, 8), true)
    };
    let wanted_rows = app.snap.cpu_cores.len().div_ceil(per_row).clamp(1, 6);
    let core_rows = wanted_rows.min(max_core_rows) as u16;
    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(2),
        Constraint::Length(core_rows),
    ])
    .split(inner);

    let cpu_temp = app
        .snap
        .temps
        .iter()
        .find(|(label, _)| label.to_ascii_lowercase().contains("tctl"))
        .map(|(_, value)| *value)
        .or(app.snap.max_temp);
    let graph_max = app.cpu_hist.iter().copied().max().unwrap_or(0).max(10);
    let mut summary = vec![
        Span::styled(
            format!(" {:.1}%", app.snap.cpu_pct),
            Style::new().fg(pal().good).bold(),
        ),
        Span::styled(
            format!(
                "  Tctl {}",
                cpu_temp
                    .map(|v| format!("{v:.1}°C"))
                    .unwrap_or_else(|| "-".into())
            ),
            Style::new().fg(pal().dim),
        ),
    ];
    if inner.width >= 56
        && let Some(mhz) = app.snap.cpu_mhz_max
    {
        summary.push(Span::styled(
            format!("  峰 {mhz:.2} GHz"),
            Style::new().fg(pal().text),
        ));
    }
    if inner.width >= 40
        && let Some(watts) = app.snap.cpu_power_w
    {
        summary.push(Span::styled(
            format!("  功耗 {watts:.1} W"),
            Style::new().fg(pal().warn),
        ));
    }
    if inner.width >= 56 {
        summary.push(Span::styled(
            format!(
                "  负载 {:.2} {:.2} {:.2}",
                app.snap.load1, app.snap.load5, app.snap.load15
            ),
            Style::new().fg(pal().dim),
        ));
    }
    if inner.width >= 74 {
        summary.push(Span::styled(
            format!("  量程 {graph_max}%"),
            Style::new().fg(pal().dim),
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(summary)), rows[0]);

    draw_graph_inner(frame, rows[1], &app.cpu_hist, graph_max, pal().accent);

    if app.snap.cpu_cores.is_empty() {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                " 等待核心采样…",
                Style::new().fg(pal().dim),
            ))),
            rows[2],
        );
    } else {
        let mut lines = Vec::new();
        for chunk in app.snap.cpu_cores.chunks(per_row) {
            let mut spans = Vec::new();
            for (offset, value) in chunk.iter().enumerate() {
                let index = lines.len() * per_row + offset;
                let label = if tiny {
                    format!("{index:02} ")
                } else if compact {
                    format!("C{index:02}{value:>3.0}% ")
                } else {
                    let bar = ["▁", "▂", "▃", "▄", "▅", "▆", "▇", "█"]
                        [((*value / 100.0 * 7.0).round() as usize).min(7)];
                    format!("C{index:02}{bar}{value:>3.0}% ")
                };
                spans.push(Span::styled(label, Style::new().fg(load_color(*value))));
            }
            lines.push(Line::from(spans));
        }
        frame.render_widget(Paragraph::new(Text::from(lines)), rows[2]);
    }
}

fn draw_memory(frame: &mut Frame, area: Rect, app: &App) {
    let block = panel(" 内存 ");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(1),
    ])
    .split(inner);

    let total = app.snap.mem_total.max(1);
    let used = app.snap.mem_used.min(total);
    let cached = app.snap.mem_cached.min(total.saturating_sub(used));
    let free = total.saturating_sub(used + cached);
    let cells = inner.width as usize;
    let used_cells = ((used as f64 / total as f64) * cells as f64).round() as usize;
    let cached_cells = ((cached as f64 / total as f64) * cells as f64).round() as usize;
    let used_cells = used_cells.min(cells);
    let cached_cells = cached_cells.min(cells.saturating_sub(used_cells));
    let free_cells = cells.saturating_sub(used_cells + cached_cells);

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("█".repeat(used_cells), Style::new().fg(pal().chart)),
            Span::styled(
                "█".repeat(cached_cells),
                Style::new().fg(mix_color(pal().chart, pal().surface, 0.45)),
            ),
            Span::styled("░".repeat(free_cells), Style::new().fg(pal().faint)),
        ])),
        rows[0],
    );
    let mut legend = vec![Span::styled(
        format!(
            " 已用 {} / {}",
            util::human_bytes(used as f64),
            util::human_bytes(total as f64)
        ),
        Style::new().fg(pal().text),
    )];
    if inner.width >= 55 {
        legend.push(Span::styled(
            format!("   缓存 {}", util::human_bytes(cached as f64)),
            Style::new().fg(pal().dim),
        ));
    }
    if app.snap.swap_total > 0 {
        if inner.width >= 72 {
            legend.push(Span::styled(
                format!(
                    "   交换 {:.1}/{:.1}G",
                    app.snap.swap_used as f64 / 1e9,
                    app.snap.swap_total as f64 / 1e9
                ),
                Style::new().fg(pal().dim),
            ));
        }
    } else if inner.width >= 55 {
        legend.push(Span::styled(
            format!("   空闲 {}", util::human_bytes(free as f64)),
            Style::new().fg(pal().dim),
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(legend)), rows[1]);
    draw_graph_inner(frame, rows[2], &app.mem_hist, 100, pal().chart);
}

fn link_label(mbps: u64) -> String {
    if mbps >= 1000 {
        let value = mbps as f64 / 1000.0;
        format!("{value:.1} Gb/s")
    } else {
        format!("{mbps} Mb/s")
    }
}

fn draw_network_graph(frame: &mut Frame, area: Rect, app: &App) {
    let active = app
        .snap
        .net_ifaces
        .iter()
        .find(|iface| iface.physical && iface.operstate == "up");
    let title = match active {
        Some(iface) => format!(
            " 网络 {} {} ",
            iface.name,
            iface
                .speed_mbps
                .map(link_label)
                .unwrap_or_else(|| "-".to_string())
        ),
        None => " 网络 ".to_string(),
    };
    let block = panel(title);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let parts = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
        Constraint::Min(1),
    ])
    .split(inner);

    let rx_peak = app.rx_hist.iter().copied().max().unwrap_or(0).max(1);
    let tx_peak = app.tx_hist.iter().copied().max().unwrap_or(0).max(1);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" ↓ ", Style::new().fg(pal().accent)),
            Span::styled(
                util::human_rate_with(app.snap.rx_bps, rate_style(app)),
                Style::new().fg(pal().text),
            ),
            Span::styled(
                format!(
                    "   峰值 {}",
                    util::human_rate_with(rx_peak as f64, rate_style(app))
                ),
                Style::new().fg(pal().dim),
            ),
        ])),
        parts[0],
    );
    draw_graph_inner(frame, parts[1], &app.rx_hist, rx_peak, pal().accent);

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" ↑ ", Style::new().fg(pal().warn)),
            Span::styled(
                util::human_rate_with(app.snap.tx_bps, rate_style(app)),
                Style::new().fg(pal().text),
            ),
            Span::styled(
                format!(
                    "   峰值 {}",
                    util::human_rate_with(tx_peak as f64, rate_style(app))
                ),
                Style::new().fg(pal().dim),
            ),
        ])),
        parts[2],
    );
    draw_graph_inner(frame, parts[3], &app.tx_hist, tx_peak, pal().warn);
}

fn draw_temps(frame: &mut Frame, area: Rect, app: &App) {
    let block = panel(" 温度 ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if app.snap.temps.is_empty() {
        frame.render_widget(
            Paragraph::new(Span::styled(
                if app.snap_loading {
                    "  正在读取温度传感器…"
                } else {
                    "  没有可读的温度传感器"
                },
                Style::new().fg(pal().dim),
            )),
            inner,
        );
        return;
    }

    let bar_width = 8usize;
    let label_width = inner.width.saturating_sub(18).max(8) as usize;
    let lines: Vec<Line> = app
        .snap
        .temps
        .iter()
        .take(inner.height as usize)
        .map(|(label, value)| {
            let ratio = ((*value - 30.0) / 70.0).clamp(0.0, 1.0);
            let filled = (ratio * bar_width as f32).round() as usize;
            let bar = format!("{}{}", "█".repeat(filled), "░".repeat(bar_width - filled));
            let label = util::truncate(label, label_width);
            let pad = label_width.saturating_sub(UnicodeWidthStr::width(label.as_str()));
            Line::from(vec![
                Span::styled(
                    format!("  {label}{}", " ".repeat(pad)),
                    Style::new().fg(pal().text),
                ),
                Span::styled(bar, Style::new().fg(temp_color(*value))),
                Span::styled(
                    format!(" {value:>5.1}°C"),
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

    let capacity = app.battery.capacity.unwrap_or(0);
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

    // 低高度下只保留最关键的读数，避免出现半截行。
    if inner.height < 5 {
        let cells = inner.width.saturating_sub(6) as usize;
        let filled = ((capacity as f64 / 100.0) * cells as f64).round() as usize;
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(
                    "█".repeat(filled.min(cells)),
                    Style::new().fg(battery_color(app)),
                ),
                Span::styled(
                    "░".repeat(cells.saturating_sub(filled)),
                    Style::new().fg(pal().faint),
                ),
                Span::styled(
                    format!("  {capacity}%"),
                    Style::new().fg(battery_color(app)).bold(),
                ),
            ])),
            Rect::new(inner.x, inner.y, inner.width, 1),
        );
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                format!(
                    "  {}   {power}   健康 {health}   限 {limit}",
                    app.battery.status_zh()
                ),
                Style::new().fg(pal().dim),
            ))),
            Rect::new(inner.x, inner.y + 1, inner.width, 1),
        );
        return;
    }

    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .split(inner);
    let cells = inner.width.saturating_sub(6) as usize;
    let filled = ((capacity as f64 / 100.0) * cells as f64).round() as usize;
    let bar = format!(
        "{}{}",
        "█".repeat(filled.min(cells)),
        "░".repeat(cells.saturating_sub(filled))
    );
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(bar, Style::new().fg(battery_color(app))),
            Span::styled(
                format!("  {capacity}%"),
                Style::new().fg(battery_color(app)).bold(),
            ),
        ])),
        rows[0],
    );

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                format!("  {}", app.battery.status_zh()),
                Style::new().fg(if app.battery.ac_online == Some(true) {
                    pal().good
                } else {
                    pal().warn
                }),
            ),
            Span::styled(
                match app.battery.ac_online {
                    Some(true) => "   已接电源",
                    Some(false) => "   电池供电",
                    None => "",
                },
                Style::new().fg(pal().dim),
            ),
        ])),
        rows[1],
    );

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
            Span::styled("  功率 ", Style::new().fg(pal().dim)),
            Span::styled(power, Style::new().fg(pal().text)),
            Span::styled("   健康 ", Style::new().fg(pal().dim)),
            Span::styled(health, Style::new().fg(pal().text)),
            Span::styled("   上限 ", Style::new().fg(pal().dim)),
            Span::styled(limit, Style::new().fg(pal().text)),
        ])),
        rows[2],
    );

    if let (Some(full), Some(design)) = (
        app.battery.energy_full_wh,
        app.battery.energy_full_design_wh,
    ) {
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("  容量 ", Style::new().fg(pal().dim)),
                Span::styled(
                    format!("{full:.1} / {design:.1} Wh"),
                    Style::new().fg(pal().text),
                ),
                Span::styled(
                    format!(
                        "   循环 {} 次",
                        app.battery
                            .cycle_count
                            .map(|v| v.to_string())
                            .unwrap_or_else(|| "-".into())
                    ),
                    Style::new().fg(pal().dim),
                ),
            ])),
            rows[3],
        );
    }

    let mut tail = vec![Span::styled(
        format!(
            "  电压 {}",
            app.battery
                .voltage_v
                .map(|v| format!("{v:.2} V"))
                .unwrap_or_else(|| "-".into())
        ),
        Style::new().fg(pal().dim),
    )];
    if let Some(hours) = app.battery.hours_left() {
        tail.push(Span::styled(
            format!("   预计剩余 {hours:.1} h"),
            Style::new().fg(pal().warn),
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(tail)), rows[4]);

    if !app.battery.manufacturer.is_empty() || !app.battery.model.is_empty() {
        let maker = match (
            app.battery.manufacturer.is_empty(),
            app.battery.model.is_empty(),
        ) {
            (false, false) => format!("{} {}", app.battery.manufacturer, app.battery.model),
            (false, true) => app.battery.manufacturer.clone(),
            (true, false) => app.battery.model.clone(),
            (true, true) => String::new(),
        };
        let technology = if app.battery.technology.is_empty() {
            String::new()
        } else {
            format!("   {}", app.battery.technology)
        };
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("  型号 ", Style::new().fg(pal().dim)),
                Span::styled(
                    util::truncate(&maker, inner.width.saturating_sub(20) as usize),
                    Style::new().fg(pal().text),
                ),
                Span::styled(technology, Style::new().fg(pal().dim)),
            ])),
            rows[5],
        );
    }
}

fn draw_asus_panel(frame: &mut Frame, area: Rect, app: &App) {
    let block = panel(" ASUS ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines = Vec::new();
    if let Some(state) = app.profile {
        lines.push(Line::from(vec![
            Span::styled("  当前 ", Style::new().fg(pal().dim)),
            Span::styled(
                state.active.zh(),
                Style::new().fg(profile_color(state.active)).bold(),
            ),
            Span::styled(
                format!("   AC {} / 电池 {}", state.ac.zh(), state.battery.zh()),
                Style::new().fg(pal().dim),
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
        Span::styled("  功耗  ", Style::new().fg(pal().dim)),
        Span::styled(power.join(" / "), Style::new().fg(pal().text)),
    ]));

    if !app.snap.fans.is_empty() {
        let fans: Vec<String> = app
            .snap
            .fans
            .iter()
            .take(2)
            .map(|(label, rpm)| format!("{label} {rpm}"))
            .collect();
        lines.push(Line::from(vec![
            Span::styled("  风扇  ", Style::new().fg(pal().dim)),
            Span::styled(
                format!("{} RPM", fans.join(" · ")),
                Style::new().fg(pal().accent),
            ),
        ]));
    }

    if app.snap.disk_total > 0 {
        let pct = app.snap.disk_used as f64 / app.snap.disk_total as f64 * 100.0;
        lines.push(Line::from(vec![
            Span::styled("  磁盘  ", Style::new().fg(pal().dim)),
            Span::styled(
                format!(
                    "{} / {}  {pct:.0}%",
                    util::human_bytes(app.snap.disk_used as f64),
                    util::human_bytes(app.snap.disk_total as f64)
                ),
                Style::new().fg(if pct >= 90.0 { pal().bad } else { pal().text }),
            ),
        ]));
    }

    let width = inner.width as usize;
    let cpu = format!(
        "{} ({}C/{}T)",
        app.hw.cpu_model, app.hw.cores, app.hw.threads
    );
    lines.push(Line::from(vec![
        Span::styled("  CPU   ", Style::new().fg(pal().dim)),
        Span::styled(
            util::truncate(&cpu, width.saturating_sub(8)),
            Style::new().fg(pal().text),
        ),
    ]));
    let gpu_load = app
        .snap
        .gpu_load
        .map(|load| format!(" · {load}%"))
        .unwrap_or_default();
    lines.push(Line::from(vec![
        Span::styled("  显卡  ", Style::new().fg(pal().dim)),
        Span::styled(
            format!(
                "{}{gpu_load}",
                util::truncate(&app.gpu_names().join(" / "), width.saturating_sub(12))
            ),
            Style::new().fg(pal().text),
        ),
    ]));

    if let Some(mode) = app.gpu_mode {
        lines.push(Line::from(vec![
            Span::styled("  模式  ", Style::new().fg(pal().dim)),
            Span::styled(mode.zh(), Style::new().fg(pal().text)),
        ]));
    }

    lines.push(Line::from(vec![
        Span::styled("  调参 ", Style::new().fg(pal().dim)),
        Span::styled(
            if app.tuning {
                "持久化开启"
            } else {
                "持久化关闭"
            },
            Style::new().fg(if app.tuning { pal().good } else { pal().warn }),
        ),
    ]));

    // 矮窗口优先保留前面的关键信息，避免出现半截行。
    lines.truncate(inner.height as usize);
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
        Span::styled("  当前 ", Style::new().fg(pal().dim)),
        Span::styled(
            format!(" {} ", active.zh()),
            Style::new()
                .fg(Color::Black)
                .bg(profile_color(active))
                .bold(),
        ),
        Span::styled("   AC 默认 ", Style::new().fg(pal().dim)),
        Span::styled(ac.zh(), Style::new().fg(profile_color(ac)).bold()),
        Span::styled("   电池 ", Style::new().fg(pal().dim)),
        Span::styled(bat.zh(), Style::new().fg(profile_color(bat)).bold()),
        Span::styled("   GPU ", Style::new().fg(pal().dim)),
        Span::styled(gpu, Style::new().fg(pal().text)),
        Span::styled("   ", Style::new()),
        Span::styled(
            tuning,
            Style::new().fg(if app.tuning { pal().good } else { pal().warn }),
        ),
    ]);
    frame.render_widget(Paragraph::new(line), inner);
}

fn draw_profiles(frame: &mut Frame, area: Rect, app: &App) {
    let focused = app.perf_focus == crate::app::PerfFocus::Profiles;
    record_mouse(
        app,
        area,
        MouseAction::PerfPanel(crate::app::PerfFocus::Profiles),
    );
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
            Span::styled(badge, style.fg(pal().dim)),
        ]));
    }

    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        " 默认模式（点击设置）",
        Style::new().fg(pal().dim).bold(),
    )));

    let ac = state.map(|s| s.ac).unwrap_or(Profile::Balanced);
    let bat = state.map(|s| s.battery).unwrap_or(Profile::Balanced);
    let mut ac_spans = vec![Span::styled("   AC   ", Style::new().fg(pal().dim))];
    let mut bat_spans = vec![Span::styled("   电池 ", Style::new().fg(pal().dim))];
    for profile in Profile::ALL.iter() {
        let style = if *profile == ac {
            Style::new()
                .fg(Color::Black)
                .bg(profile_color(*profile))
                .bold()
        } else {
            Style::new().fg(profile_color(*profile))
        };
        ac_spans.push(Span::styled(format!(" {} ", profile.zh()), style));
        ac_spans.push(Span::raw(" "));
        let style = if *profile == bat {
            Style::new()
                .fg(Color::Black)
                .bg(profile_color(*profile))
                .bold()
        } else {
            Style::new().fg(profile_color(*profile))
        };
        bat_spans.push(Span::styled(format!(" {} ", profile.zh()), style));
        bat_spans.push(Span::raw(" "));
    }

    let ac_y = inner.y + lines.len() as u16;
    let mut x = inner.x + 6;
    for index in 0..Profile::ALL.len() {
        record_mouse(
            app,
            Rect::new(x, ac_y, 6, 1),
            MouseAction::SetAcDefault(index),
        );
        x += 7;
    }
    lines.push(Line::from(ac_spans));
    let bat_y = ac_y + 1;
    let mut x = inner.x + 6;
    for index in 0..Profile::ALL.len() {
        record_mouse(
            app,
            Rect::new(x, bat_y, 6, 1),
            MouseAction::SetBatteryDefault(index),
        );
        x += 7;
    }
    lines.push(Line::from(bat_spans));
    lines.push(Line::raw(""));

    lines.push(Line::from(Span::styled(
        " Enter 切换   a AC 默认   b 电池默认",
        Style::new().fg(pal().dim),
    )));
    lines.push(Line::from(Span::styled(
        " f 切换焦点到功耗",
        Style::new().fg(pal().dim),
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
    record_mouse(
        app,
        area,
        MouseAction::PerfPanel(crate::app::PerfFocus::Gpu),
    );
    let block = panel_focused(" GPU 模式 ", app.perf_focus == crate::app::PerfFocus::Gpu);
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
                    Style::new().bg(pal().surface).fg(pal().accent).bold()
                } else if hovered {
                    Style::new().bg(pal().surface_hover).fg(pal().text)
                } else {
                    Style::new().fg(pal().text)
                },
            ),
            Span::styled(badge, Style::new().fg(pal().dim)),
        ]));
    }
    lines.push(Line::from(Span::styled(
        " g 选择   G 应用（需重启）",
        Style::new().fg(pal().dim),
    )));

    frame.render_widget(Paragraph::new(Text::from(lines)), inner);
}

fn draw_presets(frame: &mut Frame, area: Rect, app: &App) {
    record_mouse(
        app,
        area,
        MouseAction::PerfPanel(crate::app::PerfFocus::Presets),
    );
    let block = panel_focused(
        " 功耗预设 ",
        app.perf_focus == crate::app::PerfFocus::Presets,
    );
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
            Style::new().bg(pal().surface).fg(pal().accent).bold()
        } else if hovered {
            Style::new().bg(pal().surface_hover).fg(pal().text)
        } else {
            Style::new().fg(pal().text)
        };
        lines.push(Line::from(vec![
            Span::styled(format!(" {marker} {:<4}", preset.name), style),
            Span::styled(
                format!(
                    "PL {} / {} / {} W   dGPU {} W   {}°C",
                    preset.pl1, preset.pl2, preset.pl3, preset.gpu_tgp, preset.gpu_temp
                ),
                if selected {
                    Style::new().fg(pal().text)
                } else {
                    Style::new().fg(pal().dim)
                },
            ),
        ]));
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        " [ ] 选择预设   p 套用",
        Style::new().fg(pal().dim),
    )));

    frame.render_widget(Paragraph::new(Text::from(lines)), inner);
}

fn draw_power_fields(frame: &mut Frame, area: Rect, app: &App) {
    let focused = app.perf_focus == crate::app::PerfFocus::Power;
    record_mouse(
        app,
        area,
        MouseAction::PerfPanel(crate::app::PerfFocus::Power),
    );
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
            Style::new().fg(pal().text)
        };
        let marker = if selected { "▶" } else { " " };
        let range = format!("[{}..{}]", field.min, field.max);
        lines.push(Line::from(vec![
            Span::styled(format!(" {marker} {:<16}", field.label), style),
            Span::styled(
                format!("{:>4} {}", field.value, field.unit),
                style.fg(if selected { pal().accent } else { pal().text }),
            ),
            Span::styled(format!("  {range}"), Style::new().fg(pal().dim)),
        ]));
    }

    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        " h/l 调整（H/L 每次 5）   Enter/a 应用",
        Style::new().fg(pal().dim),
    )));
    lines.push(Line::from(Span::styled(
        " 写入当前供电模式的 tuning，重启由 asusd 恢复",
        Style::new().fg(pal().dim),
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

fn draw_gpu(frame: &mut Frame, area: Rect, gpus: &[gpu::Status], loading: bool) {
    if gpus.is_empty() {
        let block = panel(" 显卡 ");
        let inner = block.inner(area);
        frame.render_widget(block, area);
        frame.render_widget(
            Paragraph::new(Span::styled(
                if loading {
                    "  正在检测显卡…"
                } else {
                    "  没有检测到 GPU（/sys/class/drm 为空）"
                },
                Style::new().fg(pal().dim),
            )),
            inner,
        );
        return;
    }
    let count = gpus.len() as u32;
    let constraints: Vec<Constraint> = (0..count).map(|_| Constraint::Ratio(1, count)).collect();
    let rows = Layout::vertical(constraints).spacing(1).split(area);
    for (index, status) in gpus.iter().enumerate() {
        draw_gpu_card(frame, rows[index], status);
    }
}

fn draw_gpu_card(frame: &mut Frame, area: Rect, status: &gpu::Status) {
    let accent = gpu_vendor_color(status.gpu.vendor);
    let block = panel(Line::from(vec![
        Span::styled(
            format!(" {} ", status.gpu.vendor.label()),
            Style::new().fg(Color::Black).bg(accent).bold(),
        ),
        Span::raw(" "),
        Span::styled(status.gpu.name.clone(), Style::new().fg(pal().text).bold()),
    ]));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width < 24 || inner.height == 0 {
        return;
    }

    let label_w = 8usize;
    let bar_w = (inner.width as usize)
        .saturating_sub(label_w + 24)
        .clamp(4, 20);
    let dim = |text: String| Span::styled(text, Style::new().fg(pal().dim));
    let mut lines: Vec<Line> = Vec::new();

    let driver = if status.gpu.driver.is_empty() {
        "-".to_string()
    } else {
        status.gpu.driver.clone()
    };
    let mut head = vec![kv_label("驱动", label_w)];
    head.push(Span::styled(driver, Style::new().fg(pal().text)));
    if !status.stats.driver_version.is_empty() {
        head.push(dim(format!(" {}", status.stats.driver_version)));
    }
    if !status.stats.pcie.is_empty() {
        head.push(dim(format!("   {}", status.stats.pcie)));
    }
    lines.push(Line::from(head));

    let mut temp_row = vec![kv_label("温度", label_w)];
    match status.stats.temp {
        Some(value) => temp_row.push(Span::styled(
            format!("{value:.1}°C"),
            Style::new().fg(temp_color(value)).bold(),
        )),
        None => temp_row.push(Span::styled("-", Style::new().fg(pal().dim))),
    }
    if let Some(value) = status.stats.hotspot {
        temp_row.push(dim(format!("   热点 {value:.1}°C")));
    }
    if let Some(value) = status.stats.mem_temp {
        temp_row.push(dim(format!("   显存 {value:.1}°C")));
    }
    lines.push(Line::from(temp_row));

    if let Some(load) = status.stats.load {
        lines.push(Line::from(vec![
            kv_label("负载", label_w),
            Span::styled(
                bar_text(load as f64 / 100.0, bar_w),
                Style::new().fg(load_color(load as f32)),
            ),
            Span::styled(format!(" {load:>3}%"), Style::new().fg(pal().text)),
        ]));
    }

    if let Some(total) = status.stats.mem_total {
        let used = status.stats.mem_used.unwrap_or(0);
        let ratio = if total > 0 {
            used as f64 / total as f64
        } else {
            0.0
        };
        lines.push(Line::from(vec![
            kv_label("显存", label_w),
            Span::styled(bar_text(ratio, bar_w), Style::new().fg(pal().chart)),
            Span::styled(
                format!(
                    " {} / {}",
                    util::human_bytes(used as f64),
                    util::human_bytes(total as f64)
                ),
                Style::new().fg(pal().text),
            ),
        ]));
    }

    if let Some(shared) = status.stats.shared_used {
        lines.push(Line::from(vec![
            kv_label("共享显存", label_w),
            Span::styled(
                util::human_bytes(shared as f64),
                Style::new().fg(pal().text),
            ),
            dim("   (GTT)".to_string()),
        ]));
    }

    let mut power_row = vec![kv_label("功耗", label_w)];
    power_row.push(Span::styled(
        status
            .stats
            .power
            .map(|value| format!("{value:.1} W"))
            .unwrap_or_else(|| "-".to_string()),
        Style::new().fg(pal().text),
    ));
    if let Some(clock) = status.stats.clock_mhz {
        power_row.push(dim(format!("   {clock} MHz")));
    }
    lines.push(Line::from(power_row));

    let (runtime, runtime_color) = runtime_zh(&status.stats.runtime);
    lines.push(Line::from(vec![
        kv_label("状态", label_w),
        Span::styled(runtime, Style::new().fg(runtime_color)),
        dim(format!("   {}", status.gpu.pci)),
    ]));

    let lines: Vec<Line> = lines.into_iter().take(inner.height as usize).collect();
    frame.render_widget(Paragraph::new(Text::from(lines)), inner);
}

fn kv_label(text: &str, width: usize) -> Span<'static> {
    let pad = width.saturating_sub(UnicodeWidthStr::width(text));
    Span::styled(
        format!("  {text}{}", " ".repeat(pad)),
        Style::new().fg(pal().dim),
    )
}

fn gpu_vendor_color(vendor: gpu::Vendor) -> Color {
    match vendor {
        gpu::Vendor::Nvidia => Color::Rgb(118, 185, 0),
        gpu::Vendor::Amd => Color::Rgb(230, 90, 80),
        gpu::Vendor::Intel => Color::Rgb(80, 160, 230),
        gpu::Vendor::Other => pal().accent,
    }
}

fn runtime_zh(runtime: &str) -> (String, Color) {
    match runtime {
        "active" => ("活动".into(), pal().good),
        "suspended" => ("休眠（未唤醒）".into(), pal().warn),
        "suspending" => ("正在休眠".into(), pal().warn),
        "resuming" => ("正在唤醒".into(), pal().warn),
        "" => ("未知".into(), pal().dim),
        other => (other.to_string(), pal().dim),
    }
}

fn draw_battery(frame: &mut Frame, area: Rect, app: &App) {
    let rows = Layout::vertical([Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)])
        .spacing(1)
        .split(area);
    let top = Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)])
        .spacing(1)
        .split(rows[0]);
    draw_battery_status(frame, top[0], app);
    draw_battery_health(frame, top[1], app);
    draw_battery_limit(frame, rows[1], app);
}

fn draw_battery_status(frame: &mut Frame, area: Rect, app: &App) {
    let block = panel(" 电池状态 ");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .split(inner);

    let capacity = app.battery.capacity.unwrap_or(0);
    let width = inner.width.saturating_sub(8) as usize;
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                bar_text(capacity as f64 / 100.0, width),
                Style::new().fg(battery_color(app)),
            ),
            Span::styled(
                format!("  {capacity}%"),
                Style::new().fg(battery_color(app)).bold(),
            ),
        ])),
        rows[0],
    );
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("  状态  ", Style::new().fg(pal().dim)),
            Span::styled(
                app.battery.status_zh(),
                Style::new().fg(if app.battery.ac_online == Some(true) {
                    pal().good
                } else {
                    pal().warn
                }),
            ),
            Span::styled(
                match app.battery.ac_online {
                    Some(true) => "   已接电源",
                    Some(false) => "   电池供电",
                    None => "",
                },
                Style::new().fg(pal().dim),
            ),
        ])),
        rows[1],
    );
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("  功率  ", Style::new().fg(pal().dim)),
            Span::styled(
                app.battery
                    .power_w
                    .map(|v| format!("{v:.1} W"))
                    .unwrap_or_else(|| "-".into()),
                Style::new().fg(pal().text),
            ),
            Span::styled("   电压  ", Style::new().fg(pal().dim)),
            Span::styled(
                app.battery
                    .voltage_v
                    .map(|v| format!("{v:.2} V"))
                    .unwrap_or_else(|| "-".into()),
                Style::new().fg(pal().text),
            ),
        ])),
        rows[2],
    );
    frame.render_widget(Line::from(Span::raw("")), rows[3]);

    let lines = vec![
        section("使用建议"),
        Line::from(Span::styled(
            "  长期插电建议上限 60–80%",
            Style::new().fg(pal().text),
        )),
        Line::from(Span::styled(
            "  高温会加速电池老化，注意散热",
            Style::new().fg(pal().text),
        )),
    ];
    frame.render_widget(Paragraph::new(Text::from(lines)), rows[4]);
}

fn draw_battery_health(frame: &mut Frame, area: Rect, app: &App) {
    let block = panel(" 健康与容量 ");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let mut lines = Vec::new();

    let health = app.battery.health_pct().unwrap_or(0.0);
    lines.push(Line::from(vec![
        Span::styled("  健康度  ", Style::new().fg(pal().dim)),
        Span::styled(
            bar_text(health as f64 / 100.0, 16),
            Style::new().fg(if health >= 80.0 {
                pal().good
            } else {
                pal().warn
            }),
        ),
        Span::styled(format!("  {health:.1}%"), Style::new().fg(pal().text)),
    ]));
    lines.push(Line::from(vec![
        Span::styled("  容量  ", Style::new().fg(pal().dim)),
        Span::styled(
            if let (Some(full), Some(design)) = (
                app.battery.energy_full_wh,
                app.battery.energy_full_design_wh,
            ) {
                format!("{full:.1} / {design:.1} Wh")
            } else {
                "-".into()
            },
            Style::new().fg(pal().text),
        ),
    ]));
    lines.push(Line::from(vec![
        Span::styled("  循环  ", Style::new().fg(pal().dim)),
        Span::styled(
            app.battery
                .cycle_count
                .map(|v| format!("{v} 次"))
                .unwrap_or_else(|| "-".into()),
            Style::new().fg(pal().text),
        ),
    ]));
    if !app.battery.manufacturer.is_empty() {
        lines.push(Line::from(vec![
            Span::styled("  厂商  ", Style::new().fg(pal().dim)),
            Span::styled(
                util::truncate(&app.battery.manufacturer, 30),
                Style::new().fg(pal().text),
            ),
        ]));
    }
    if !app.battery.model.is_empty() {
        lines.push(Line::from(vec![
            Span::styled("  型号  ", Style::new().fg(pal().dim)),
            Span::styled(
                util::truncate(&app.battery.model, 30),
                Style::new().fg(pal().text),
            ),
        ]));
    }
    if !app.battery.technology.is_empty() {
        lines.push(Line::from(vec![
            Span::styled("  技术  ", Style::new().fg(pal().dim)),
            Span::styled(app.battery.technology.clone(), Style::new().fg(pal().dim)),
        ]));
    }

    frame.render_widget(
        Paragraph::new(Text::from(lines)).wrap(Wrap { trim: false }),
        inner,
    );
}

fn draw_battery_limit(frame: &mut Frame, area: Rect, app: &App) {
    let block = panel(" 充电上限 ");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .split(inner);

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                bar_text(
                    app.limit_edit as f64 / 100.0,
                    inner.width.saturating_sub(10) as usize,
                ),
                Style::new().fg(if app.limit_dirty {
                    pal().warn
                } else {
                    pal().good
                }),
            ),
            Span::styled(
                format!("  {}%", app.limit_edit),
                Style::new()
                    .fg(if app.limit_dirty {
                        pal().warn
                    } else {
                        pal().good
                    })
                    .bold(),
            ),
        ])),
        rows[0],
    );
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("  当前上限  ", Style::new().fg(pal().dim)),
            Span::styled(
                app.battery
                    .limit
                    .map(|v| format!("{v}%"))
                    .unwrap_or_else(|| "-".into()),
                Style::new().fg(pal().text),
            ),
            Span::styled(
                if app.limit_dirty {
                    "   编辑值未应用（Enter 应用）"
                } else {
                    "   h/l 调整   Enter 应用"
                },
                Style::new().fg(pal().dim),
            ),
        ])),
        rows[1],
    );
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "  o 单次充满（忽略上限）   长期插电建议 60-80%",
            Style::new().fg(pal().dim),
        ))),
        rows[2],
    );
    let y = rows[3].y;
    let mut hints = Vec::new();
    if let Some(line) = action_bar(
        app,
        rows[3],
        y,
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
    frame.render_widget(Paragraph::new(Text::from(hints)), rows[3]);
}

fn bar_text(ratio: f64, width: usize) -> String {
    let width = width.max(1);
    let filled = ((ratio.clamp(0.0, 1.0)) * width as f64).round() as usize;
    format!(
        "{}{}",
        "█".repeat(filled.min(width)),
        "░".repeat(width.saturating_sub(filled))
    )
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
    use ratatui::widgets::canvas::{Canvas, Circle, Line as CanvasLine};

    let cpu_color = pal().accent;
    let gpu_color = pal().chart;
    let graph_bg = mix_color(pal().surface, Color::Rgb(4, 6, 9), 0.48);
    let grid = mix_color(pal().border, graph_bg, 0.35);
    let axis = pal().dim;

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
        Span::styled(" 风扇曲线 ", Style::new().fg(pal().accent).bold()),
        Span::styled("CPU ", Style::new().fg(cpu_color)),
        Span::styled("GPU ", Style::new().fg(gpu_color)),
        Span::styled(" 温度 → / PWM ↑", Style::new().fg(pal().dim)),
    ]);

    let canvas = Canvas::default()
        .block(panel(title))
        .background_color(graph_bg)
        .x_bounds([20.0, 100.0])
        .y_bounds([0.0, 255.0])
        .paint(|ctx| {
            // 先铺曲线下方的暗色面积，网格和曲线再覆盖上去。
            for (_name, points, color, _editing) in &curves {
                let coords: Vec<(f64, f64)> = points
                    .iter()
                    .map(|(temp, pwm)| (*temp as f64, *pwm as f64))
                    .collect();
                let Some(first) = coords.first() else {
                    continue;
                };
                let Some(last) = coords.last() else {
                    continue;
                };
                let fill = mix_color(*color, graph_bg, 0.80);
                let start = first.0.max(20.0);
                let end = last.0.min(100.0);
                if end <= start {
                    continue;
                }
                let steps = ((end - start) * 4.0).ceil() as usize;
                for i in 0..=steps {
                    let x = start + (end - start) * i as f64 / steps as f64;
                    let mut previous = coords[0];
                    let mut y = previous.1;
                    for pair in coords.windows(2) {
                        if x <= pair[1].0 {
                            let span = (pair[1].0 - pair[0].0).max(0.001);
                            let t = ((x - pair[0].0) / span).clamp(0.0, 1.0);
                            y = pair[0].1 + (pair[1].1 - pair[0].1) * t;
                            break;
                        }
                        previous = pair[1];
                        y = previous.1;
                    }
                    ctx.draw(&CanvasLine {
                        x1: x,
                        y1: 0.0,
                        x2: x,
                        y2: y,
                        color: fill,
                    });
                }
            }

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

            let label = |text: &'static str| Line::from(Span::styled(text, Style::new().fg(axis)));
            ctx.print(20.8, 245.0, label("255"));
            ctx.print(20.8, 124.0, label("128"));
            ctx.print(20.8, 7.0, label("0"));
            ctx.print(21.0, 2.0, label("20"));
            ctx.print(91.0, 2.0, label("100"));
            ctx.print(96.0, 245.0, label("°C"));

            for (_name, points, color, editing) in &curves {
                let coords: Vec<(f64, f64)> = points
                    .iter()
                    .map(|(temp, pwm)| (*temp as f64, *pwm as f64))
                    .collect();
                for pair in coords.windows(2) {
                    for offset in [-0.7, 0.0, 0.7] {
                        ctx.draw(&CanvasLine {
                            x1: pair[0].0,
                            y1: (pair[0].1 + offset).clamp(0.0, 255.0),
                            x2: pair[1].0,
                            y2: (pair[1].1 + offset).clamp(0.0, 255.0),
                            color: *color,
                        });
                    }
                }
                for (x, y) in &coords {
                    ctx.draw(&Circle::new(
                        *x,
                        *y,
                        1.6,
                        if *editing { Color::White } else { *color },
                    ));
                }
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
            pal().accent
        } else {
            pal().chart
        };
        let mut head = vec![
            Span::styled(
                format!("  {:<4} ", curve.fan),
                Style::new().fg(color).bold(),
            ),
            Span::styled(
                if curve.enabled { "启用" } else { "停用" },
                Style::new().fg(if curve.enabled { pal().good } else { pal().bad }),
            ),
        ];
        if let Some(rpm) = fan_rpm(app, &curve.fan) {
            head.push(Span::styled(
                format!("   {rpm} RPM"),
                Style::new().fg(pal().accent),
            ));
        }
        lines.push(Line::from(head));
        let temps: Vec<String> = curve.temp.iter().map(|temp| format!("{temp:>4}")).collect();
        let pwms: Vec<String> = curve.pwm.iter().map(|pwm| format!("{pwm:>4}")).collect();
        let percent: Vec<String> = curve
            .pwm
            .iter()
            .map(|pwm| format!("{:>3}%", *pwm as u32 * 100 / 255))
            .collect();
        lines.push(Line::from(vec![
            Span::styled("     温度 ", Style::new().fg(pal().dim)),
            Span::styled(temps.join(" "), Style::new().fg(pal().text)),
        ]));
        lines.push(Line::from(vec![
            Span::styled("     PWM  ", Style::new().fg(pal().dim)),
            Span::styled(pwms.join(" "), Style::new().fg(pal().text)),
        ]));
        lines.push(Line::from(vec![
            Span::styled("     占空 ", Style::new().fg(pal().dim)),
            Span::styled(percent.join(" "), Style::new().fg(pal().accent)),
        ]));
        lines.push(Line::raw(""));
    }

    lines.push(Line::from(Span::styled(
        " i 编辑 CPU   I 编辑 GPU   e 启用/停用",
        Style::new().fg(pal().dim),
    )));
    lines.push(Line::from(Span::styled(
        " d 恢复固件默认   s 保存当前为预设",
        Style::new().fg(pal().dim),
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
            row_style(selected, hovered).fg(if selected { Color::White } else { pal().text });
        let marker = if selected { "▶" } else { " " };
        lines.push(Line::from(vec![
            Span::styled(format!(" {marker} 点{}  ", index + 1), style),
            Span::styled(format!("{:>3}°C", edit.temp[index]), style),
            Span::styled(format!("  PWM {:>3}", edit.pwm[index]), style),
            Span::styled(
                format!("  {:>3}%", edit.percent(index)),
                style.fg(if selected { pal().accent } else { pal().dim }),
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
                Style::new().fg(pal().dim),
            )),
            Line::from(Span::styled(
                " c 切 CPU/GPU   a 写入   Esc 取消",
                Style::new().fg(pal().dim),
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
            Style::new().fg(pal().dim),
        )));
        lines.push(Line::raw(""));
        lines.push(Line::from(Span::styled(
            "  按 s 把当前已写入的曲线保存为预设",
            Style::new().fg(pal().dim),
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
                        Style::new().bg(pal().surface).fg(pal().accent).bold()
                    } else if hovered {
                        Style::new().bg(pal().surface_hover).fg(pal().text)
                    } else {
                        Style::new().fg(pal().text)
                    },
                ),
                Span::styled(
                    format!("  [{}]", preset.profile.zh()),
                    Style::new().fg(pal().dim),
                ),
            ]));
        }
        lines.push(Line::raw(""));
        lines.push(Line::from(Span::styled(
            " p 应用   x 删除   s 覆盖保存",
            Style::new().fg(pal().dim),
        )));
    }

    frame.render_widget(Paragraph::new(Text::from(lines)), inner);
}

fn hex_rgb(hex: &str) -> (u8, u8, u8) {
    let value = u32::from_str_radix(hex.trim_start_matches('#'), 16).unwrap_or(0xffffff);
    (
        ((value >> 16) & 0xff) as u8,
        ((value >> 8) & 0xff) as u8,
        (value & 0xff) as u8,
    )
}

fn gradient_line(width: usize, from: &str, to: &str) -> Line<'static> {
    let (ar, ag, ab) = hex_rgb(from);
    let (br, bg, bb) = hex_rgb(to);
    let width = width.max(1);
    let mut spans = Vec::new();
    for i in 0..width {
        let t = if width <= 1 {
            0.0
        } else {
            i as f64 / (width - 1) as f64
        };
        let r = (ar as f64 + (br as f64 - ar as f64) * t) as u8;
        let g = (ag as f64 + (bg as f64 - ag as f64) * t) as u8;
        let b = (ab as f64 + (bb as f64 - ab as f64) * t) as u8;
        spans.push(Span::styled("█", Style::new().fg(Color::Rgb(r, g, b))));
    }
    Line::from(spans)
}

fn draw_lighting(frame: &mut Frame, area: Rect, app: &App) {
    let rows = Layout::vertical([Constraint::Length(11), Constraint::Min(0)]).split(area);
    let cols =
        Layout::horizontal([Constraint::Percentage(46), Constraint::Percentage(54)]).split(rows[0]);

    draw_lighting_status(frame, cols[0], app);
    draw_lighting_colors(frame, cols[1], app);
    let bottom = Layout::horizontal([Constraint::Percentage(62), Constraint::Percentage(38)])
        .spacing(1)
        .split(rows[1]);
    draw_lighting_modes(frame, bottom[0], app);
    draw_lighting_notes(frame, bottom[1], app);
}

fn draw_lighting_status(frame: &mut Frame, area: Rect, app: &App) {
    let block = panel(" 当前灯效 ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines = Vec::new();
    if let Some(aura) = &app.aura {
        lines.push(Line::from(vec![
            Span::styled("  模式  ", Style::new().fg(pal().dim)),
            Span::styled(
                format!("{}（{}）", aura.mode_name, aura.mode_zh),
                Style::new().fg(pal().accent).bold(),
            ),
        ]));
        lines.push(Line::from(vec![
            Span::styled("  亮度  ", Style::new().fg(pal().dim)),
            Span::styled(aura.brightness_name.clone(), Style::new().fg(pal().good)),
        ]));
        lines.push(Line::from(vec![
            Span::styled("  颜色  ", Style::new().fg(pal().dim)),
            Span::styled(format!("{} ", aura.color1), Style::new().fg(pal().text)),
            Span::styled(
                "████",
                Style::new().fg(hex_color(aura.color1.trim_start_matches('#'))),
            ),
        ]));
        if aura.color2 != "#000000" {
            lines.push(Line::from(vec![
                Span::styled("  副色  ", Style::new().fg(pal().dim)),
                Span::styled(format!("{} ", aura.color2), Style::new().fg(pal().text)),
                Span::styled(
                    "████",
                    Style::new().fg(hex_color(aura.color2.trim_start_matches('#'))),
                ),
            ]));
        }
        if !aura.speed.is_empty() {
            lines.push(Line::from(vec![
                Span::styled("  速度  ", Style::new().fg(pal().dim)),
                Span::styled(aura.speed.clone(), Style::new().fg(pal().text)),
            ]));
        }
        if !aura.zone.is_empty() {
            lines.push(Line::from(vec![
                Span::styled("  区域  ", Style::new().fg(pal().dim)),
                Span::styled(aura.zone.clone(), Style::new().fg(pal().text)),
            ]));
        }
    } else {
        lines.push(Line::from(vec![
            Span::styled("  当前亮度  ", Style::new().fg(pal().dim)),
            Span::styled(app.led.clone(), Style::new().fg(pal().accent).bold()),
        ]));
        lines.push(Line::from(Span::styled(
            "  无法读取 Aura 状态，键盘可能只支持亮度。",
            Style::new().fg(pal().warn),
        )));
    }

    if let Some(aura) = &app.aura {
        lines.push(Line::raw(""));
        lines.push(Line::from(Span::styled(
            "  颜色预览",
            Style::new().fg(pal().dim),
        )));
        lines.push(gradient_line(
            inner.width.saturating_sub(2) as usize,
            &aura.color1,
            if aura.color2 == "#000000" {
                "#1e242d"
            } else {
                &aura.color2
            },
        ));
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        "  b 亮度   e/E 灯效   c 选色   S 应用静态色",
        Style::new().fg(pal().dim),
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
                Style::new().bg(pal().surface).fg(pal().accent)
            } else if hovered {
                Style::new().bg(pal().surface_hover).fg(pal().dim)
            } else {
                Style::new().fg(pal().dim)
            },
        ));
        row.push(Span::styled("██ ", Style::new().fg(hex_color(hex))));
        row.push(Span::styled(
            format!("{:<4}", name),
            if selected {
                Style::new().fg(pal().accent).bold()
            } else {
                Style::new().fg(pal().text)
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
            Span::styled("  当前颜色  ", Style::new().fg(pal().dim)),
            Span::styled(format!("{} ", aura.color1), Style::new().fg(pal().text)),
            Span::styled(
                "████████",
                Style::new().fg(hex_color(aura.color1.trim_start_matches('#'))),
            ),
        ]));
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        "  c 切换选中颜色   S 应用为静态色",
        Style::new().fg(pal().dim),
    )));
    lines.push(Line::from(Span::styled(
        "  静态色只在 Static 灯效下生效",
        Style::new().fg(pal().dim),
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
    for chunk in supported.chunks(2) {
        let mut spans = Vec::new();
        for mode in chunk {
            let active = current == Some(*mode);
            spans.push(Span::styled(
                format!(" {} ", if active { "▶" } else { " " }),
                Style::new().fg(if active { pal().accent } else { pal().dim }),
            ));
            spans.push(Span::styled(
                format!("{:<13}", asus::aura_mode_name(*mode)),
                if active {
                    Style::new().fg(pal().accent).bold()
                } else {
                    Style::new().fg(pal().text)
                },
            ));
            spans.push(Span::styled(
                format!("{:<7}", asus::aura_mode_zh(*mode)),
                if active {
                    Style::new().fg(pal().accent)
                } else {
                    Style::new().fg(pal().dim)
                },
            ));
        }
        lines.push(Line::from(spans));
    }

    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        "  e 下一个   E 上一个",
        Style::new().fg(pal().dim),
    )));
    lines.push(Line::from(Span::styled(
        "  灯效由键盘固件决定，部分模式不支持颜色/速度调节",
        Style::new().fg(pal().dim),
    )));

    frame.render_widget(
        Paragraph::new(Text::from(lines)).wrap(Wrap { trim: false }),
        inner,
    );
}

fn draw_lighting_notes(frame: &mut Frame, area: Rect, app: &App) {
    let block = panel(" 灯效说明 ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines = vec![
        section("当前模式"),
        Line::from(vec![
            Span::styled("   ", Style::new()),
            Span::styled(
                app.aura
                    .as_ref()
                    .map(|a| format!("{}（{}）", a.mode_name, a.mode_zh))
                    .unwrap_or_else(|| "不可用".into()),
                Style::new().fg(pal().accent),
            ),
        ]),
        Line::raw(""),
        section("快捷键"),
        Line::from(Span::styled(
            "  e / E   下一个 / 上一个",
            Style::new().fg(pal().text),
        )),
        Line::from(Span::styled(
            "  c       切换静态颜色",
            Style::new().fg(pal().text),
        )),
        Line::from(Span::styled(
            "  S       应用静态色",
            Style::new().fg(pal().text),
        )),
        Line::raw(""),
        Line::from(Span::styled(
            "  静态色只在 Static 下生效",
            Style::new().fg(pal().dim),
        )),
        Line::from(Span::styled(
            "  部分灯效由固件控制，颜色/速度",
            Style::new().fg(pal().dim),
        )),
        Line::from(Span::styled("  可能被忽略", Style::new().fg(pal().dim))),
    ];
    if let Some(aura) = &app.aura {
        lines.push(Line::raw(""));
        lines.push(Line::from(vec![
            Span::styled("  当前颜色  ", Style::new().fg(pal().dim)),
            Span::styled(format!("{} ", aura.color1), Style::new().fg(pal().text)),
            Span::styled(
                "████",
                Style::new().fg(hex_color(aura.color1.trim_start_matches('#'))),
            ),
        ]));
    }

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
    let totals = app
        .snap
        .net_ifaces
        .iter()
        .filter(|iface| iface.physical)
        .fold((0u64, 0u64), |(rx, tx), iface| {
            (rx + iface.rx_total, tx + iface.tx_total)
        });
    let mut ifaces: Vec<Span> = app
        .snap
        .net_ifaces
        .iter()
        .filter(|iface| iface.physical)
        .take(2)
        .flat_map(|iface| {
            vec![
                Span::styled(
                    format!(
                        "  {} {}  ",
                        iface.name,
                        iface
                            .speed_mbps
                            .map(link_label)
                            .unwrap_or_else(|| iface.operstate.clone())
                    ),
                    Style::new().fg(pal().text),
                ),
                Span::styled(
                    format!(
                        "↓ {}  ↑ {}",
                        util::human_rate_with(iface.rx_bps, rate_style(app)),
                        util::human_rate_with(iface.tx_bps, rate_style(app))
                    ),
                    Style::new().fg(pal().dim),
                ),
            ]
        })
        .collect();
    ifaces.push(Span::styled(
        format!(
            "  累计 ↓ {}  ↑ {}",
            util::human_bytes(totals.0 as f64),
            util::human_bytes(totals.1 as f64)
        ),
        Style::new().fg(pal().dim),
    ));
    let header = Line::from(vec![
        Span::styled(" Wi-Fi ", Style::new().fg(pal().dim)),
        Span::styled(
            match app.wifi_enabled {
                Some(true) => "已启用",
                Some(false) => "已禁用",
                None => "未知",
            },
            Style::new().fg(if app.wifi_enabled == Some(true) {
                pal().good
            } else {
                pal().warn
            }),
        ),
        Span::styled("   SSID ", Style::new().fg(pal().dim)),
        Span::styled(
            app.ssid.clone().unwrap_or_else(|| "-".into()),
            Style::new().fg(pal().text),
        ),
        if nm::hotspot_active() {
            Span::styled("   热点已开启", Style::new().fg(pal().warn))
        } else {
            Span::raw("")
        },
        if app.net_loading {
            Span::styled("   刷新中…", Style::new().fg(pal().warn))
        } else {
            Span::raw("")
        },
    ]);
    let iface_line = Line::from(ifaces);
    frame.render_widget(
        Paragraph::new(Text::from(vec![header, iface_line])).style(Style::new()),
        rows[0],
    );

    let cols = Layout::horizontal([Constraint::Percentage(56), Constraint::Percentage(44)])
        .spacing(1)
        .split(rows[1]);
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
                    Style::new().fg(Color::Black).bg(pal().accent).bold()
                } else {
                    Style::new().fg(pal().dim)
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
                    Span::styled(" [已连接]", Style::new().fg(pal().good))
                } else {
                    Span::raw("")
                };
                lines.push(Line::from(vec![
                    Span::styled(
                        format!(" {marker} {}", util::truncate(&conn.name, 20)),
                        if selected {
                            Style::new().bg(pal().surface).fg(pal().accent).bold()
                        } else if hovered {
                            Style::new().bg(pal().surface_hover).fg(pal().text)
                        } else {
                            Style::new().fg(pal().text)
                        },
                    ),
                    Span::styled(
                        format!("  {}", util::truncate(short_kind(&conn.kind), 14)),
                        Style::new().fg(pal().dim),
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
                    Span::styled(" [已连接]", Style::new().fg(pal().good))
                } else {
                    Span::raw("")
                };
                lines.push(Line::from(vec![
                    Span::styled(
                        format!(" {marker} {}", util::truncate(&ap.ssid, 24)),
                        if selected {
                            Style::new().bg(pal().surface).fg(pal().accent).bold()
                        } else if hovered {
                            Style::new().bg(pal().surface_hover).fg(pal().text)
                        } else {
                            Style::new().fg(pal().text)
                        },
                    ),
                    Span::styled(
                        format!("  {bars} {:>3}%", ap.signal),
                        Style::new().fg(signal_color(ap.signal)),
                    ),
                    Span::styled(
                        format!("  {}", util::truncate(&ap.security, 12)),
                        Style::new().fg(pal().dim),
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
                    Span::styled(" [已连接]", Style::new().fg(pal().good))
                } else {
                    Span::styled(" [未连接]", Style::new().fg(pal().dim))
                };
                lines.push(Line::from(vec![
                    Span::styled(
                        format!(" {marker} {}", util::truncate(&vpn.name, 26)),
                        if selected {
                            Style::new().bg(pal().surface).fg(pal().accent).bold()
                        } else if hovered {
                            Style::new().bg(pal().surface_hover).fg(pal().text)
                        } else {
                            Style::new().fg(pal().text)
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
            Style::new().fg(pal().dim),
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
                if let Some(dev) = &app.net_detail
                    && (!dev.ip6_addresses.is_empty()
                        || !dev.ip6_gateway.is_empty()
                        || !dev.ip6_dns.is_empty())
                {
                    lines.push(Line::raw(""));
                    lines.push(section("IPv6"));
                    if !dev.ip6_addresses.is_empty() {
                        lines.push(detail("IPv6", util::truncate(&dev.ip6_addresses, 60)));
                    }
                    if !dev.ip6_gateway.is_empty() {
                        lines.push(detail("网关", dev.ip6_gateway.clone()));
                    }
                    if !dev.ip6_dns.is_empty() {
                        lines.push(detail("DNS", util::truncate(&dev.ip6_dns, 60)));
                    }
                }
                lines.push(Line::raw(""));
                lines.push(section("操作"));
                lines.push(Line::from(Span::styled(
                    " Enter 编辑 IPv4 / DNS",
                    Style::new().fg(pal().dim),
                )));
                lines.push(Line::from(Span::styled(
                    " u 激活   d 断开   x 删除",
                    Style::new().fg(pal().dim),
                )));
                lines.push(Line::from(Span::styled(
                    " P 修改 Wi-Fi 密码",
                    Style::new().fg(pal().dim),
                )));
                lines.push(Line::from(Span::styled(
                    " p Ping   n DNS 查询   c 缓存",
                    Style::new().fg(pal().dim),
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
                    Style::new().fg(pal().dim),
                )));
                lines.push(Line::from(Span::styled(
                    " s 重新扫描   p Ping   n DNS 查询   c 缓存",
                    Style::new().fg(pal().dim),
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
                    Style::new().fg(pal().dim),
                )));
            } else {
                lines.push(Line::from(Span::styled(
                    " 没有 VPN 连接",
                    Style::new().fg(pal().dim),
                )));
            }
        }
    }

    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        " v 视图   o 无线   t 热点",
        Style::new().fg(pal().dim),
    )));

    let bar_y = inner.y + lines.len() as u16;
    if let Some(line) = action_bar(
        app,
        inner,
        bar_y,
        &[
            ("Ping", MouseAction::NetPing),
            ("DNS", MouseAction::NetDns),
            ("缓存", MouseAction::NetCache),
            ("刷新", MouseAction::NetRefresh),
        ],
    ) {
        lines.push(line);
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
        .title(Line::from(vec![
            Span::styled("▍", Style::new().fg(pal().accent)),
            Span::styled(
                format!(" 编辑 IPv4 · {} ", form.conn),
                Style::new().fg(pal().text).bold(),
            ),
        ]))
        .border_style(Style::new().fg(pal().accent));
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
            Style::new().fg(pal().text)
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
                style.fg(if selected { pal().accent } else { pal().text }),
            ),
        ]));
        if form.method == "auto" && (1..=3).contains(&index) {
            lines.push(Line::from(Span::styled(
                "              切换到手动后可编辑",
                Style::new().fg(pal().dim),
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
                Style::new().fg(pal().dim),
            )),
            Line::from(Span::styled(
                " a 应用   Esc 取消   Ctrl+U 清空当前项",
                Style::new().fg(pal().dim),
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
        record_mouse(app, Rect::new(x, y, width + 3, 1), *action);
        let style = if app.hover == Some(*action) {
            Style::new().fg(Color::Black).bg(pal().accent).bold()
        } else {
            Style::new().fg(pal().accent).bg(pal().surface)
        };
        spans.push(Span::styled(format!(" {label} "), style));
        spans.push(Span::raw(" "));
        x += width + 3;
    }
    Some(Line::from(spans))
}

fn row_style(selected: bool, hovered: bool) -> Style {
    if selected {
        Style::new().bg(pal().selected).fg(Color::White).bold()
    } else if hovered {
        Style::new().bg(pal().surface_hover).fg(pal().text)
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
    Line::from(vec![
        Span::styled("  ▍ ", Style::new().fg(pal().accent)),
        Span::styled(title, Style::new().fg(Color::White).bold()),
    ])
}

fn detail(label: &'static str, value: String) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("  {label:<6}"), Style::new().fg(pal().dim)),
        Span::styled(util::truncate(&value, 34), Style::new().fg(pal().text)),
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
        pal().good
    } else if signal >= 40 {
        pal().warn
    } else {
        pal().bad
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
            "active" => pal().good,
            "failed" => pal().bad,
            "activating" | "reloading" => pal().warn,
            _ => pal().dim,
        };
        let enabled_color = match info.enabled.as_str() {
            "enabled" => pal().good,
            "static" => pal().warn,
            "masked" => pal().bad,
            _ => pal().dim,
        };
        lines.push(Line::from(vec![
            Span::styled(
                format!(" {marker} {:<24}", util::truncate(info.display_name(), 24)),
                if selected {
                    Style::new().bg(pal().surface).fg(pal().accent).bold()
                } else if hovered {
                    Style::new().bg(pal().surface_hover).fg(pal().text)
                } else {
                    Style::new().fg(pal().text)
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
        Style::new().fg(pal().dim),
    )));
    lines.push(Line::from(Span::styled(
        " e 启用自启   d 禁用自启   E 启用并启动   D 禁用并停止",
        Style::new().fg(pal().dim),
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
            Style::new().fg(pal().dim),
        )));
        details.push(Line::from(Span::styled(
            " e 启用自启   d 禁用自启",
            Style::new().fg(pal().dim),
        )));
        details.push(Line::from(Span::styled(
            " E 启用并立即启动   D 禁用并停止",
            Style::new().fg(pal().dim),
        )));
        details.push(Line::raw(""));
        details.push(Line::from(Span::styled(
            " 停止或重启 NetworkManager 会断网；",
            Style::new().fg(pal().warn),
        )));
        details.push(Line::from(Span::styled(
            " 停止 asusd 会失去 ASUS 硬件控制。",
            Style::new().fg(pal().warn),
        )));
        if info.load == "not-found" {
            details.push(Line::raw(""));
            details.push(Line::from(Span::styled(
                " 该服务不存在",
                Style::new().fg(pal().bad),
            )));
        }
    } else {
        details.push(Line::from(Span::styled(
            " 没有服务",
            Style::new().fg(pal().dim),
        )));
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
        .title(Line::from(vec![
            Span::styled("▍", Style::new().fg(pal().accent)),
            Span::styled(" 网络缓存 ", Style::new().fg(pal().text).bold()),
        ]))
        .border_style(Style::new().fg(pal().accent));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let mut lines = Vec::new();
    if let Some(stats) = &app.cache_stats {
        lines.push(Line::from(vec![
            Span::styled("  ▍ ", Style::new().fg(pal().accent)),
            Span::styled("DNS 缓存", Style::new().fg(pal().text).bold()),
            Span::styled("（systemd-resolved）", Style::new().fg(pal().dim)),
        ]));
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
            Style::new().fg(pal().warn),
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
        Style::new().fg(pal().dim),
    )));
    record_mouse(
        app,
        Rect::new(inner.x, y + 1, inner.width, 1),
        MouseAction::CacheFlushNeighbors,
    );
    lines.push(Line::from(Span::styled(
        " [F] 清空邻居缓存",
        Style::new().fg(pal().dim),
    )));
    record_mouse(
        app,
        Rect::new(inner.x, y + 2, inner.width, 1),
        MouseAction::CacheEditSize,
    );
    lines.push(Line::from(Span::styled(
        " [e] 修改 DNS 缓存上限（重启 resolved）",
        Style::new().fg(pal().dim),
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
        Span::styled(" [r] 刷新统计", Style::new().fg(pal().dim)),
        Span::styled("   [Esc] 关闭", Style::new().fg(pal().dim)),
    ]));
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        " 修改缓存上限会写入 /etc/systemd/resolved.conf.d/rsztui-cache.conf",
        Style::new().fg(pal().dim),
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
            Span::styled("  配置文件  ", Style::new().fg(pal().dim)),
            Span::styled(
                crate::config::Config::path().display().to_string(),
                Style::new().fg(pal().text),
            ),
        ]),
        Line::from(vec![
            Span::styled("  asusd 配置 ", Style::new().fg(pal().dim)),
            Span::styled("/etc/asusd/asusd.ron", Style::new().fg(pal().text)),
        ]),
        Line::from(vec![
            Span::styled("  版本      ", Style::new().fg(pal().dim)),
            Span::styled(
                format!("RszTui {}", env!("CARGO_PKG_VERSION")),
                Style::new().fg(pal().text),
            ),
        ]),
        Line::from(vec![
            Span::styled("  持久化    ", Style::new().fg(pal().dim)),
            Span::styled(
                if app.tuning {
                    "asusd 接管，开启"
                } else {
                    "关闭"
                },
                Style::new().fg(if app.tuning { pal().good } else { pal().warn }),
            ),
        ]),
        Line::raw(""),
        section("硬件与系统"),
        Line::from(vec![
            Span::styled("  CPU       ", Style::new().fg(pal().dim)),
            Span::styled(
                util::truncate(&app.hw.cpu_model, 58),
                Style::new().fg(pal().text),
            ),
        ]),
        Line::from(vec![
            Span::styled("  核心      ", Style::new().fg(pal().dim)),
            Span::styled(
                format!("{} 核 / {} 线程", app.hw.cores, app.hw.threads),
                Style::new().fg(pal().text),
            ),
        ]),
        Line::from(vec![
            Span::styled("  显卡      ", Style::new().fg(pal().dim)),
            Span::styled(
                util::truncate(&app.gpu_names().join(" / "), 58),
                Style::new().fg(pal().text),
            ),
        ]),
        Line::from(vec![
            Span::styled("  内存      ", Style::new().fg(pal().dim)),
            Span::styled(
                util::human_bytes(app.hw.memory_total as f64),
                Style::new().fg(pal().text),
            ),
        ]),
        Line::from(vec![
            Span::styled("  内核      ", Style::new().fg(pal().dim)),
            Span::styled(app.hw.kernel.clone(), Style::new().fg(pal().text)),
        ]),
        Line::from(vec![
            Span::styled("  BIOS      ", Style::new().fg(pal().dim)),
            Span::styled(app.hw.bios.clone(), Style::new().fg(pal().text)),
        ]),
        Line::raw(""),
        section("依赖检查"),
    ];

    let mut appearance = vec![section("外观")];
    appearance.push(setting_choices(
        app,
        inner,
        inner.y + 6,
        "主题",
        &crate::config::THEMES,
        &app.cfg.ui.theme,
        MouseAction::SetTheme,
    ));
    let rate_key =
        crate::config::RATE_STYLES[crate::config::rate_style_index(&app.cfg.ui.rate_style)].0;
    appearance.push(setting_choices(
        app,
        inner,
        inner.y + 7,
        "速率",
        &crate::config::RATE_STYLES,
        rate_key,
        MouseAction::SetRateStyle,
    ));
    appearance.push(Line::from(Span::styled(
        "  t 主题  y 速率   二进制=KiB/s / 十进制=KB/s / 比特=Mbit/s",
        Style::new().fg(pal().dim),
    )));
    appearance.push(Line::raw(""));
    lines.splice(5..5, appearance);

    for check in &app.deps.checks {
        lines.push(Line::from(vec![
            Span::styled(
                format!(
                    "  {} {:<14}",
                    if check.ok { "✅" } else { "❌" },
                    check.name
                ),
                Style::new().fg(if check.ok { pal().good } else { pal().bad }),
            ),
            Span::styled(
                util::truncate(&check.detail, 52),
                Style::new().fg(pal().text),
            ),
        ]));
    }
    if app.deps.has_warnings() {
        lines.push(Line::from(Span::styled(
            "  运行 rsztui doctor 查看安装/启动命令",
            Style::new().fg(pal().warn),
        )));
    }

    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        "  按 ? 查看完整快捷键，持久化说明见 docs/CONFIG.md",
        Style::new().fg(pal().dim),
    )));

    frame.render_widget(Paragraph::new(Text::from(lines)), inner);
}

fn setting_choices(
    app: &App,
    inner: Rect,
    y: u16,
    label: &str,
    items: &[(&str, &str)],
    selected: &str,
    action: fn(usize) -> MouseAction,
) -> Line<'static> {
    let prefix = format!("  {label}    ");
    let mut x = inner.x + UnicodeWidthStr::width(prefix.as_str()) as u16;
    let mut spans = vec![Span::styled(prefix, Style::new().fg(pal().dim))];
    for (index, (key, text)) in items.iter().enumerate() {
        let active = *key == selected;
        let style = if active {
            Style::new().fg(Color::Black).bg(pal().accent).bold()
        } else {
            Style::new().fg(pal().accent)
        };
        let chip = format!(" {text} ");
        let width = UnicodeWidthStr::width(chip.as_str()) as u16;
        if y < inner.bottom() {
            record_mouse(app, Rect::new(x, y, width, 1), action(index));
        }
        spans.push(Span::styled(chip, style));
        spans.push(Span::raw(" "));
        x += width + 1;
    }
    Line::from(spans)
}

fn draw_footer(frame: &mut Frame, area: Rect, app: &App) {
    let rows = Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).split(area);

    let status_style = if app.status_err {
        Style::new().fg(pal().bad)
    } else {
        Style::new().fg(pal().good)
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" ▌ ", Style::new().fg(pal().accent)),
            Span::styled(
                util::truncate(&app.status, (area.width as usize).saturating_sub(3)),
                status_style,
            ),
        ]))
        .style(Style::new().bg(pal().surface)),
        rows[0],
    );

    let hint = match app.tab {
        Tab::Dashboard => " Tab 切换  r 刷新  ? 帮助  q 退出",
        Tab::Performance => match app.perf_focus {
            crate::app::PerfFocus::Profiles => " j/k 选择  Enter 切换  a/b 默认  f 焦点  g/G GPU",
            crate::app::PerfFocus::Presets => " j/k 预设  Enter/p 套用  f 焦点",
            crate::app::PerfFocus::Power => " j/k 字段  h/l 调整  Enter 应用  f 焦点",
            crate::app::PerfFocus::Gpu => " j/k 选择 GPU  Enter/G 应用  f 焦点",
        },
        Tab::Battery => " h/l 上限  Enter 应用  o 单次充满  ? 帮助",
        Tab::Fan => " i/I 编辑曲线  j/k 预设  p 应用  s 保存  e 启用  ? 帮助",
        Tab::Lighting => " b 亮度  e/E 灯效  c 颜色  S 静态色  ? 帮助",
        Tab::Network => " v 视图  j/k 选择  Enter 编辑  u/d 启停  p Ping  n DNS  c 缓存",
        Tab::Gpu => " 只读监控  r 刷新  ? 帮助  q 退出",
        Tab::Services => " j/k 选择  s 启动  t 停止  R 重启  e/d 自启  E/D 立即  ? 帮助",
        _ => " Tab 切换  1-9 跳转  r 刷新  ? 帮助  q 退出",
    };
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            hint,
            Style::new().fg(pal().dim).bg(Color::Rgb(28, 32, 38)),
        ))),
        rows[1],
    );
}

fn wrapped_line_count(text: &str, width: usize) -> usize {
    if width == 0 {
        return text.lines().count().max(1);
    }
    let mut lines = 0;
    for line in text.split('\n') {
        let mut used = 0;
        let mut count = 1;
        for ch in line.chars() {
            let w = ch.width().unwrap_or(0);
            if used + w > width && used > 0 {
                count += 1;
                used = 0;
            }
            used += w;
        }
        lines += count;
    }
    lines.max(1)
}

fn draw_modal(frame: &mut Frame, area: Rect, app: &App, modal: &Modal) {
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
        Modal::Hotspot(form) => {
            let mark = |focus: usize| if form.focus == focus { "▸" } else { " " };
            let password = "●".repeat(form.password.chars().count());
            (
                "开启热点".into(),
                format!(
                    "{} SSID     {}\n\n{} 密码     {}",
                    mark(0),
                    form.ssid,
                    mark(1),
                    password
                ),
                true,
            )
        }
    };
    let buttons: Vec<(&str, MouseAction)> = match modal {
        Modal::Message { .. } => vec![("关闭", MouseAction::ModalConfirm)],
        Modal::Input { .. } | Modal::Confirm { .. } | Modal::Hotspot(_) => vec![
            ("确认", MouseAction::ModalConfirm),
            ("取消", MouseAction::ModalCancel),
        ],
    };
    let hint = match (confirm, modal) {
        (true, Modal::Hotspot(_)) => "   Tab 切换字段 · Enter 开启 / Esc 取消",
        (true, _) => "   Enter 确认 / Esc 取消",
        (false, _) => "   j/k 滚动 · Enter 关闭",
    };

    let button_width = buttons
        .iter()
        .map(|(label, _)| UnicodeWidthStr::width(*label) + 3)
        .sum::<usize>()
        + UnicodeWidthStr::width(hint);
    let title_width = UnicodeWidthStr::width(title.as_str()) + 2;
    let body_width = body.lines().map(UnicodeWidthStr::width).max().unwrap_or(0);
    let desired_width = body_width.max(button_width).max(title_width).max(32) + 4;
    let max_width = area.width.saturating_sub(4).max(1) as usize;
    let width = desired_width
        .min(max_width)
        .max(10)
        .min(area.width.max(1) as usize) as u16;
    let inner_width = width.saturating_sub(2).max(1) as usize;
    let body_lines = wrapped_line_count(&body, inner_width) as u16;
    let max_height = area.height.saturating_sub(2).max(1);
    let height = (body_lines + 4)
        .max(5)
        .min(max_height)
        .min(area.height.max(1));
    let popup = Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    );

    frame.render_widget(Clear, popup);

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .title(Line::from(vec![
            Span::styled("▍", Style::new().fg(pal().accent)),
            Span::styled(format!(" {title} "), Style::new().fg(pal().text).bold()),
        ]))
        .border_style(Style::new().fg(pal().accent));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let parts = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).split(inner);
    let mut paragraph = Paragraph::new(body.clone()).wrap(Wrap { trim: false });
    if matches!(modal, Modal::Message { .. }) {
        let total = wrapped_line_count(&body, inner_width) as u16;
        let max_scroll = total.saturating_sub(parts[0].height);
        let scroll = app.modal_scroll.get().min(max_scroll);
        app.modal_scroll.set(scroll);
        paragraph = paragraph.scroll((scroll, 0));
        if max_scroll > 0 {
            let mut state = ratatui::widgets::ScrollbarState::new(max_scroll as usize)
                .position(scroll as usize);
            frame.render_stateful_widget(
                ratatui::widgets::Scrollbar::new(
                    ratatui::widgets::ScrollbarOrientation::VerticalRight,
                )
                .thumb_style(Style::new().fg(pal().accent))
                .track_style(Style::new().fg(pal().faint)),
                parts[0],
                &mut state,
            );
        }
    }
    frame.render_widget(paragraph, parts[0]);

    let mut spans = Vec::new();
    let mut x = parts[1].x;
    for (label, action) in &buttons {
        let width = UnicodeWidthStr::width(*label) as u16;
        record_mouse(app, Rect::new(x, parts[1].y, width + 3, 1), *action);
        let style = if app.hover == Some(*action) {
            Style::new().fg(Color::Black).bg(pal().accent).bold()
        } else {
            Style::new().fg(pal().accent).bg(pal().surface).bold()
        };
        spans.push(Span::styled(format!(" {label} "), style));
        spans.push(Span::raw(" "));
        x += width + 3;
    }
    spans.push(Span::styled(hint, Style::new().fg(pal().dim)));
    frame.render_widget(Paragraph::new(Line::from(spans)), parts[1]);
}
fn panel(title: impl Into<Line<'static>>) -> Block<'static> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .title(panel_title(title.into()))
        .title_style(Style::new().fg(pal().text).bold())
        .border_style(Style::new().fg(pal().border))
}

fn panel_focused(title: impl Into<Line<'static>>, focused: bool) -> Block<'static> {
    let border = if focused { pal().accent } else { pal().border };
    Block::bordered()
        .border_type(BorderType::Rounded)
        .title(panel_title(title.into()))
        .title_style(Style::new().fg(pal().text).bold())
        .border_style(Style::new().fg(border))
}

fn panel_title(mut title: Line<'static>) -> Line<'static> {
    title
        .spans
        .insert(0, Span::styled("▍", Style::new().fg(pal().accent)));
    title
}

fn profile_color(profile: Profile) -> Color {
    match profile {
        Profile::Quiet => Color::Rgb(110, 170, 255),
        Profile::Balanced => pal().good,
        Profile::Performance => pal().bad,
    }
}

fn battery_color(app: &App) -> Color {
    match app.battery.capacity {
        Some(v) if v <= 15 => pal().bad,
        Some(v) if v <= 35 => pal().warn,
        _ => pal().good,
    }
}

fn mix_color(a: Color, b: Color, t: f32) -> Color {
    let (Color::Rgb(ar, ag, ab), Color::Rgb(br, bg, bb)) = (a, b) else {
        return a;
    };
    let mix = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t) as u8;
    Color::Rgb(mix(ar, br), mix(ag, bg), mix(ab, bb))
}

fn load_color(value: f32) -> Color {
    if value >= 85.0 {
        pal().bad
    } else if value >= 55.0 {
        pal().warn
    } else {
        pal().good
    }
}

fn temp_color(value: f32) -> Color {
    if value >= 90.0 {
        pal().bad
    } else if value >= 75.0 {
        pal().warn
    } else {
        pal().good
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    #[test]
    fn labels_link_speed() {
        assert_eq!(super::link_label(1000), "1.0 Gb/s");
        assert_eq!(super::link_label(2500), "2.5 Gb/s");
        assert_eq!(super::link_label(100), "100 Mb/s");
    }

    fn status(vendor: gpu::Vendor, name: &str) -> gpu::Status {
        gpu::Status {
            gpu: gpu::Gpu {
                vendor,
                name: name.to_string(),
                short: name.to_string(),
                driver: "nvidia".into(),
                pci: "0000:01:00.0".into(),
                card: "card0".into(),
                hwmon: None,
            },
            stats: gpu::Stats {
                temp: Some(38.0),
                hotspot: Some(42.5),
                load: Some(12),
                mem_used: Some(2_097_152),
                mem_total: Some(8_589_934_592),
                power: Some(2.19),
                clock_mhz: Some(210),
                pcie: "PCIe 4.0 x8".into(),
                driver_version: "615.78.08".into(),
                runtime: "active".into(),
                ..Default::default()
            },
        }
    }

    fn dump(gpus: &[gpu::Status], width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| draw_gpu(frame, frame.area(), gpus, false))
            .unwrap();
        let mut text = String::new();
        let mut skip_continuation = false;
        for cell in terminal.backend().buffer().content() {
            if skip_continuation {
                skip_continuation = false;
                continue;
            }
            let symbol = cell.symbol();
            text.push_str(symbol);
            skip_continuation = UnicodeWidthStr::width(symbol) > 1;
        }
        text
    }

    #[test]
    fn renders_gpu_cards_at_common_sizes() {
        let gpus = [
            status(gpu::Vendor::Nvidia, "RTX 4060"),
            status(gpu::Vendor::Amd, "Radeon 780M"),
        ];
        for (width, height) in [(120, 30), (90, 24), (60, 16), (40, 10), (30, 8)] {
            dump(&gpus, width, height);
        }
        let text = dump(&gpus, 120, 30);
        assert!(text.contains("RTX 4060"));
        assert!(text.contains("Radeon 780M"));
        assert!(text.contains("615.78.08"));
        assert!(text.contains("PCIe 4.0 x8"));
        assert!(text.contains("38.0°C"));
    }

    #[test]
    fn explains_missing_gpus() {
        let text = dump(&[], 60, 12);
        assert!(text.contains("没有检测到 GPU"), "dump={text:?}");
    }
}
