//! TUI 渲染。

use crate::app::{App, Screen, byte_idx};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use unicode_width::UnicodeWidthChar;
use unicode_width::UnicodeWidthStr;

const CYAN: Style = Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD);
const GREEN: Style = Style::new().fg(Color::Green);
const RED: Style = Style::new().fg(Color::Red);
const YELLOW: Style = Style::new().fg(Color::Yellow);
const DIM: Style = Style::new().fg(Color::DarkGray);

pub fn draw(f: &mut Frame, app: &App) {
    let area = f.area();
    let v = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(area);

    draw_top_bar(f, app, v[0]);
    draw_main(f, app, v[1]);
    draw_status_bar(f, app, v[2]);

    if app.screen == Screen::Settings {
        draw_settings_overlay(f, app);
    }
}

fn draw_top_bar(f: &mut Frame, app: &App, area: Rect) {
    let h = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(0), Constraint::Length(18)])
        .split(area);
    let title = Span::styled("计算器 Calculator", CYAN);
    f.render_widget(Paragraph::new(Line::from(title)), h[0]);
    let mode = if app.eval.degree {
        "[DEG 角度]"
    } else {
        "[RAD 弧度]"
    };
    let badge = Span::styled(mode, YELLOW.add_modifier(Modifier::BOLD));
    f.render_widget(
        Paragraph::new(Line::from(badge)).alignment(ratatui::layout::Alignment::Right),
        h[1],
    );
}

fn draw_main(f: &mut Frame, app: &App, area: Rect) {
    let h = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(62), Constraint::Percentage(38)])
        .split(area);
    draw_left(f, app, h[0]);
    draw_help(f, app, h[1]);
}

fn draw_left(f: &mut Frame, app: &App, area: Rect) {
    let v = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(0),
        ])
        .split(area);
    draw_expr(f, app, v[0]);
    draw_result(f, app, v[1]);
    draw_history(f, app, v[2]);
}

fn draw_expr(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title("表达式")
        .title_style(CYAN);
    let inner = block.inner(area);
    let text = if app.input.is_empty() {
        Line::from(Span::styled(
            "在此输入表达式… (可用 \\p \\s \\i 等输入符号)",
            DIM,
        ))
    } else {
        Line::from(app.input.as_str())
    };
    f.render_widget(
        Paragraph::new(text).block(block).wrap(Wrap { trim: false }),
        area,
    );

    // 光标定位
    if app.screen == Screen::Main {
        let prefix = &app.input[..byte_idx(&app.input, app.cursor)];
        let w = UnicodeWidthStr::width(prefix) as u16;
        let inner_w = inner.width;
        let scroll = w.saturating_sub(inner_w.saturating_sub(1));
        let x = inner.x + (w - scroll).min(inner_w.saturating_sub(1));
        f.set_cursor_position(Position::new(x, inner.y));
    }
}

fn draw_result(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title("结果")
        .title_style(CYAN);
    let line = match &app.result {
        Some(Ok(v)) => Line::from(vec![
            Span::styled("= ", GREEN.add_modifier(Modifier::BOLD)),
            Span::styled(crate::math::fmt_result(*v), GREEN),
        ]),
        Some(Err(e)) => Line::from(Span::styled(format!("✗ {e}"), RED)),
        None => Line::from(Span::styled("输入表达式后按 Enter 计算", DIM)),
    };
    f.render_widget(
        Paragraph::new(line).block(block).wrap(Wrap { trim: false }),
        area,
    );
}

fn draw_history(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title("历史")
        .title_style(CYAN);
    let max = (area.height.saturating_sub(2)) as usize;
    let start = app.history.len().saturating_sub(max);
    let mut lines: Vec<Line> = app.history[start..]
        .iter()
        .map(|e| {
            Line::from(vec![
                Span::styled(format!("{} = ", e.expr), Style::new().fg(Color::White)),
                Span::styled(&e.result, GREEN),
            ])
        })
        .collect();
    if lines.is_empty() {
        lines.push(Line::from(Span::styled("暂无记录 (↑/↓ 可回看)", DIM)));
    }
    f.render_widget(Paragraph::new(lines).block(block), area);
}

