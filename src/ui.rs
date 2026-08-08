//! TUI 渲染。

use crate::app::{App, Focus, Screen, byte_idx, char_len, next_func_name};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use unicode_width::UnicodeWidthStr;

const CYAN: Style = Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD);
const GREEN: Style = Style::new().fg(Color::Green);
const RED: Style = Style::new().fg(Color::Red);
const YELLOW: Style = Style::new().fg(Color::Yellow);
const DIM: Style = Style::new().fg(Color::DarkGray);
const ACCENT: Style = Style::new().fg(Color::Rgb(198, 160, 255));

pub fn draw(f: &mut Frame, app: &mut App) {
    let area = f.area();
    let v = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(area);

    draw_top_bar(f, v[0]);
    let (expr_rect, func_rect) = draw_main(f, app, v[1]);
    draw_status_bar(f, app, v[2]);

    if app.screen == Screen::Main {
        draw_autocomplete(f, app, expr_rect, func_rect);
    }

    if app.screen == Screen::Settings {
        draw_settings_overlay(f, app);
    }
}

/// 自定义函数自动补全下拉框(绘制在活动输入框下方)。
fn draw_autocomplete(f: &mut Frame, app: &App, expr_rect: Rect, func_rect: Rect) {
    if app.auto.suppressed {
        return;
    }
    let Some((_, cands)) = app.autocomplete_state() else {
        return;
    };
    let box_rect = match app.focus {
        Focus::Main => expr_rect,
        Focus::Func => func_rect,
    };
    let max_rows = 6;
    let count = cands.len().min(max_rows);
    let width = cands
        .iter()
        .map(|n| UnicodeWidthStr::width(n.as_str()))
        .max()
        .unwrap_or(4)
        + 4;
    // 宽度至少容纳操作提示行
    let width = width.max(24);
    // 候选行 + 操作提示行 + 上下边框
    let h = count as u16 + 3;
    let x = box_rect.x + 1;
    let y = box_rect.bottom().min(f.area().height.saturating_sub(h));
    let rect = Rect::new(
        x,
        y,
        (width as u16).min(box_rect.width.saturating_sub(2)).max(8),
        h,
    );

    f.render_widget(ratatui::widgets::Clear, rect);

    let selected = app.auto.selected.min(cands.len() - 1);
    let mut lines: Vec<Line> = cands
        .iter()
        .take(max_rows)
        .enumerate()
        .map(|(i, n)| {
            let style = if i == selected {
                Style::new().fg(Color::Black).bg(Color::Cyan)
            } else {
                Style::new()
            };
            Line::from(Span::styled(n.as_str(), style))
        })
        .collect();
    lines.push(Line::from(Span::styled("↑/↓ 选择 · Enter 插入", DIM)));
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::new().fg(Color::Magenta))
        .title("函数")
        .title_style(CYAN);
    f.render_widget(Paragraph::new(lines).block(block), rect);
}

fn draw_top_bar(f: &mut Frame, area: Rect) {
    let title = Span::styled("计算器 Calculator", CYAN);
    f.render_widget(Paragraph::new(Line::from(title)), area);
}

/// 返回 (表达式框, 自定义函数框) 的矩形, 供自动补全下拉定位。
fn draw_main(f: &mut Frame, app: &mut App, area: Rect) -> (Rect, Rect) {
    if app.show_help {
        let h = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(58), Constraint::Percentage(42)])
            .split(area);
        let r = draw_left(f, app, h[0]);
        draw_help(f, app, h[1]);
        r
    } else {
        // 帮助关闭时整个窗口全部为计算页面
        draw_left(f, app, area)
    }
}

fn draw_left(f: &mut Frame, app: &mut App, area: Rect) -> (Rect, Rect) {
    let rows = app.eval.funcs.len().clamp(1, 6);
    let v = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),               // 表达式
            Constraint::Length(3),               // 结果
            Constraint::Min(0),                  // 历史
            Constraint::Length(3),               // 自定义函数
            Constraint::Length(2 + rows as u16), // 已有函数
        ])
        .split(area);
    draw_expr(f, app, v[0]);
    draw_result(f, app, v[1]);
    draw_history(f, app, v[2]);
    draw_func_input(f, app, v[3]);
    draw_func_list(f, app, v[4]);
    (v[0], v[3])
}

