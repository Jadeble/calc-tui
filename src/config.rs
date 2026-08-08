//! 配置(组合按键 + 自定义函数)的加载与持久化。

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::PathBuf;

/// 一个组合按键: 输入 keys(如 \s) 自动替换为 insert(如 Σ()。
/// preset 为 true 时是预设组合(π/Σ/∫ 等键盘无法直接输入的字符),
/// 可修改按键但不能删除。
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Combo {
    pub keys: String,
    pub insert: String,
    #[serde(default)]
    pub preset: bool,
}

/// 自定义函数: name 形如 f_1, body 为函数体文本(以 x 为参数)。
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct UserFunc {
    pub name: String,
    pub body: String,
}

/// 完整配置。
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Config {
    pub combos: Vec<Combo>,
    pub functions: Vec<UserFunc>,
}

/// 已废弃的组合按键(× ÷ e 直接用 * / e 输入, 旧配置中残留时过滤)。
const OBSOLETE_KEYS: &[&str] = &["\\e", "\\v", "\\x"];

/// 默认组合按键(全部为预设, 不可删除)。
pub fn default_combos() -> Vec<Combo> {
    vec![
        Combo {
            keys: "\\p".into(),
            insert: "π".into(),
            preset: true,
        },
        Combo {
            keys: "\\s".into(),
            insert: "Σ(".into(),
            preset: true,
        },
        Combo {
            keys: "\\i".into(),
            insert: "∫(".into(),
            preset: true,
        },
        Combo {
            keys: "\\d".into(),
            insert: "deriv(".into(),
            preset: true,
        },
        Combo {
            keys: "\\r".into(),
            insert: "√(".into(),
            preset: true,
        },
        Combo {
            keys: "\\f".into(),
            insert: "φ".into(),
            preset: true,
        },
    ]
}

/// 与默认预设相同的组合标记为预设(兼容旧配置与手动编辑后的加载)。
fn mark_presets(combos: &mut [Combo]) {
    for c in combos.iter_mut() {
        if default_combos()
            .iter()
            .any(|d| d.keys == c.keys && d.insert == c.insert)
        {
            c.preset = true;
        }
    }
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

fn valid_combo(c: &Combo) -> bool {
    !c.keys.is_empty()
        && c.keys.starts_with('\\')
        && c.keys.chars().count() <= 12
        && c.insert.chars().count() <= 24
        && !OBSOLETE_KEYS.contains(&c.keys.as_str())
}

fn valid_func(f: &UserFunc) -> bool {
    let name_ok = f
        .name
        .strip_prefix("f_")
        .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()));
    name_ok && !f.body.is_empty() && f.body.chars().count() <= 200
}

/// 读取配置; 文件不存在或损坏时使用默认值。
/// 兼容旧格式(裸组合按键数组)。
pub fn load_config() -> Config {
    let path = config_path();
    let raw = match fs::read_to_string(&path) {
        Ok(s) => s,
        Err(_) => {
            return Config {
                combos: default_combos(),
                functions: Vec::new(),
            };
        }
    };
    // 新格式: {"combos": [...], "functions": [...]}
    if let Ok(mut cfg) = serde_json::from_str::<Config>(&raw) {
        cfg.combos.retain(valid_combo);
        cfg.functions.retain(valid_func);
        // 函数名去重(保留首个)
        let mut seen = HashSet::new();
        cfg.functions.retain(|f| seen.insert(f.name.clone()));
        if cfg.combos.is_empty() {
            cfg.combos = default_combos();
        } else {
            mark_presets(&mut cfg.combos);
        }
        return cfg;
    }
    // 旧格式: 裸组合按键数组
    if let Ok(mut list) = serde_json::from_str::<Vec<Combo>>(&raw) {
        list.retain(valid_combo);
        if list.is_empty() {
            return Config {
                combos: default_combos(),
                functions: Vec::new(),
            };
        }
        mark_presets(&mut list);
        return Config {
            combos: list,
            functions: Vec::new(),
        };
    }
    Config {
        combos: default_combos(),
        functions: Vec::new(),
    }
}

/// 保存配置。
pub fn save_config(cfg: &Config) -> io::Result<()> {
    let path = config_path();
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let json = serde_json::to_string_pretty(cfg)?;
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
        let cfg = Config {
            combos: default_combos(),
            functions: vec![
                UserFunc {
                    name: "f_1".into(),
                    body: "x+1".into(),
                },
                UserFunc {
                    name: "f_2".into(),
                    body: "sin(x)^2".into(),
                },
            ],
        };
        let s = serde_json::to_string(&cfg).unwrap();
        let back: Config = serde_json::from_str(&s).unwrap();
        assert_eq!(cfg, back);
    }

    #[test]
    fn legacy_combo_array_parses() {
        // 旧配置文件是裸组合按键数组, 模拟 load_config 的回退路径
        let combos = default_combos();
        let s = serde_json::to_string(&combos).unwrap();
        let cfg = serde_json::from_str::<Config>(&s).unwrap_or_else(|_| {
            let list: Vec<Combo> = serde_json::from_str(&s).unwrap();
            Config {
                combos: list,
                functions: Vec::new(),
            }
        });
        assert_eq!(cfg.combos, combos);
        assert!(cfg.functions.is_empty());
    }

    #[test]
    fn invalid_funcs_filtered() {
        let cfg = Config {
            combos: default_combos(),
            functions: vec![
                UserFunc {
                    name: "f_1".into(),
                    body: "x+1".into(),
                },
                UserFunc {
                    name: "f_1".into(),
                    body: "x*2".into(),
                },
                UserFunc {
                    name: "bad".into(),
                    body: "x".into(),
                },
                UserFunc {
                    name: "f_3".into(),
                    body: "".into(),
                },
                UserFunc {
                    name: "f_10".into(),
                    body: "x".into(),
                },
            ],
        };
        let s = serde_json::to_string(&cfg).unwrap();
        let mut parsed: Config = serde_json::from_str(&s).unwrap();
        parsed.combos.retain(valid_combo);
        parsed.functions.retain(valid_func);
        let mut seen = HashSet::new();
        parsed.functions.retain(|f| seen.insert(f.name.clone()));
        assert_eq!(parsed.functions.len(), 2);
        assert_eq!(parsed.functions[0].name, "f_1");
        assert_eq!(parsed.functions[1].name, "f_10");
    }
}
