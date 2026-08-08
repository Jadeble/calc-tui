//! 应用状态与按键处理。

use crate::complex::Cmplx;
use crate::config::{Combo, UserFunc, load_config, save_config};
use crate::math::{Evaluator, fmt_result, func_name_referenced};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Clone, PartialEq, Copy, Debug)]
pub enum Screen {
    Main,
    Settings,
}

/// 主窗口输入焦点: 表达式输入框或自定义函数输入框。
#[derive(Clone, PartialEq, Copy, Debug, Default)]
pub enum Focus {
    #[default]
    Main,
    Func,
}

/// 自定义函数自动补全下拉状态。
#[derive(Clone, Copy, Debug, Default)]
pub struct AutoState {
    pub selected: usize,
    /// 用户按 Esc 关闭下拉后, 在输入内容变化前不再弹出
    pub suppressed: bool,
}

#[derive(Clone)]
pub struct HistoryEntry {
    pub expr: String,
    pub result: String,
}

pub struct App {
    pub screen: Screen,
    // 输入
    pub input: String,
    pub cursor: usize, // 字符下标
    pub combo_token: Option<String>,
    pub combo_token_start: usize,
    // 计算
    pub eval: Evaluator,
    pub result: Option<Result<Cmplx, String>>,
    pub history: Vec<HistoryEntry>,
    pub hist_idx: Option<usize>,
    pub saved_input: String,
    // 配置
    pub combos: Vec<Combo>,
    // 自定义函数
    pub func_input: String,
    pub func_cursor: usize,
    pub focus: Focus,
    // 界面
    pub show_help: bool,
    // 自动补全
    pub auto: AutoState,
    // 设置界面
    pub set: SettingsState,
    pub quit: bool,
}

pub struct SettingsState {
    /// 0 = 组合按键, 1 = 自定义函数
    pub tab: u8,
    pub selected: usize,
    pub editing: Option<EditBuf>,
    /// 自定义函数标签页中正在内联编辑的函数体
    pub func_editing: Option<String>,
    pub msg: Option<String>,
}

pub struct EditBuf {
    pub keys: String,
    pub insert: String,
    pub focus_keys: bool, // true=编辑按键, false=编辑插入内容
}

impl App {
    pub fn new() -> Self {
        // 测试环境不读写真实配置文件(避免并行测试互相污染)
        let mut cfg = if cfg!(test) {
            crate::config::Config {
                combos: crate::config::default_combos(),
                functions: Vec::new(),
            }
        } else {
            load_config()
        };
        let mut eval = Evaluator::new(false);
        eval.funcs = std::mem::take(&mut cfg.functions);
        Self {
            screen: Screen::Main,
            input: String::new(),
            cursor: 0,
            combo_token: None,
            combo_token_start: 0,
            eval,
            result: None,
            history: Vec::new(),
            hist_idx: None,
            saved_input: String::new(),
            combos: cfg.combos,
            func_input: String::new(),
            func_cursor: 0,
            focus: Focus::Main,
            show_help: false,
            auto: AutoState::default(),
            set: SettingsState {
                tab: 0,
                selected: 0,
                editing: None,
                func_editing: None,
                msg: None,
            },
            quit: false,
        }
    }

    /// 处理按键; 返回 true 表示退出。
    pub fn handle_key(&mut self, key: KeyEvent) -> bool {
        if key.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(key.code, KeyCode::Char('c' | 'C' | 'q' | 'Q'))
        {
            self.quit = true;
            return true;
        }
        match self.screen {
            Screen::Settings => self.settings_key(key),
            Screen::Main => self.main_key(key),
        }
        false
    }

    /// 当前活动输入框中光标前的"当前词"(标识符段)。
    /// 若以 f 开头且有匹配的自定义函数, 返回 (当前词起始字符下标, 候选函数名)。
    pub fn autocomplete_state(&self) -> Option<(usize, Vec<String>)> {
        let (text, cursor) = match self.focus {
            Focus::Main => (self.input.as_str(), self.cursor),
            Focus::Func => (self.func_input.as_str(), self.func_cursor),
        };
        let chars: Vec<char> = text.chars().collect();
        let mut start = cursor;
        while start > 0 && is_ident_char(chars[start - 1]) {
            start -= 1;
        }
        let word: String = chars[start..cursor].iter().collect();
        if word.is_empty() || !matches!(word.chars().next(), Some('f' | 'F')) {
            return None;
        }
        let wl = word.to_ascii_lowercase();
        let cands: Vec<String> = self
            .eval
            .funcs
            .iter()
            .map(|f| f.name.clone())
            .filter(|n| n.to_ascii_lowercase().starts_with(&wl))
            .collect();
        if cands.is_empty() {
            return None;
        }
        Some((start, cands))
    }

    /// 输入内容变化后重置自动补全(重新弹出, 选中回到第一项)。
    fn reset_autocomplete(&mut self) {
        self.auto.suppressed = false;
        self.auto.selected = 0;
    }

    /// 用选中的函数名替换当前词。
    fn accept_autocomplete(&mut self) {
        let Some((start, cands)) = self.autocomplete_state() else {
            return;
        };
        let Some(name) = cands.get(self.auto.selected.min(cands.len().saturating_sub(1))) else {
            return;
        };
        match self.focus {
            Focus::Main => {
                remove_range(&mut self.input, start, self.cursor);
                insert_at(&mut self.input, start, name);
                self.cursor = start + char_len(name);
                self.combo_token = None;
                self.recalc();
            }
            Focus::Func => {
                remove_range(&mut self.func_input, start, self.func_cursor);
                insert_at(&mut self.func_input, start, name);
                self.func_cursor = start + char_len(name);
            }
        }
        self.auto.suppressed = true;
    }

