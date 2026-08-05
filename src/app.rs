//! 应用状态与按键处理。

use crate::config::{Combo, save_combos};
use crate::math::{Evaluator, fmt_result};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Clone, PartialEq, Copy, Debug)]
pub enum Screen {
    Main,
    Settings,
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
    pub result: Option<Result<f64, String>>,
    pub history: Vec<HistoryEntry>,
    pub hist_idx: Option<usize>,
    pub saved_input: String,
    // 配置
    pub combos: Vec<Combo>,
    // 设置界面
    pub set: SettingsState,
    pub quit: bool,
}

pub struct SettingsState {
    pub selected: usize,
    pub editing: Option<EditBuf>,
    pub msg: Option<String>,
}

pub struct EditBuf {
    pub keys: String,
    pub insert: String,
    pub focus_keys: bool, // true=编辑按键, false=编辑插入内容
}

impl App {
    pub fn new() -> Self {
        let combos = crate::config::load_combos();
        Self {
            screen: Screen::Main,
            input: String::new(),
            cursor: 0,
            combo_token: None,
            combo_token_start: 0,
            eval: Evaluator::new(false),
            result: None,
            history: Vec::new(),
            hist_idx: None,
            saved_input: String::new(),
            combos,
            set: SettingsState {
                selected: 0,
                editing: None,
                msg: None,
            },
            quit: false,
        }
    }

    /// 处理按键; 返回 true 表示退出。
    pub fn handle_key(&mut self, key: KeyEvent) -> bool {
        if key.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(key.code, KeyCode::Char('c' | 'C'))
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

    // ------------------------------------------------------------ 主界面

    fn main_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Enter => self.evaluate(),
            KeyCode::Esc => {
                // 输入非空 → 清空; 否则退出
                if !self.input.is_empty() {
                    self.input.clear();
                    self.cursor = 0;
                    self.combo_token = None;
                    self.result = None;
                    self.hist_idx = None;
                } else {
                    self.quit = true;
                }
            }
            KeyCode::Tab => {
                self.eval.degree = !self.eval.degree;
                self.combo_token = None;
            }
            KeyCode::F(2) => {
                self.screen = Screen::Settings;
                self.set.selected = 0;
                self.set.editing = None;
                self.set.msg = None;
            }
            KeyCode::Char(c) if !c.is_control() => self.insert_char(c),
            KeyCode::Backspace => {
                self.combo_token = None;
                char_remove_before(&mut self.input, &mut self.cursor);
            }
            KeyCode::Delete => {
                self.combo_token = None;
                char_remove_at(&mut self.input, &mut self.cursor);
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

    fn insert_char(&mut self, c: char) {
        if let Some(token) = self.combo_token.clone() {
            let new_token = format!("{token}{c}");
            // 精确匹配: 替换为预设内容
            if let Some(combo) = self.combos.iter().find(|x| x.keys == new_token) {
                let start = self.combo_token_start;
                let end = self.combo_token_start + char_len(&new_token);
                remove_range(&mut self.input, start, end);
                insert_at(&mut self.input, start, &combo.insert);
                self.cursor = start + char_len(&combo.insert);
                self.combo_token = None;
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
    }

    fn evaluate(&mut self) {
        let expr = self.input.trim().to_string();
        if expr.is_empty() {
            return;
        }
        self.combo_token = None;
        let r = self.eval.evaluate(&expr);
        if let Ok(v) = r {
            self.history.push(HistoryEntry {
                expr: expr.clone(),
                result: fmt_result(v),
            });
            if self.history.len() > 500 {
                self.history.remove(0);
            }
        }
        self.result = Some(r);
        self.input.clear();
        self.cursor = 0;
        self.hist_idx = None;
    }

    fn history_up(&mut self) {
        if self.history.is_empty() {
            return;
        }
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
        }
    }

    fn history_down(&mut self) {
        let Some(idx) = self.hist_idx else { return };
        if idx + 1 < self.history.len() {
            self.hist_idx = Some(idx + 1);
            self.input = self.history[idx + 1].expr.clone();
        } else {
            self.hist_idx = None;
            self.input = std::mem::take(&mut self.saved_input);
        }
        self.cursor = char_len(&self.input);
        self.combo_token = None;
    }

    // ------------------------------------------------------------ 设置界面

    fn settings_key(&mut self, key: KeyEvent) {
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
            KeyCode::Up => {
                if self.set.selected > 0 {
                    self.set.selected -= 1;
                }
            }
            KeyCode::Down => {
                if self.set.selected + 1 < self.combos.len() {
                    self.set.selected += 1;
                }
            }
            KeyCode::Enter => {
                if let Some(c) = self.combos.get(self.set.selected) {
                    self.set.editing = Some(EditBuf {
                        keys: c.keys.clone(),
                        insert: c.insert.clone(),
                        focus_keys: true,
                    });
                    self.set.msg = None;
                }
            }
            KeyCode::Char('a') => {
                self.combos.push(Combo {
                    keys: "\\z".into(),
                    insert: String::new(),
                });
                self.set.selected = self.combos.len() - 1;
                let _ = save_combos(&self.combos);
            }
            KeyCode::Char('d') => {
                if !self.combos.is_empty() {
                    self.combos
                        .remove(self.set.selected.min(self.combos.len() - 1));
                    if self.set.selected >= self.combos.len() && !self.combos.is_empty() {
                        self.set.selected = self.combos.len() - 1;
                    }
                    let _ = save_combos(&self.combos);
                }
            }
            KeyCode::Char('r') => {
                self.combos = crate::config::default_combos();
                self.set.selected = 0;
                let _ = save_combos(&self.combos);
            }
            _ => {}
        }
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
        let _ = save_combos(&self.combos);
    }
}

// ------------------------------------------------------------ 字符串工具

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
        app.main_key(key(KeyCode::Enter));
        assert!(app.input.is_empty(), "输入应被清空, 实际: {}", app.input);
        match &app.result {
            Some(Ok(v)) => assert!((*v - 14.0).abs() < 1e-9),
            other => panic!("结果应为 Ok(14), 实际: {other:?}"),
        }
        assert_eq!(app.history.len(), 1);
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
        app.main_key(key(KeyCode::Enter));
        match &app.result {
            Some(Ok(v)) => assert!((*v - 385.0).abs() < 1e-9, "实际: {v}"),
            other => panic!("Σ 结果错误: {other:?}"),
        }
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
    fn esc_clears_then_quits() {
        let mut app = App::new();
        for c in "sqrt(16)".chars() {
            app.insert_char(c);
        }
        app.main_key(key(KeyCode::Esc));
        assert!(!app.quit, "输入非空时 Esc 应清空而非退出");
        assert!(app.input.is_empty());
        app.main_key(key(KeyCode::Esc));
        assert!(app.quit, "输入为空时 Esc 应退出");
    }

    #[test]
    fn q_is_a_normal_char() {
        let mut app = App::new();
        for c in "sqrt(4)".chars() {
            app.insert_char(c);
        }
        assert_eq!(app.input, "sqrt(4)");
        assert!(!app.quit);
        app.main_key(key(KeyCode::Enter));
        match &app.result {
            Some(Ok(v)) => assert!((*v - 2.0).abs() < 1e-9),
            other => panic!("sqrt(4) 应为 2, 实际: {other:?}"),
        }
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
}