fn draw_help(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title("帮助 / 公式输入")
        .title_style(CYAN);
    let mut lines: Vec<Line> = Vec::new();

    let sec = |t: String| Line::from(Span::styled(t, CYAN));
    let row = |t: String| Line::from(t);
    let dim = |t: String| Line::from(Span::styled(t, DIM));

    lines.push(sec("函数".into()));
    lines.push(row("sin cos tan  三角函数".into()));
    lines.push(row("asin acos atan  反三角".into()));
    lines.push(row("sinh cosh tanh  双曲(弧度)".into()));
    lines.push(row("ln 自然对数  log 常用对数(10)".into()));
    lines.push(row("logb(底,x) 任意底  log2(x)".into()));
    lines.push(row("sqrt( 平方根  exp( e^x  x^y 幂".into()));
    lines.push(row("n! 阶乘  π 圆周率  e 自然底数".into()));
    lines.push(Line::from(""));
    lines.push(sec("高级运算 (参数顺序固定)".into()));
    lines.push(row("Σ(k,1,10,k^2)  求和 → 385".into()));
    lines.push(row("∫(x,0,1,x^2)  定积分".into()));
    lines.push(row("deriv(x^2,x,2)  求导 → 4".into()));
    lines.push(row("可嵌套: Σ(k,1,3,Σ(j,1,k,j))".into()));
    lines.push(Line::from(""));
    lines.push(sec("角度 / 弧度".into()));
    let mode_hint = if app.eval.degree {
        "当前 DEG: sin(30)=0.5".to_string()
    } else {
        "当前 RAD: sin(π/6)=0.5".to_string()
    };
    lines.push(row(mode_hint));
    lines.push(row("Tab 键切换  |  30° 表示 30 度".into()));
    lines.push(Line::from(""));
    lines.push(sec("组合按键 (输入即替换)".into()));
    for c in &app.combos {
        let ins = if c.insert.is_empty() {
            "(空)".to_string()
        } else {
            c.insert.clone()
        };
        lines.push(row(format!("[{}] → {}", c.keys, ins)));
    }
    lines.push(dim("F2 修改组合按键".into()));
    lines.push(Line::from(""));
    lines.push(sec("示例".into()));
    lines.push(row("2πr   sin(30°)   √(16)  5!".into()));
    lines.push(row("(2+3)!  2pi   Σ(k,1,5,k^3)".into()));

    f.render_widget(
        Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn draw_status_bar(f: &mut Frame, app: &App, area: Rect) {
    let (style, text) = match app.screen {
        Screen::Main => (
            Style::new().fg(Color::Black).bg(Color::Blue),
            " Enter 计算 | Tab 角度/弧度 | F2 组合按键 | ↑/↓ 历史 | Esc 清空/退出 ",
        ),
        Screen::Settings => (
            Style::new().fg(Color::Black).bg(Color::Magenta),
            " ↑/↓ 选择 | Enter 编辑 | Tab 切换字段 | a 添加 | d 删除 | r 恢复默认 | Esc 返回 ",
        ),
    };
    f.render_widget(Paragraph::new(Line::from(Span::styled(text, style))), area);
}

// ------------------------------------------------------------ 设置覆盖层

fn draw_settings_overlay(f: &mut Frame, app: &App) {
    let area = f.area();
    let w = (area.width.saturating_sub(8)).min(64);
    let h = ((app.combos.len() + 8) as u16)
        .min(area.height.saturating_sub(6))
        .max(6);
    let x = area.x + (area.width - w) / 2;
    let y = area.y + (area.height - h) / 2;
    let rect = Rect::new(x, y, w, h);

    f.render_widget(ratatui::widgets::Clear, rect);

    let mut lines: Vec<Line> = Vec::new();
    let keys_w = app
        .combos
        .iter()
        .map(|c| UnicodeWidthStr::width(c.keys.as_str()))
        .max()
        .unwrap_or(4)
        + 2;

    for (i, c) in app.combos.iter().enumerate() {
        let num = Span::raw(format!("{:>2} ", i + 1));
        let mut spans = vec![num];
        let editing = app.set.editing.as_ref().filter(|_| i == app.set.selected);
        if let Some(ed) = editing {
            let keys_style = if ed.focus_keys {
                Style::new()
                    .fg(Color::Black)
                    .bg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::new().fg(Color::Yellow)
            };
            let insert_style = if !ed.focus_keys {
                Style::new()
                    .fg(Color::Black)
                    .bg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::new().fg(Color::Yellow)
            };
            spans.push(Span::styled(ed.keys.as_str(), keys_style));
            spans.push(Span::raw(
                " ".repeat(keys_w - UnicodeWidthStr::width(ed.keys.as_str())),
            ));
            spans.push(Span::styled("→", Style::new().fg(Color::Gray)));
            spans.push(Span::raw(" "));
            spans.push(Span::styled(ed.insert.as_str(), insert_style));
        } else {
            let sel = i == app.set.selected;
            let base = if sel {
                Style::new().fg(Color::Black).bg(Color::Cyan)
            } else {
                Style::new()
            };
            spans.push(Span::styled(
                c.keys.as_str(),
                base.add_modifier(Modifier::BOLD),
            ));
            spans.push(Span::styled(
                " ".repeat(keys_w - UnicodeWidthStr::width(c.keys.as_str())),
                base,
            ));
            spans.push(Span::styled("→ ", base));
            spans.push(Span::styled(
                c.insert.as_str(),
                base.add_modifier(Modifier::BOLD),
            ));
        }
        lines.push(Line::from(spans));
    }

    lines.push(Line::from(""));
    let hint = if app.set.editing.is_some() {
        "编辑中: 输入字符修改 | Tab 切换字段 | Enter 保存 | Esc 取消"
    } else {
        "↑/↓ 选择 | Enter 编辑 | a 添加 | d 删除 | r 恢复默认 | Esc 返回"
    };
    lines.push(Line::from(Span::styled(hint, DIM)));
    if let Some(msg) = &app.set.msg {
        let style = if msg == "已保存" { GREEN } else { RED };
        lines.push(Line::from(Span::styled(msg, style)));
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::new().fg(Color::Magenta))
        .title("组合按键设置")
        .title_style(CYAN);
    let inner = block.inner(rect);
    f.render_widget(Paragraph::new(lines).block(block), rect);

    // 编辑时光标
    if let Some(ed) = app.set.editing.as_ref() {
        let row = inner.y + app.set.selected as u16;
        let prefix = format!("{:>2} ", app.set.selected + 1);
        let mut x = inner.x + UnicodeWidthStr::width(prefix.as_str()) as u16;
        if ed.focus_keys {
            x += UnicodeWidthStr::width(ed.keys.as_str()) as u16;
        } else {
            x += (keys_w + 2) as u16 + UnicodeWidthStr::width(ed.insert.as_str()) as u16;
        }
        f.set_cursor_position(Position::new(x, row));
    }
}

// 占位,避免未使用警告
#[allow(dead_code)]
fn _char_width(c: char) -> u16 {
    UnicodeWidthChar::width(c).unwrap_or(0) as u16
}