    // ------------------------------------------------------------ 主界面

    fn main_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('h' | 'H') => {
                    self.show_help = !self.show_help;
                    self.combo_token = None;
                    return;
                }
                KeyCode::Char('f' | 'F') => {
                    self.focus = match self.focus {
                        Focus::Main => Focus::Func,
                        Focus::Func => Focus::Main,
                    };
                    self.combo_token = None;
                    self.reset_autocomplete();
                    return;
                }
                _ => {}
            }
        }
        // 自动补全下拉: ↑/↓ 选择, Enter 插入, Esc 关闭
        if !self.auto.suppressed
            && let Some((_, cands)) = self.autocomplete_state()
        {
            let last = cands.len().saturating_sub(1);
            match key.code {
                KeyCode::Up => {
                    self.auto.selected = self.auto.selected.saturating_sub(1).min(last);
                    return;
                }
                KeyCode::Down => {
                    self.auto.selected = (self.auto.selected + 1).min(last);
                    return;
                }
                KeyCode::Enter => {
                    self.accept_autocomplete();
                    return;
                }
                KeyCode::Esc => {
                    self.auto.suppressed = true;
                    return;
                }
                _ => {}
            }
        }
        match self.focus {
            Focus::Func => self.func_input_key(key),
            Focus::Main => self.main_expr_key(key),
        }
    }

    fn main_expr_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Enter => self.evaluate(),
            KeyCode::Esc => {
                // Esc 仅清空输入区域
                if !self.input.is_empty() {
                    self.input.clear();
                    self.cursor = 0;
                    self.combo_token = None;
                    self.result = None;
                    self.hist_idx = None;
                    self.reset_autocomplete();
                }
            }
            KeyCode::Tab => {
                self.eval.degree = !self.eval.degree;
                self.combo_token = None;
                self.recalc();
            }
            KeyCode::F(2) => {
                self.screen = Screen::Settings;
                self.set.tab = 0;
                self.set.selected = 0;
                self.set.editing = None;
                self.set.func_editing = None;
                self.set.msg = None;
            }
            KeyCode::Char(c) => {
                if !c.is_control() {
                    self.insert_char(c);
                }
            }
            KeyCode::Backspace => {
                self.combo_token = None;
                char_remove_before(&mut self.input, &mut self.cursor);
                self.recalc();
                self.reset_autocomplete();
            }
            KeyCode::Delete => {
                self.combo_token = None;
                char_remove_at(&mut self.input, &mut self.cursor);
                self.recalc();
                self.reset_autocomplete();
            }
            KeyCode::Left => {
                self.combo_token = None;
                self.cursor = self.cursor.saturating_sub(1);
            }
            KeyCode::Right => {
                self.combo_token = None;
                if self.cursor < char_len(&self.input) {
                    self.cursor += 1;
                }
            }
            KeyCode::Home => {
                self.combo_token = None;
                self.cursor = 0;
            }
            KeyCode::End => {
                self.combo_token = None;
                self.cursor = char_len(&self.input);
            }
            KeyCode::Up => self.history_up(),
            KeyCode::Down => self.history_down(),
            _ => {}
        }
    }

    /// 自定义函数输入框按键处理。
    fn func_input_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Enter => self.save_func(),
            KeyCode::Esc => {
                if !self.func_input.is_empty() {
                    self.func_input.clear();
                    self.func_cursor = 0;
                    self.reset_autocomplete();
                }
            }
            KeyCode::Backspace => {
                char_remove_before(&mut self.func_input, &mut self.func_cursor);
                self.reset_autocomplete();
            }
            KeyCode::Delete => {
                char_remove_at(&mut self.func_input, &mut self.func_cursor);
                self.reset_autocomplete();
            }
            KeyCode::Left => self.func_cursor = self.func_cursor.saturating_sub(1),
            KeyCode::Right => {
                if self.func_cursor < char_len(&self.func_input) {
                    self.func_cursor += 1;
                }
            }
            KeyCode::Home => self.func_cursor = 0,
            KeyCode::End => self.func_cursor = char_len(&self.func_input),
            KeyCode::Char(c) if !c.is_control() && self.func_input.chars().count() < 200 => {
                insert_at(&mut self.func_input, self.func_cursor, &c.to_string());
                self.func_cursor += 1;
                self.reset_autocomplete();
            }
            _ => {}
        }
    }

    /// 保存函数输入框内容为新的自定义函数, 名字取最小空闲编号。
    fn save_func(&mut self) {
        let body = self.func_input.trim().to_string();
        if body.is_empty() {
            self.result = Some(Err("函数体不能为空".into()));
            return;
        }
        let name = next_func_name(&self.eval.funcs);
        self.eval.funcs.push(UserFunc { name, body });
        self.func_input.clear();
        self.func_cursor = 0;
        self.save_all();
    }

    fn insert_char(&mut self, c: char) {
        self.reset_autocomplete();
        if let Some(token) = self.combo_token.clone() {
            let new_token = format!("{token}{c}");
            // 精确匹配: 替换为预设内容
            // 注意: 最后一个字符尚未插入输入串, 实际输入的部分是
            // [combo_token_start, cursor)(光标移动会清除 token, 二者必然连续)
            if let Some(combo) = self.combos.iter().find(|x| x.keys == new_token) {
                let start = self.combo_token_start;
                let end = self.cursor;
                remove_range(&mut self.input, start, end);
                insert_at(&mut self.input, start, &combo.insert);
                self.cursor = start + char_len(&combo.insert);
                self.combo_token = None;
                self.recalc();
                return;
            }
            // 仍是某组合的前缀则继续累积
            let is_prefix = self.combos.iter().any(|x| x.keys.starts_with(&new_token));
            if is_prefix {
                insert_at(&mut self.input, self.cursor, &c.to_string());
                self.cursor += 1;
                self.combo_token = Some(new_token);
                return;
            }
            // 无前缀匹配: 取消组合模式, 字符原样输入
            self.combo_token = None;
        }
        if c == '\\' && !self.combos.is_empty() {
            insert_at(&mut self.input, self.cursor, "\\");
            self.combo_token_start = self.cursor;
            self.cursor += 1;
            self.combo_token = Some("\\".to_string());
            return;
        }
        insert_at(&mut self.input, self.cursor, &c.to_string());
        self.cursor += 1;
        self.recalc();
    }

    /// 实时计算: 输入变化后更新结果。求值失败时不改变已有结果,
    /// 但会清除上一次 Enter 留下的错误提示。
    fn recalc(&mut self) {
        let expr = self.input.trim();
        if expr.is_empty() {
            self.result = None;
            return;
        }
        match self.eval.evaluate(expr) {
            Ok(v) => self.result = Some(Ok(v)),
            Err(_) => {
                if matches!(self.result, Some(Err(_))) {
                    self.result = None;
                }
            }
        }
    }

    fn evaluate(&mut self) {
        let expr = self.input.trim().to_string();
        if expr.is_empty() {
            return;
        }
        self.combo_token = None;
        match self.eval.evaluate(&expr) {
            Ok(v) => {
                self.history.push(HistoryEntry {
                    expr: expr.clone(),
                    result: fmt_result(v),
                });
                if self.history.len() > 500 {
                    self.history.remove(0);
                }
                // 成功后清空输入与结果, 式子+结果已存入历史
                self.result = None;
                self.input.clear();
                self.cursor = 0;
                self.hist_idx = None;
            }
            // 输入不合法: 红色提示错误, 不清空输入也不提交
            Err(e) => self.result = Some(Err(e)),
        }
    }

    fn history_up(&mut self) {
        if self.history.is_empty() {
            return;
        }
        self.reset_autocomplete();
        if let Some(idx) = self.hist_idx {
            if idx > 0 {
                self.hist_idx = Some(idx - 1);
            }
        } else {
            self.saved_input = self.input.clone();
            self.hist_idx = Some(self.history.len() - 1);
        }
        if let Some(idx) = self.hist_idx {
            self.input = self.history[idx].expr.clone();
            self.cursor = char_len(&self.input);
            self.combo_token = None;
            self.recalc();
        }
    }

    fn history_down(&mut self) {
        let Some(idx) = self.hist_idx else { return };
        self.reset_autocomplete();
        if idx + 1 < self.history.len() {
            self.hist_idx = Some(idx + 1);
            self.input = self.history[idx + 1].expr.clone();
        } else {
            self.hist_idx = None;
            self.input = std::mem::take(&mut self.saved_input);
        }
        self.cursor = char_len(&self.input);
        self.combo_token = None;
        self.recalc();
    }

    // ------------------------------------------------------------ 设置界面

    fn settings_key(&mut self, key: KeyEvent) {
        if let Some(body) = self.set.func_editing.as_mut() {
            match key.code {
                KeyCode::Esc => {
                    self.set.func_editing = None;
                    self.set.msg = None;
                }
                KeyCode::Enter => self.commit_func_edit(),
                KeyCode::Backspace => {
                    body.pop();
                }
                KeyCode::Char(c) if !c.is_control() && body.chars().count() < 200 => {
                    body.push(c);
                }
                _ => {}
            }
            return;
        }
        if let Some(ed) = self.set.editing.as_mut() {
            match key.code {
                KeyCode::Esc => {
                    self.set.editing = None;
                    self.set.msg = None;
                }
                KeyCode::Enter => self.commit_edit(),
                KeyCode::Tab => ed.focus_keys = !ed.focus_keys,
                KeyCode::Backspace => {
                    if ed.focus_keys {
                        ed.keys.pop();
                    } else {
                        ed.insert.pop();
                    }
                }
                KeyCode::Char(c) if !c.is_control() => {
                    if ed.focus_keys {
                        if ed.keys.chars().count() < 12 {
                            ed.keys.push(c);
                        }
                    } else if ed.insert.chars().count() < 24 {
                        ed.insert.push(c);
                    }
                }
                _ => {}
            }
            return;
        }
        match key.code {
            KeyCode::Esc => self.screen = Screen::Main,
            KeyCode::Left | KeyCode::Right => {
                // 切换标签页: 组合按键 ↔ 自定义函数
                self.set.tab ^= 1;
                self.set.selected = 0;
                self.set.msg = None;
            }
            KeyCode::Up => {
                if self.set.selected > 0 {
                    self.set.selected -= 1;
                }
            }
            KeyCode::Down => {
                let len = if self.set.tab == 0 {
                    self.combos.len()
                } else {
                    self.eval.funcs.len()
                };
                if self.set.selected + 1 < len {
                    self.set.selected += 1;
                }
            }
            KeyCode::Enter => match self.set.tab {
                0 => {
                    if let Some(c) = self.combos.get(self.set.selected) {
                        self.set.editing = Some(EditBuf {
                            keys: c.keys.clone(),
                            insert: c.insert.clone(),
                            focus_keys: true,
                        });
                        self.set.msg = None;
                    }
                }
                1 => {
                    if let Some(f) = self.eval.funcs.get(self.set.selected) {
                        self.set.func_editing = Some(f.body.clone());
                        self.set.msg = None;
                    }
                }
                _ => {}
            },
            KeyCode::Char('a') if self.set.tab == 0 => {
                self.combos.push(Combo {
                    keys: "\\z".into(),
                    insert: String::new(),
                    preset: false,
                });
                self.set.selected = self.combos.len() - 1;
                self.save_all();
            }
            KeyCode::Char('d') if self.set.tab == 0 => {
                // 预设组合(π/Σ/∫ 等无法直接键入的字符)不可删除, 只能改按键
                if let Some(c) = self.combos.get(self.set.selected)
                    && c.preset
                {
                    self.set.msg = Some("预设组合不可删除 (可修改按键)".into());
                    return;
                }
                if !self.combos.is_empty() {
                    self.combos
                        .remove(self.set.selected.min(self.combos.len() - 1));
                    if self.set.selected >= self.combos.len() && !self.combos.is_empty() {
                        self.set.selected = self.combos.len() - 1;
                    }
                    self.save_all();
                }
            }
            KeyCode::Char('d') => self.delete_func(),
            KeyCode::Char('r') if self.set.tab == 0 => {
                self.combos = crate::config::default_combos();
                self.set.selected = 0;
                self.save_all();
            }
            _ => {}
        }
    }

    /// 删除选中的自定义函数。被其他函数体引用时拦截并提示直接引用者。
    fn delete_func(&mut self) {
        let Some(func) = self.eval.funcs.get(self.set.selected) else {
            return;
        };
        let name = func.name.clone();
        let refs: Vec<String> = self
            .eval
            .funcs
            .iter()
            .filter(|f| f.name != name && func_name_referenced(&f.body, &name))
            .map(|f| f.name.clone())
            .collect();
        if !refs.is_empty() {
            self.set.msg = Some(format!("无法删除: {name} 被 {} 引用", refs.join("、")));
            return;
        }
        self.eval.funcs.remove(self.set.selected);
        // 同步清理历史中引用该函数的记录
        self.history
            .retain(|e| !func_name_referenced(&e.expr, &name));
        if self.set.selected >= self.eval.funcs.len() && !self.eval.funcs.is_empty() {
            self.set.selected = self.eval.funcs.len() - 1;
        }
        self.save_all();
    }

    fn commit_edit(&mut self) {
        let Some(ed) = self.set.editing.take() else {
            return;
        };
        let keys = ed.keys.trim().to_string();
        if keys.is_empty() {
            self.set.msg = Some("按键不能为空".into());
            self.set.editing = Some(ed);
            return;
        }
        if !keys.starts_with('\\') {
            self.set.msg = Some("按键必须以 \\ 开头".into());
            self.set.editing = Some(ed);
            return;
        }
        let dup = self
            .combos
            .iter()
            .enumerate()
            .any(|(i, c)| c.keys == keys && i != self.set.selected);
        if dup {
            self.set.msg = Some(format!("按键 {keys} 已存在"));
            self.set.editing = Some(ed);
            return;
        }
        if let Some(c) = self.combos.get_mut(self.set.selected) {
            c.keys = keys;
            c.insert = ed.insert.clone();
        }
        self.set.msg = Some("已保存".into());
        self.save_all();
    }

    /// 提交自定义函数体的内联编辑。
    fn commit_func_edit(&mut self) {
        let Some(body) = self.set.func_editing.take() else {
            return;
        };
        let trimmed = body.trim().to_string();
        if trimmed.is_empty() {
            self.set.msg = Some("函数体不能为空".into());
            self.set.func_editing = Some(body);
            return;
        }
        if let Some(f) = self.eval.funcs.get_mut(self.set.selected) {
            f.body = trimmed;
        }
        self.set.msg = Some("已保存".into());
        self.save_all();
    }

    /// 保存完整配置(组合按键 + 自定义函数)。
    fn save_all(&mut self) {
        let _ = save_config(&crate::config::Config {
            combos: self.combos.clone(),
            functions: self.eval.funcs.clone(),
        });
    }
}

