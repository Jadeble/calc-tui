# calc-tui v0.1.0 — 终端科学计算器

基于 Rust + ratatui 的终端 TUI 科学计算器。左侧输入表达式、查看结果与历史记录,右侧实时显示帮助面板与组合按键说明。

## 功能

- **基本运算**: 加减乘除、乘方、取模、隐式乘法(`2pi`、`2(3+4)`、`2sin(x)`)、科学计数法(`2e3`)
- **函数**: 三角/反三角/双曲函数、`ln`/`log`/`logb(底,x)`/`log2`、`exp`、`sqrt`、阶乘 `n!`(支持 `(2+3)!`、`3!!`)
- **高级运算**: `Σ(k,1,10,k^2)` 求和、`∫(x,0,1,x^2)` 定积分、`deriv(x^2,x,2)` 求导,三者可任意嵌套
- **角度/弧度**: `Tab` 一键切换,DEG 模式下 `sin(30)=0.5`;`30°` 写法两种模式均正确
- **组合按键**: `\p`→π、`\s`→Σ(、`\i`→∫(、`\d`→deriv(、`\r`→√(、`\x`→×、`\v`→÷、`\e`→e,`F2` 可自由自定义

## 文件结构

| 路径 | 说明 |
|---|---|
| `src/` | 源码(Rust) |
| `src/main.rs` | 程序入口、终端初始化与事件循环 |
| `src/app.rs` | 应用状态管理与按键处理 |
| `src/math.rs` | 表达式预处理与求值引擎(Σ/∫/deriv/阶乘/角度切换) |
| `src/ui.rs` | TUI 界面渲染(主界面/结果历史/帮助面板/设置覆盖层) |
| `src/config.rs` | 组合按键配置的加载与持久化 |
| `.github/workflows/` | CI 检查与 Release 自动构建流水线 |
| `scripts/publish.sh` | 一键发布脚本 |
| `run.sh` | 默认终端启动脚本 |
| `README.md` | 中文项目文档 |
| `LICENSE-MIT` / `LICENSE-APACHE` | 双许可证 |

## 安装

从 Releases 下载对应平台的裸二进制(直接可运行,无需解压):

| 平台 | 文件名 |
|---|---|
| Linux x86_64(静态链接,不依赖 glibc) | `calc-tui-x86_64-linux-musl` |
| Linux ARM64(静态链接) | `calc-tui-aarch64-linux-musl` |
| macOS Apple Silicon | `calc-tui-aarch64-macos` |
| macOS Intel | `calc-tui-x86_64-macos` |
| Windows x86_64 | `calc-tui-x86_64-windows.exe` |

Linux 产物使用 musl 静态编译,不依赖系统 glibc 版本,在旧版本 Linux 系统上也可直接运行:

```bash
chmod +x calc-tui-x86_64-linux-musl
./calc-tui-x86_64-linux-musl
```

## 发布信息

- 版本: v0.1.0
- 发布者: JADE
- 日期: 2026.08.05
- 许可证: MIT OR Apache-2.0