fn draw_expr(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title("表达式")
        .title_style(CYAN);
    let inner = block.inner(area);
    let text = if app.input.is_empty() {
        Line::from(Span::styled("在此输入表达式…", DIM))
    } else {
        Line::from(app.input.as_str())
    };
    f.render_widget(
        Paragraph::new(text).block(block).wrap(Wrap { trim: false }),
        area,
    );

    // 行尾固定徽标: 单独右对齐渲染, 始终贴在表达式框最右端(不随文字移动)
    let mode = if app.eval.degree {
        "[DEG 角度]"
    } else {
        "[RAD 弧度]"
    };
    let badge = Span::styled(mode, YELLOW.add_modifier(Modifier::BOLD));
    f.render_widget(
        Paragraph::new(Line::from(badge)).alignment(ratatui::layout::Alignment::Right),
        inner,
    );

    // 光标定位(仅表达式焦点)
    if app.screen == Screen::Main && app.focus == Focus::Main {
        let prefix = &app.input[..byte_idx(&app.input, app.cursor)];
        let w = UnicodeWidthStr::width(prefix) as u16;
        let inner_w = inner.width;
        let scroll = w.saturating_sub(inner_w.saturating_sub(1));
        let x = inner.x + (w - scroll).min(inner_w.saturating_sub(1));
        f.set_cursor_position(Position::new(x, inner.y));
    }
}

fn draw_func_input(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title("自定义函数")
        .title_style(CYAN);
    let inner = block.inner(area);
    let prefix = format!("{}(x)=", next_func_name(&app.eval.funcs));
    let line = if app.func_input.is_empty() {
        Line::from(vec![
            Span::styled(&prefix, YELLOW),
            Span::styled(" 在此输入函数体, Enter 保存 (Ctrl+F 切换焦点)", DIM),
        ])
    } else {
        Line::from(vec![
            Span::styled(&prefix, YELLOW),
            Span::raw(app.func_input.as_str()),
        ])
    };
    f.render_widget(
        Paragraph::new(line).block(block).wrap(Wrap { trim: false }),
        area,
    );

    // 光标定位(仅函数焦点)
    if app.screen == Screen::Main && app.focus == Focus::Func {
        let text = format!("{prefix}{}", app.func_input);
        let ci = char_len(&prefix) + app.func_cursor;
        let w = UnicodeWidthStr::width(&text[..byte_idx(&text, ci)]) as u16;
        let inner_w = inner.width;
        let scroll = w.saturating_sub(inner_w.saturating_sub(1));
        let x = inner.x + (w - scroll).min(inner_w.saturating_sub(1));
        f.set_cursor_position(Position::new(x, inner.y));
    }
}

