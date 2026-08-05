# calc-tui — 终端科学计算器

一个运行在终端中的 TUI 科学计算器。启动后显示一个完整的计算器界面:
左侧为表达式输入、结果与历史记录,右侧为帮助面板与组合按键说明。

> 版本 v0.1.0  ·  发布者 JADE  ·  2026.08.05

## 功能特性

- **基本运算**: 加减乘除、乘方 `x^y`、取模、隐式乘法(`2pi`、`2(3+4)`、`2sin(x)` 自动补乘号,支持科学计数法 `2e3`)
- **函数**: 三角函数、反三角函数、双曲函数、`ln`/`log`/`logb(底,x)`/`log2`、`exp`、`sqrt`、阶乘 `n!`(支持 `(2+3)!`、`3!!`)
- **高级运算**: 求和 `Σ(k,1,10,k^2)`、定积分 `∫(x,0,1,x^2)`、求导 `deriv(x^2,x,2)`,三者可任意嵌套
- **角度/弧度切换**: `Tab` 一键切换,`30°` 写法两种模式均正确
- **组合按键输入**: 无法直接键入的符号用 `\p`、`\s` 等组合输入即替换,`F2` 进入设置界面可自由修改、添加、删除
- **表达式历史**: `↑`/`↓` 回看,`Enter` 计算

## 安装

### 方式一:下载 Release 二进制(Linux)

从 [Releases](https://github.com/Jadeble/calc-tui/releases) 下载 `calc-tui-x86_64-unknown-linux-gnu`(直接可运行,无需解压):

```bash
chmod +x calc-tui-x86_64-unknown-linux-gnu
./calc-tui-x86_64-unknown-linux-gnu
```

> macOS / Windows 用户请自行构建(见下),本仓库的 Release 仅提供 Linux 产物。

### 方式二:源码构建

需要 Rust 工具链(1.75+):

```bash
git clone https://github.com/Jadeble/calc-tui.git
cd calc-tui
cargo build --release
./target/release/calc-tui
```

也可以直接 `cargo run --release` 运行。项目附带的 `run.sh` 会在检测到的默认终端中启动计算器。

## 使用说明

| 按键 | 功能 |
|---|---|
| `Enter` | 计算 |
| `Tab` | 切换 角度(DEG) / 弧度(RAD) |
| `F2` | 组合按键设置 |
| `↑` / `↓` | 历史记录回看 |
| `Esc` | 输入非空时清空,为空时退出 |
| `Ctrl+C` | 强制退出 |

### 组合按键(输入即替换)

| 组合 | 插入 | 组合 | 插入 |
|---|---|---|---|
| `\p` | π | `\d` | deriv( |
| `\s` | Σ( | `\r` | √( |
| `\i` | ∫( | `\x` | × |
| `\v` | ÷ | `\e` | e |

按 `F2` 可编辑/添加/删除组合按键,修改后自动保存到 `~/.config/calc-tui/config.json`。

### 公式语法

- **函数**: `sin` `cos` `tan` `asin` `acos` `atan` `sinh` `cosh` `tanh` `ln` `log`(底 10) `logb(底,x)` `log2` `sqrt` `exp` `n!`
- **求和**: `Σ(变量, 起始, 结束, 表达式)` → `Σ(k,1,10,k^2)` = 385
- **定积分**: `∫(变量, 下限, 上限, 表达式)` → `∫(x,0,1,x^2)` ≈ 0.3333
- **求导**: `deriv(表达式, 变量, 取值点)` → `deriv(x^2,x,2)` = 4
- **嵌套**: `Σ(k,1,3,∫(x,0,k,x^2))` = 12
- **角度**: DEG 模式下 `sin(30)` = 0.5;两种模式下 `sin(30°)` 均 = 0.5
- 结果范围受双精度浮点限制(约 1.8×10³⁰⁸),`171!` 等溢出显示为 `∞`

## 技术栈

- [ratatui](https://github.com/ratatui/ratatui) — TUI 渲染
- [crossterm](https://github.com/crossterm-rs/crossterm) — 终端事件
- [meval](https://github.com/rekka/meval) — 表达式解析求值

## 许可证

双许可证: [MIT](LICENSE-MIT) OR [Apache-2.0](LICENSE-APACHE)

Copyright © 2026 JADE