/// 自定义函数的下一个名字: 取未被占用的最小编号(f_1, f_2, ...)。
/// 删除不重编号, 因此编号可能复用(删 f_2 后新建得到 f_2)。
pub fn next_func_name(funcs: &[UserFunc]) -> String {
    let mut n = 1;
    loop {
        let name = format!("f_{n}");
        if !funcs.iter().any(|f| f.name == name) {
            return name;
        }
        n += 1;
    }
}

// ------------------------------------------------------------ 字符串工具

fn is_ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

pub fn char_len(s: &str) -> usize {
    s.chars().count()
}

/// 将字符下标转为字节下标。
pub fn byte_idx(s: &str, ci: usize) -> usize {
    s.char_indices().nth(ci).map(|(b, _)| b).unwrap_or(s.len())
}

/// 在字符下标位置插入字符串。
pub fn insert_at(s: &mut String, ci: usize, ins: &str) {
    let b = byte_idx(s, ci);
    s.insert_str(b, ins);
}

/// 删除 [start, end) 字符区间。
pub fn remove_range(s: &mut String, start: usize, end: usize) {
    let bs = byte_idx(s, start);
    let be = byte_idx(s, end);
    s.replace_range(bs..be, "");
}

/// 删除光标前一个字符。
pub fn char_remove_before(s: &mut String, cursor: &mut usize) {
    if *cursor > 0 {
        let b = byte_idx(s, *cursor - 1);
        s.remove(b);
        *cursor -= 1;
    }
}