fn draw_func_list(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title("已有函数")
        .title_style(CYAN);
    let mut lines: Vec<Line> = app
        .eval
        .funcs
        .iter()
        .map(|f| {
            Line::from(vec![
                Span::styled(format!("{} = ", f.name), Style::new().fg(Color::White)),
                Span::styled(&f.body, GREEN),
            ])
        })
        .collect();
    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            "暂无自定义函数 (上方输入后按 Enter 创建)",
            DIM,
        )));
    }
    f.render_widget(Paragraph::new(lines).block(block), area);
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
        None => Line::from(Span::styled("输入后实时显示结果", DIM)),
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
        .title("帮助")
        .title_style(CYAN);
    let mut lines: Vec<Line> = Vec::new();

    // 由配置中的组合按键反查按键名(用户修改后帮助同步显示新按键)
    let combo_key = |insert: &str| -> Option<String> {
        app.combos
            .iter()
            .find(|c| c.insert == insert)
            .map(|c| c.keys.clone())
    };

    struct Row {
        left: &'static str,
        desc: &'static str,
        combo: Option<String>,
    }

    // 按各列最大宽度对齐的表格行, 行尾附组合按键
    let table = |lines: &mut Vec<Line>, rows: Vec<Row>| {
        let lw = rows
            .iter()
            .map(|r| UnicodeWidthStr::width(r.left))
            .max()
            .unwrap_or(0);
        let dw = rows
            .iter()
            .map(|r| UnicodeWidthStr::width(r.desc))
            .max()
            .unwrap_or(0);
        let kw = rows
            .iter()
            .filter_map(|r| r.combo.as_deref())
            .map(UnicodeWidthStr::width)
            .max()
            .unwrap_or(0);
        for r in rows {
            let mut spans = vec![
                Span::styled(
                    format!(
                        "{}{}",
                        r.left,
                        " ".repeat(lw.saturating_sub(UnicodeWidthStr::width(r.left)))
                    ),
                    Style::new().fg(Color::White),
                ),
                Span::styled(" | ", ACCENT),
                Span::styled(
                    format!(
                        "{}{}",
                        r.desc,
                        " ".repeat(dw.saturating_sub(UnicodeWidthStr::width(r.desc)))
                    ),
                    Style::new().fg(Color::Green),
                ),
            ];
            if let Some(k) = &r.combo {
                spans.push(Span::styled(
                    format!(
                        "  {}{}",
                        k,
                        " ".repeat(kw.saturating_sub(UnicodeWidthStr::width(k.as_str())))
                    ),
                    ACCENT,
                ));
            }
            lines.push(Line::from(spans));
        }
    };
    let sec = |lines: &mut Vec<Line>, title: &str| {
        lines.push(Line::from(Span::styled(format!("── {title} ──"), CYAN)));
    };

    sec(&mut lines, "常数");
    table(
        &mut lines,
        vec![
            Row {
                left: "π e",
                desc: "圆周率等",
                combo: combo_key("π"),
            },
            Row {
                left: "φ g",
                desc: "黄金·重力",
                combo: combo_key("φ"),
            },
            Row {
                left: "i γ ln2",
                desc: "虚数单位·欧拉",
                combo: None,
            },
            Row {
                left: "ln10 sqrt2",
                desc: "常用常量",
                combo: None,
            },
        ],
    );

    sec(&mut lines, "函数");
    table(
        &mut lines,
        vec![
            Row {
                left: "sin cos tan",
                desc: "三角函数",
                combo: None,
            },
            Row {
                left: "asin acos atan",
                desc: "反三角·atan2",
                combo: None,
            },
            Row {
                left: "sinh cosh tanh",
                desc: "双曲",
                combo: None,
            },
            Row {
                left: "asinh…atanh",
                desc: "反双曲",
                combo: None,
            },
            Row {
                left: "sec csc cot",
                desc: "余函数",
                combo: None,
            },
            Row {
                left: "sech csch coth",
                desc: "双曲余函数",
                combo: None,
            },
            Row {
                left: "ln log logb",
                desc: "对数族(2/10底)",
                combo: None,
            },
            Row {
                left: "exp sqrt abs",
                desc: "指数·根·模",
                combo: None,
            },
            Row {
                left: "floor…round",
                desc: "取整·舍入",
                combo: None,
            },
            Row {
                left: "frac signum",
                desc: "小数·符号",
                combo: None,
            },
            Row {
                left: "mod(x,y) %",
                desc: "取余",
                combo: None,
            },
            Row {
                left: "n! gamma(x)",
                desc: "阶乘·Γ函数",
                combo: None,
            },
            Row {
                left: "C(n,k) A(n,k)",
                desc: "组合·排列",
                combo: None,
            },
            Row {
                left: "re im conj arg",
                desc: "分量·共轭·辐角",
                combo: None,
            },
            Row {
                left: "max min mean",
                desc: "最值·均值",
                combo: None,
            },
            Row {
                left: "var std",
                desc: "方差·标准差",
                combo: None,
            },
        ],
    );

    sec(&mut lines, "高级运算 (参数顺序固定)");
    table(
        &mut lines,
        vec![
            Row {
                left: "Σ(k,1,10,k²)",
                desc: "求和(可∞)",
                combo: combo_key("Σ("),
            },
            Row {
                left: "∫(x,0,1,x²)",
                desc: "积分(可∞)",
                combo: combo_key("∫("),
            },
            Row {
                left: "deriv(x²,x,2)",
                desc: "求导",
                combo: combo_key("deriv("),
            },
        ],
    );

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
            " Enter 记录 · Tab R/D · Ctrl+F 函数 · F2 设置 · Ctrl+H 帮助 · Esc 清空 · Ctrl+Q 退出 ",
        ),
        Screen::Settings => (
            Style::new().fg(Color::Black).bg(Color::Magenta),
            " ←/→ 标签 | ↑/↓ 选择 | Enter 编辑 | a 添加 | d 删除 | r 恢复默认 | Esc 返回 ",
        ),
    };
    f.render_widget(Paragraph::new(Line::from(Span::styled(text, style))), area);
}

