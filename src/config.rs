//! 组合按键(符号输入)配置的加载与持久化。

use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::PathBuf;

/// 一个组合按键: 输入 keys(如 \s) 自动替换为 insert(如 Σ()。
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Combo {
    pub keys: String,
    pub insert: String,
}

/// 默认组合按键。
pub fn default_combos() -> Vec<Combo> {
    vec![
        Combo {
            keys: "\\p".into(),
            insert: "π".into(),
        },
        Combo {
            keys: "\\s".into(),
            insert: "Σ(".into(),
        },
        Combo {
            keys: "\\i".into(),
            insert: "∫(".into(),
        },
        Combo {
            keys: "\\d".into(),
            insert: "deriv(".into(),
        },
        Combo {
            keys: "\\r".into(),
            insert: "√(".into(),
        },
        Combo {
            keys: "\\x".into(),
            insert: "×".into(),
        },
        Combo {
            keys: "\\v".into(),
            insert: "÷".into(),
        },
        Combo {
            keys: "\\e".into(),
            insert: "e".into(),
        },
    ]
}

fn config_path() -> PathBuf {
    if let Ok(x) = std::env::var("XDG_CONFIG_HOME") {
        PathBuf::from(x).join("calc-tui").join("config.json")
    } else {
        std::env::var("HOME")
            .map(|h| {
                PathBuf::from(h)
                    .join(".config")
                    .join("calc-tui")
                    .join("config.json")
            })
            .unwrap_or_else(|_| PathBuf::from("calc-tui-config.json"))
    }
}

/// 读取配置; 文件不存在或损坏时使用默认值。过滤非法条目(空按键等)。
pub fn load_combos() -> Vec<Combo> {
    let valid = |c: &Combo| {
        !c.keys.is_empty()
            && c.keys.starts_with('\\')
            && c.keys.chars().count() <= 12
            && c.insert.chars().count() <= 24
    };
    let path = config_path();
    match fs::read_to_string(&path) {
        Ok(s) => match serde_json::from_str::<Vec<Combo>>(&s) {
            Ok(list) => {
                let list: Vec<Combo> = list.into_iter().filter(valid).collect();
                if list.is_empty() {
                    default_combos()
                } else {
                    list
                }
            }
            _ => default_combos(),
        },
        Err(_) => default_combos(),
    }
}

/// 保存配置。
pub fn save_combos(combos: &[Combo]) -> io::Result<()> {
    let path = config_path();
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let json = serde_json::to_string_pretty(combos)?;
    fs::write(path, json)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_not_empty() {
        assert!(!default_combos().is_empty());
    }

    #[test]
    fn json_roundtrip() {
        let c = default_combos();
        let s = serde_json::to_string(&c).unwrap();
        let back: Vec<Combo> = serde_json::from_str(&s).unwrap();
        assert_eq!(c, back);
    }
}