/// 删除光标处字符。
pub fn char_remove_at(s: &mut String, cursor: &mut usize) {
    if *cursor < char_len(s) {
        let b = byte_idx(s, *cursor);
        s.remove(b);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn evaluate_clears_input_and_sets_result() {
        let mut app = App::new();
        for c in "2+3*4".chars() {
            app.insert_char(c);
        }
        assert_eq!(app.input, "2+3*4");
        match &app.result {
            Some(Ok(v)) => assert!((*v - 14.0).abs() < 1e-9),
            other => panic!("输入过程中应实时显示 Ok(14), 实际: {other:?}"),
        }
        app.main_key(key(KeyCode::Enter));
        assert!(app.input.is_empty(), "输入应被清空, 实际: {}", app.input);
        assert!(
            app.result.is_none(),
            "回车后结果区域应清空, 实际: {:?}",
            app.result
        );
        assert_eq!(app.history.len(), 1);
        assert_eq!(app.history[0].expr, "2+3*4");
        assert_eq!(app.history[0].result, "14");
    }

    #[test]
    fn live_result_updates_as_typed() {
        let mut app = App::new();
        for c in "2".chars() {
            app.insert_char(c);
        }
        assert_eq!(app.result, Some(Ok(Cmplx::real(2.0))), "输入 2 应显示 = 2");
        app.insert_char('+');
        assert_eq!(
            app.result,
            Some(Ok(Cmplx::real(2.0))),
            "输入 + 后保持上一次有效结果 = 2"
        );
        app.insert_char('3');
        assert_eq!(
            app.result,
            Some(Ok(Cmplx::real(5.0))),
            "输入 3 后应实时显示 = 5"
        );
        app.insert_char('+');
        app.insert_char('3');
        assert_eq!(
            app.result,
            Some(Ok(Cmplx::real(8.0))),
            "再输入 +3 后应显示 = 8"
        );
    }

    #[test]
    fn enter_with_invalid_input_shows_error_and_keeps_input() {
        let mut app = App::new();
        for c in "2+".chars() {
            app.insert_char(c);
        }
        assert_eq!(
            app.result,
            Some(Ok(Cmplx::real(2.0))),
            "未按回车前保留上一次有效结果"
        );
        app.main_key(key(KeyCode::Enter));
        assert!(
            matches!(app.result, Some(Err(_))),
            "回车后应显示错误提示, 实际: {:?}",
            app.result
        );
        assert_eq!(app.input, "2+", "输入不合法时不应清空输入");
        assert!(app.history.is_empty(), "输入不合法时不应提交历史");
        // 修正输入后错误提示消失, 恢复实时结果
        app.main_key(key(KeyCode::Backspace));
        assert_eq!(
            app.result,
            Some(Ok(Cmplx::real(2.0))),
            "修正后应恢复实时结果"
        );
        app.insert_char('+');
        app.insert_char('3');
        assert_eq!(app.result, Some(Ok(Cmplx::real(5.0))));
    }

    #[test]
    fn unclosed_paren_not_evaluated_live() {
        let mut app = App::new();
        for c in "2+(".chars() {
            app.insert_char(c);
        }
        assert_eq!(
            app.result,
            Some(Ok(Cmplx::real(2.0))),
            "括号未闭合时不更新, 保留上一次结果"
        );
        app.insert_char('3');
        assert_eq!(app.result, Some(Ok(Cmplx::real(2.0))), "2+(3 仍未闭合");
        app.insert_char(')');
        assert_eq!(
            app.result,
            Some(Ok(Cmplx::real(5.0))),
            "补全 ) 后实时显示 = 5"
        );
    }

    #[test]
    fn combo_replacement() {
        let mut app = App::new();
        for c in "\\s".chars() {
            app.insert_char(c);
        }
        assert_eq!(app.input, "Σ(");
        for c in "k,1,10,k^2)".chars() {
            app.insert_char(c);
        }
        match &app.result {
            Some(Ok(v)) => assert!((*v - 385.0).abs() < 1e-9, "实际: {v}"),
            other => panic!("Σ 实时结果错误: {other:?}"),
        }
        app.main_key(key(KeyCode::Enter));
        assert!(app.result.is_none());
        assert!(app.input.is_empty());
        assert_eq!(app.history.len(), 1);
    }

    #[test]
    fn combo_replacement_mid_string() {
        // 回归: 光标在字符串中间输入 \p 时, 不应误删 \ 之后的原字符
        let mut app = App::new();
        for c in "sin()*2*cos(π)".chars() {
            app.insert_char(c);
        }
        app.cursor = 4; // 光标移到 sin( 与 ) 之间
        app.insert_char('\\');
        app.insert_char('p');
        assert_eq!(app.input, "sin(π)*2*cos(π)", "实际: {}", app.input);
        assert_eq!(app.cursor, 5);
        match &app.result {
            Some(Ok(v)) => assert!((*v).abs() < 1e-9, "sin(π)*2*cos(π) 应可求值: {v}"),
            other => panic!("组合替换后应能实时求值, 实际: {other:?}"),
        }
    }

    #[test]
    fn phi_combo_inserts_golden_ratio() {
        let mut app = App::new();
        for c in "\\f".chars() {
            app.insert_char(c);
        }
        assert_eq!(app.input, "φ");
        for c in "+1".chars() {
            app.insert_char(c);
        }
        match &app.result {
            Some(Ok(v)) => {
                let phi = (1.0 + 5.0f64.sqrt()) / 2.0;
                assert!((*v - (phi + 1.0)).abs() < 1e-9, "实际: {v}");
            }
            other => panic!("φ+1 实时结果错误: {other:?}"),
        }
        app.main_key(key(KeyCode::Enter));
        assert!(app.result.is_none());
        assert_eq!(app.history.len(), 1);
    }

    #[test]
    fn tab_toggles_degree() {
        let mut app = App::new();
        app.main_key(key(KeyCode::Tab));
        assert!(app.eval.degree);
        app.main_key(key(KeyCode::Tab));
        assert!(!app.eval.degree);
    }

    #[test]
    fn history_recall() {
        let mut app = App::new();
        for c in "1+1".chars() {
            app.insert_char(c);
        }
        app.main_key(key(KeyCode::Enter));
        for c in "2+2".chars() {
            app.insert_char(c);
        }
        app.main_key(key(KeyCode::Enter));
        assert_eq!(app.history.len(), 2);
        app.main_key(key(KeyCode::Up));
        assert_eq!(app.input, "2+2");
        app.main_key(key(KeyCode::Up));
        assert_eq!(app.input, "1+1");
        app.main_key(key(KeyCode::Down));
        assert_eq!(app.input, "2+2");
        app.main_key(key(KeyCode::Down));
        assert_eq!(app.input, "");
    }

    #[test]
    fn esc_only_clears_input() {
        let mut app = App::new();
        for c in "sqrt(16)".chars() {
            app.insert_char(c);
        }
        app.main_key(key(KeyCode::Esc));
        assert!(!app.quit, "Esc 不应退出");
        assert!(app.input.is_empty());
        app.main_key(key(KeyCode::Esc));
        assert!(!app.quit, "输入为空时 Esc 也不应退出");
        assert!(app.input.is_empty());
    }

    #[test]
    fn ctrl_q_quits() {
        let mut app = App::new();
        let k = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL);
        assert!(app.handle_key(k), "Ctrl+Q 应退出");
        assert!(app.quit);
        let mut app = App::new();
        assert!(app.handle_key(KeyEvent::new(KeyCode::Char('Q'), KeyModifiers::CONTROL)));
        assert!(app.quit);
    }

    #[test]
    fn ctrl_h_toggles_help() {
        let mut app = App::new();
        assert!(!app.show_help, "默认不显示帮助");
        let k = KeyEvent::new(KeyCode::Char('h'), KeyModifiers::CONTROL);
        app.handle_key(k);
        assert!(app.show_help);
        assert!(app.input.is_empty(), "Ctrl+H 不应输入字符");
        app.handle_key(k);
        assert!(!app.show_help);
    }

    #[test]
    fn q_is_a_normal_char() {
        let mut app = App::new();
        for c in "sqrt(4)".chars() {
            app.insert_char(c);
        }
        assert_eq!(app.input, "sqrt(4)");
        assert!(!app.quit);
        match &app.result {
            Some(Ok(v)) => assert!((*v - 2.0).abs() < 1e-9),
            other => panic!("sqrt(4) 实时结果应为 2, 实际: {other:?}"),
        }
        app.main_key(key(KeyCode::Enter));
        assert!(app.result.is_none(), "回车后结果区域应清空");
        assert_eq!(app.history.len(), 1);
    }

    #[test]
    fn settings_edit_commit() {
        let mut app = App::new();
        app.main_key(key(KeyCode::F(2)));
        assert_eq!(app.screen, Screen::Settings);
        app.settings_key(key(KeyCode::Enter));
        assert!(app.set.editing.is_some());
        app.settings_key(key(KeyCode::Backspace)); // 删除 \p 的 p
        app.settings_key(key(KeyCode::Backspace)); // 删除 \
        app.settings_key(key(KeyCode::Enter)); // 提交 (空按键)
        assert!(app.set.editing.is_some(), "空按键应被拦截");
        assert!(app.set.msg.is_some());
        app.settings_key(key(KeyCode::Esc));
        assert!(app.set.editing.is_none());
        app.settings_key(key(KeyCode::Esc));
        assert_eq!(app.screen, Screen::Main);
    }

    #[test]
    fn preset_combos_cannot_be_deleted() {
        let mut app = App::new();
        // 所有默认组合都是预设
        assert!(app.combos.iter().all(|c| c.preset), "默认组合应全部为预设");
        app.main_key(key(KeyCode::F(2)));
        let count = app.combos.len();
        app.settings_key(key(KeyCode::Char('d'))); // 尝试删除第一个预设 \p
        assert_eq!(app.combos.len(), count, "预设组合不应被删除");
        assert!(
            app.set.msg.as_deref().is_some_and(|m| m.contains("预设")),
            "应提示预设不可删除"
        );
        // 恢复默认后仍全部为预设
        app.settings_key(key(KeyCode::Char('r')));
        assert!(app.combos.iter().all(|c| c.preset));
    }

    #[test]
    fn user_added_combo_can_be_deleted() {
        let mut app = App::new();
        app.main_key(key(KeyCode::F(2)));
        app.settings_key(key(KeyCode::Char('a'))); // 添加 \z
        let last = app.combos.len() - 1;
        assert!(!app.combos[last].preset, "新添加的组合不应是预设");
        app.settings_key(key(KeyCode::Down));
        app.settings_key(key(KeyCode::Char('d'))); // 删除它
        assert!(
            !app.combos.iter().any(|c| c.keys == "\\z"),
            "用户添加的组合应可删除"
        );
    }

    #[test]
    fn preset_combo_keys_editable() {
        let mut app = App::new();
        app.main_key(key(KeyCode::F(2)));
        // 修改预设组合的按键 \p → \q (仍保留 insert π)
        app.settings_key(key(KeyCode::Enter));
        app.settings_key(key(KeyCode::Backspace)); // 删 p, 保留 \
        app.settings_key(key(KeyCode::Char('q')));
        app.settings_key(key(KeyCode::Enter));
        assert_eq!(app.combos[0].keys, "\\q");
        assert_eq!(app.combos[0].insert, "π");
        assert!(app.combos[0].preset, "修改按键后仍应保持预设保护");
        // 回到主屏, \q 仍能输入 π
        app.settings_key(key(KeyCode::Esc));
        app.main_key(key(KeyCode::Char('\\')));
        app.main_key(key(KeyCode::Char('q')));
        assert_eq!(app.input, "π");
    }

    // ------------------------------------------------------------ 自定义函数

    #[test]
    fn ctrl_f_toggles_focus() {
        let mut app = App::new();
        assert_eq!(app.focus, Focus::Main);
        let k = KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL);
        app.handle_key(k);
        assert_eq!(app.focus, Focus::Func);
        app.handle_key(k);
        assert_eq!(app.focus, Focus::Main);
    }

    #[test]
    fn func_input_saves_new_function() {
        let mut app = App::new();
        app.focus = Focus::Func;
        for c in "x+1".chars() {
            app.handle_key(key(KeyCode::Char(c)));
        }
        assert_eq!(app.func_input, "x+1");
        app.handle_key(key(KeyCode::Enter));
        assert_eq!(app.eval.funcs.len(), 1);
        assert_eq!(app.eval.funcs[0].name, "f_1");
        assert_eq!(app.eval.funcs[0].body, "x+1");
        assert!(app.func_input.is_empty(), "保存后输入框应清空");
        // 保存后立即在主输入中使用
        app.focus = Focus::Main;
        for c in "f_1(2)*3".chars() {
            app.handle_key(key(KeyCode::Char(c)));
        }
        assert_eq!(app.result, Some(Ok(Cmplx::real(9.0))));
    }

    #[test]
    fn func_input_rejects_empty_body() {
        let mut app = App::new();
        app.focus = Focus::Func;
        app.handle_key(key(KeyCode::Enter));
        assert!(app.eval.funcs.is_empty());
        assert!(matches!(app.result, Some(Err(_))), "空函数体应提示错误");
    }

    #[test]
    fn next_func_name_smallest_free() {
        let f1 = UserFunc {
            name: "f_1".into(),
            body: "x".into(),
        };
        let f3 = UserFunc {
            name: "f_3".into(),
            body: "x".into(),
        };
        assert_eq!(next_func_name(&[]), "f_1");
        assert_eq!(next_func_name(std::slice::from_ref(&f1)), "f_2");
        assert_eq!(
            next_func_name(&[f1.clone(), f3]),
            "f_2",
            "删 f_2 后新建应复用 f_2"
        );
        assert_eq!(
            next_func_name(&[
                f1,
                UserFunc {
                    name: "f_2".into(),
                    body: "x".into(),
                }
            ]),
            "f_3"
        );
    }

    #[test]
    fn delete_func_review_blocks_when_referenced() {
        let mut app = App::new();
        app.eval.funcs = vec![
            UserFunc {
                name: "f_1".into(),
                body: "f_2(x)+1".into(),
            },
            UserFunc {
                name: "f_2".into(),
                body: "x*2".into(),
            },
        ];
        app.main_key(key(KeyCode::F(2)));
        app.settings_key(key(KeyCode::Right)); // 切到自定义函数标签
        assert_eq!(app.set.tab, 1);
        app.settings_key(key(KeyCode::Down)); // 选中 f_2
        app.settings_key(key(KeyCode::Char('d')));
        assert_eq!(app.eval.funcs.len(), 2, "被引用时不应删除");
        let msg = app.set.msg.as_deref().unwrap_or("");
        assert!(msg.contains("f_1"), "应提示直接引用者 f_1, 实际: {msg}");
    }

    #[test]
    fn delete_func_review_lists_all_referrers() {
        let mut app = App::new();
        app.eval.funcs = vec![
            UserFunc {
                name: "f_1".into(),
                body: "f_3(x)+1".into(),
            },
            UserFunc {
                name: "f_2".into(),
                body: "f_3(x)*2".into(),
            },
            UserFunc {
                name: "f_3".into(),
                body: "x".into(),
            },
        ];
        app.main_key(key(KeyCode::F(2)));
        app.settings_key(key(KeyCode::Right));
        app.settings_key(key(KeyCode::Down));
        app.settings_key(key(KeyCode::Down));
        app.settings_key(key(KeyCode::Char('d')));
        let msg = app.set.msg.as_deref().unwrap_or("");
        assert!(
            msg.contains("f_1") && msg.contains("f_2"),
            "应列出全部直接引用者: {msg}"
        );
        assert_eq!(app.eval.funcs.len(), 3);
    }

    #[test]
    fn delete_func_removes_history_refs() {
        let mut app = App::new();
        app.eval.funcs = vec![
            UserFunc {
                name: "f_1".into(),
                body: "x+1".into(),
            },
            UserFunc {
                name: "f_2".into(),
                body: "x*2".into(),
            },
        ];
        app.history.push(HistoryEntry {
            expr: "f_2(3)".into(),
            result: "6".into(),
        });
        app.history.push(HistoryEntry {
            expr: "f_20(1)".into(),
            result: "x".into(),
        });
        app.history.push(HistoryEntry {
            expr: "f_1(2)".into(),
            result: "3".into(),
        });
        app.main_key(key(KeyCode::F(2)));
        app.settings_key(key(KeyCode::Right));
        app.settings_key(key(KeyCode::Down)); // 选中 f_2
        app.settings_key(key(KeyCode::Char('d')));
        assert_eq!(app.eval.funcs.len(), 1);
        assert_eq!(app.eval.funcs[0].name, "f_1");
        assert_eq!(app.history.len(), 2, "引用 f_2 的历史记录应被删除");
        assert_eq!(app.history[0].expr, "f_20(1)", "f_20 不应被误删");
        assert_eq!(app.history[1].expr, "f_1(2)");
    }

    #[test]
    fn func_edit_commit_in_settings() {
        let mut app = App::new();
        app.eval.funcs = vec![UserFunc {
            name: "f_1".into(),
            body: "x+1".into(),
        }];
        app.main_key(key(KeyCode::F(2)));
        app.settings_key(key(KeyCode::Right));
        app.settings_key(key(KeyCode::Enter)); // 进入内联编辑
        assert!(app.set.func_editing.is_some());
        app.settings_key(key(KeyCode::Backspace)); // 删掉 +1 的 1
        app.settings_key(key(KeyCode::Enter)); // 提交
        assert_eq!(app.eval.funcs[0].body, "x+");
        assert_eq!(app.set.msg.as_deref(), Some("已保存"));
        // 空函数体应被拦截
        app.settings_key(key(KeyCode::Enter));
        for _ in 0..3 {
            app.settings_key(key(KeyCode::Backspace));
        }
        app.settings_key(key(KeyCode::Enter));
        assert!(app.set.func_editing.is_some(), "空函数体应被拦截");
    }

    // ------------------------------------------------------------ 自动补全

    fn with_funcs(funcs: &[(&str, &str)]) -> App {
        let mut app = App::new();
        app.eval.funcs = funcs
            .iter()
            .map(|(n, b)| UserFunc {
                name: n.to_string(),
                body: b.to_string(),
            })
            .collect();
        app
    }

    #[test]
    fn autocomplete_candidates_filter() {
        let mut app = with_funcs(&[("f_1", "x+1"), ("f_2", "x*2"), ("f_10", "x^2")]);
        app.insert_char('f');
        let (start, cands) = app.autocomplete_state().expect("输入 f 应弹出候选");
        assert_eq!(start, 0);
        assert_eq!(cands, vec!["f_1", "f_2", "f_10"]);
        app.insert_char('_');
        app.insert_char('1');
        let (_, cands) = app.autocomplete_state().unwrap();
        assert_eq!(cands, vec!["f_1", "f_10"], "输入 f_1 应过滤掉 f_2");
        app.insert_char('(');
        assert!(app.autocomplete_state().is_none(), "词后跟 ( 应关闭下拉");
    }

    #[test]
    fn autocomplete_word_position() {
        let mut app = with_funcs(&[("f_1", "x+1")]);
        for c in "2+".chars() {
            app.insert_char(c);
        }
        assert!(app.autocomplete_state().is_none(), "未输入 f 不应弹出");
        app.insert_char('f');
        let (start, cands) = app.autocomplete_state().unwrap();
        assert_eq!(start, 2, "当前词应从 '+' 之后开始");
        assert_eq!(cands, vec!["f_1"]);
        // 非 f 开头词不弹出
        let mut app2 = with_funcs(&[("f_1", "x+1")]);
        app2.insert_char('x');
        assert!(app2.autocomplete_state().is_none());
        // 无自定义函数时不弹出
        let mut app3 = App::new();
        app3.insert_char('f');
        assert!(app3.autocomplete_state().is_none());
    }

    #[test]
    fn autocomplete_select_and_insert() {
        let mut app = with_funcs(&[("f_1", "x+1"), ("f_2", "x*2")]);
        app.insert_char('f');
        app.main_key(key(KeyCode::Down)); // 选中 f_2
        app.main_key(key(KeyCode::Enter)); // 插入
        assert_eq!(app.input, "f_2");
        assert_eq!(app.cursor, 3);
        assert!(app.history.is_empty(), "下拉打开时 Enter 不应求值");
        // 继续输入括号完成调用
        app.insert_char('(');
        app.insert_char('3');
        assert_eq!(app.input, "f_2(3");
        assert!(app.autocomplete_state().is_none());
        app.insert_char(')');
        assert_eq!(app.result, Some(Ok(Cmplx::real(6.0))));
    }

    #[test]
    fn autocomplete_inserts_at_word_start() {
        let mut app = with_funcs(&[("f_1", "x+1")]);
        for c in "1+f".chars() {
            app.insert_char(c);
        }
        app.main_key(key(KeyCode::Enter));
        assert_eq!(app.input, "1+f_1", "应只替换当前词 f");
        assert_eq!(app.cursor, 5);
    }

    #[test]
    fn autocomplete_in_func_body() {
        let mut app = with_funcs(&[("f_1", "x+1")]);
        app.focus = Focus::Func;
        for c in "x+f".chars() {
            app.handle_key(key(KeyCode::Char(c)));
        }
        let (start, cands) = app.autocomplete_state().unwrap();
        assert_eq!(start, 2);
        assert_eq!(cands, vec!["f_1"]);
        app.handle_key(key(KeyCode::Enter)); // 插入函数名而非保存函数
        assert_eq!(app.func_input, "x+f_1");
        assert_eq!(app.eval.funcs.len(), 1, "不应保存新函数");
    }

    #[test]
    fn autocomplete_esc_dismisses_until_input_changes() {
        let mut app = with_funcs(&[("f_1", "x+1")]);
        app.insert_char('f');
        app.main_key(key(KeyCode::Esc)); // 关闭下拉(不清空输入)
        assert_eq!(app.input, "f");
        assert!(app.auto.suppressed);
        app.insert_char('_'); // 输入变化后重新可用
        assert!(!app.auto.suppressed);
        let (_, cands) = app.autocomplete_state().unwrap();
        assert_eq!(cands, vec!["f_1"]);
    }
}