// ------------------------------------------------------------ 设置覆盖层

fn draw_settings_overlay(f: &mut Frame, app: &App) {
    let area = f.area();
    let list_len = if app.set.tab == 0 {
        app.combos.len()
    } else {
        app.eval.funcs.len()
    };
    let w = (area.width.saturating_sub(8)).min(72);
    let h = ((list_len + 10) as u16)
        .min(area.height.saturating_sub(6))
        .max(8);
    let x = area.x + (area.width - w) / 2;
    let y = area.y + (area.height - h) / 2;
    let rect = Rect::new(x, y, w, h);

    f.render_widget(ratatui::widgets::Clear, rect);

    let mut lines: Vec<Line> = Vec::new();
    // 标签行
    let tab_line: Vec<Span> = [("组合按键", 0u8), ("自定义函数", 1u8)]
        .iter()
        .map(|(label, t)| {
            let active = app.set.tab == *t;
            Span::styled(
                format!("[{label}]"),
                if active {
                    ACCENT.add_modifier(Modifier::BOLD)
                } else {
                    DIM
                },
            )
        })
        .collect();
    lines.push(Line::from(Span::raw("←/→ 切换标签  ")));
    lines[0].spans.extend(tab_line);
    lines.push(Line::from(""));

    if app.set.tab == 0 {
        draw_combo_rows(app, &mut lines);
    } else {
        draw_func_rows(app, &mut lines);
    }

    let editing = app.set.editing.is_some() || app.set.func_editing.is_some();
    let hint = if editing {
        "编辑中: 输入字符修改 | Enter 保存 | Esc 取消"
    } else if app.set.tab == 0 {
        "↑/↓ 选择 | Enter 编辑 | a 添加 | d 删除(预设不可删) | r 恢复默认 | Esc 返回"
    } else {
        "↑/↓ 选择 | Enter 编辑 | d 删除 | Esc 返回"
    };
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(hint, DIM)));
    if let Some(msg) = &app.set.msg {
        let style = if msg == "已保存" { GREEN } else { RED };
        lines.push(Line::from(Span::styled(msg, style)));
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::new().fg(Color::Magenta))
        .title("设置")
        .title_style(CYAN);
    let inner = block.inner(rect);
    f.render_widget(Paragraph::new(lines).block(block), rect);

    // 编辑时光标
    if let Some(ed) = app.set.editing.as_ref() {
        let keys_w = app
            .combos
            .iter()
            .map(|c| UnicodeWidthStr::width(c.keys.as_str()))
            .max()
            .unwrap_or(4)
            + 2;
        let row = inner.y + 2 + app.set.selected as u16;
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

/// 组合按键标签页的行。
fn draw_combo_rows<'a>(app: &'a App, lines: &mut Vec<Line<'a>>) {
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
            spans.push(Span::styled("→", ACCENT));
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
            if c.preset {
                spans.push(Span::styled(" 固定", DIM));
            }
        }
        lines.push(Line::from(spans));
    }
}

/// 自定义函数标签页的行。
fn draw_func_rows<'a>(app: &'a App, lines: &mut Vec<Line<'a>>) {
    for (i, f) in app.eval.funcs.iter().enumerate() {
        let num = Span::raw(format!("{:>2} ", i + 1));
        let mut spans = vec![num];
        let editing = app
            .set
            .func_editing
            .as_ref()
            .filter(|_| i == app.set.selected);
        if let Some(body) = editing {
            spans.push(Span::styled(
                format!("{} = ", f.name),
                Style::new().fg(Color::Yellow),
            ));
            spans.push(Span::styled(
                body.as_str(),
                Style::new()
                    .fg(Color::Black)
                    .bg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ));
        } else {
            let sel = i == app.set.selected;
            let base = if sel {
                Style::new().fg(Color::Black).bg(Color::Cyan)
            } else {
                Style::new()
            };
            spans.push(Span::styled(format!("{} = ", f.name), base));
            let mut body: String = f.body.clone();
            if body.chars().count() > 48 {
                body = body.chars().take(48).collect();
                body.push('…');
            }
            spans.push(Span::styled(body, base.add_modifier(Modifier::BOLD)));
        }
        lines.push(Line::from(spans));
    }
    if app.eval.funcs.is_empty() {
        lines.push(Line::from(Span::styled(
            "暂无自定义函数 (主界面函数输入框按 Enter 创建)",
            DIM,
        )));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::App;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::backend::TestBackend;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    /// 用 TestBackend(无需真实终端)渲染 80x24 界面, 按行返回文本。
    /// 中文字符在 buffer 中占 2 列, 跳过其占用的后续列, 重建视觉文本。
    fn render(app: &mut App) -> Vec<String> {
        let backend = TestBackend::new(80, 30);
        let mut terminal = ratatui::Terminal::new(backend).expect("创建测试终端失败");
        terminal.draw(|f| draw(f, app)).expect("渲染失败");
        let buf = terminal.backend().buffer();
        let mut lines = Vec::new();
        for y in 0..buf.area.height {
            let mut s = String::new();
            let mut x = 0;
            while x < buf.area.width {
                let sym = buf[(x, y)].symbol();
                s.push_str(sym);
                x += UnicodeWidthStr::width(sym).max(1) as u16;
            }
            lines.push(s);
        }
        lines
    }

    fn any_line(lines: &[String], needle: &str) -> bool {
        lines.iter().any(|l| l.contains(needle))
    }

    #[test]
    fn main_screen_layout() {
        let mut app = App::new();
        app.combos = crate::config::default_combos();
        let key_h = KeyEvent::new(KeyCode::Char('h'), KeyModifiers::CONTROL);
        app.handle_key(key_h);
        let lines = render(&mut app);
        assert!(any_line(&lines, "计算器 Calculator"));
        assert!(any_line(&lines, "[RAD 弧度]"));
        assert!(any_line(&lines, "表达式"));
        assert!(any_line(&lines, "结果"));
        assert!(any_line(&lines, "历史"));
        assert!(any_line(&lines, "常数"), "帮助应包含常数区域");
        assert!(any_line(&lines, "函数"), "帮助应包含函数区域");
        assert!(any_line(&lines, "高级运算"), "帮助应包含高级运算区域");
        assert!(any_line(&lines, "π e"), "常数区应显示 π e");
        assert!(any_line(&lines, "φ g"), "常数区应显示 φ g");
        assert!(any_line(&lines, "i γ ln2"), "常数区应显示虚数单位与常量");
        assert!(any_line(&lines, "\\f"), "帮助中 φ 应显示组合按键 \\f");
        assert!(any_line(&lines, "frac signum"), "帮助应显示 frac/signum");
        assert!(any_line(&lines, "re im conj arg"), "帮助应显示复数分量函数");
        assert!(any_line(&lines, "max min mean"), "帮助应显示统计函数");
        assert!(any_line(&lines, "var std"), "帮助应显示方差/标准差");
        assert!(any_line(&lines, "gamma(x)"), "帮助应显示 Γ 函数");
        assert!(any_line(&lines, "C(n,k)"), "帮助应显示组合数");
        assert!(any_line(&lines, "logb"), "帮助应显示 logb");
        assert!(any_line(&lines, "\\p"), "帮助中 π 应显示组合按键 \\p");
        assert!(any_line(&lines, "输入后实时显示结果"));
        assert!(any_line(&lines, "暂无记录"));
        assert!(any_line(&lines, "Enter 记录"), "底部状态栏应显示按键提示");
    }

    #[test]
    fn ctrl_h_toggles_help_panel() {
        let mut app = App::new();
        app.combos = crate::config::default_combos();
        let key_h = KeyEvent::new(KeyCode::Char('h'), KeyModifiers::CONTROL);
        assert!(!any_line(&render(&mut app), "常数"), "默认不显示帮助内容");
        assert!(
            any_line(&render(&mut app), "历史"),
            "隐藏帮助时计算页面应占满窗口"
        );
        app.handle_key(key_h);
        assert!(app.show_help);
        assert!(any_line(&render(&mut app), "常数"), "按 Ctrl+H 应显示帮助");
        app.handle_key(key_h);
        assert!(!app.show_help);
    }

    #[test]
    fn evaluate_shows_result_and_history() {
        let mut app = App::new();
        app.combos = crate::config::default_combos();
        for c in "2+3".chars() {
            app.handle_key(key(KeyCode::Char(c)));
        }
        let lines = render(&mut app);
        assert!(any_line(&lines, "2+3"), "应显示输入内容");
        assert!(
            any_line(&lines, "= 5"),
            "输入过程中结果区应实时显示 = 5, 实际: {lines:?}"
        );
        app.handle_key(key(KeyCode::Enter));
        let lines = render(&mut app);
        assert!(
            any_line(&lines, "输入后实时显示结果"),
            "回车后结果区应清空为占位提示, 实际: {lines:?}"
        );
        assert!(
            any_line(&lines, "2+3 = 5"),
            "历史区应显示 2+3 = 5, 实际: {lines:?}"
        );
    }

    #[test]
    fn invalid_input_shows_red_error_on_enter() {
        let mut app = App::new();
        app.combos = crate::config::default_combos();
        for c in "2+".chars() {
            app.handle_key(key(KeyCode::Char(c)));
        }
        app.handle_key(key(KeyCode::Enter));
        let lines = render(&mut app);
        assert!(
            any_line(&lines, "✗"),
            "结果区应显示红色错误提示, 实际: {lines:?}"
        );
        assert!(any_line(&lines, "2+"), "输入不应被清空");
    }

    #[test]
    fn tab_toggle_shows_degree_badge() {
        let mut app = App::new();
        app.combos = crate::config::default_combos();
        app.handle_key(key(KeyCode::Tab));
        assert!(any_line(&render(&mut app), "[DEG 角度]"));
    }

    #[test]
    fn settings_overlay_renders_combos() {
        let mut app = App::new();
        app.combos = crate::config::default_combos();
        app.handle_key(key(KeyCode::F(2)));
        let lines = render(&mut app);
        assert!(any_line(&lines, "[组合按键]"), "设置页应显示组合按键标签");
        assert!(any_line(&lines, "\\p"), "设置列表应显示组合按键 \\p");
        assert!(any_line(&lines, "π"), "设置列表应显示插入内容 π");
        assert!(any_line(&lines, "Σ("), "设置列表应显示插入内容 Σ(");
        assert!(any_line(&lines, "↑/↓ 选择"), "应显示设置界面按键提示");
    }

    #[test]
    fn func_input_and_list_render() {
        let mut app = App::new();
        app.combos = crate::config::default_combos();
        app.eval.funcs = vec![crate::config::UserFunc {
            name: "f_1".into(),
            body: "x+1".into(),
        }];
        let lines = render(&mut app);
        assert!(
            any_line(&lines, "自定义函数"),
            "主界面应显示自定义函数输入框"
        );
        assert!(any_line(&lines, "已有函数"), "主界面应显示已有函数列表");
        assert!(
            any_line(&lines, "f_2(x)="),
            "已有 f_1 时输入框前缀应为 f_2(x)="
        );
        assert!(
            any_line(&lines, "f_1 = x+1"),
            "已有函数列表应显示 f_1 = x+1"
        );
    }

    #[test]
    fn func_tab_renders_in_settings() {
        let mut app = App::new();
        app.combos = crate::config::default_combos();
        app.eval.funcs = vec![crate::config::UserFunc {
            name: "f_1".into(),
            body: "sin(x)^2".into(),
        }];
        app.handle_key(key(KeyCode::F(2)));
        app.handle_key(key(KeyCode::Right)); // 切到自定义函数标签
        let lines = render(&mut app);
        assert!(any_line(&lines, "[自定义函数]"));
        assert!(any_line(&lines, "f_1 = sin(x)^2"));
        assert!(any_line(&lines, "d 删除"), "函数标签页应提示 d 删除");
    }

    #[test]
    fn autocomplete_dropdown_renders() {
        let mut app = App::new();
        app.combos = crate::config::default_combos();
        app.eval.funcs = vec![
            crate::config::UserFunc {
                name: "f_1".into(),
                body: "x+1".into(),
            },
            crate::config::UserFunc {
                name: "f_2".into(),
                body: "x*2".into(),
            },
        ];
        app.handle_key(key(KeyCode::Char('f')));
        let lines = render(&mut app);
        assert!(any_line(&lines, "Enter 插入"), "下拉框应显示操作提示");
        assert!(any_line(&lines, "↑/↓ 选择"), "下拉框应显示选择提示");
        assert!(
            lines.iter().filter(|l| l.contains("f_1")).count() >= 1,
            "下拉框应包含候选函数 f_1"
        );
        // 关闭下拉后不再渲染
        app.handle_key(key(KeyCode::Esc));
        let lines = render(&mut app);
        assert!(!any_line(&lines, "Enter 插入"), "Esc 关闭后不应渲染下拉框");
    }
}
