#!/usr/bin/env bash
# 在默认终端中启动计算器 TUI。
# 用法: ./run.sh   (首次运行会自动构建 release 版本)
set -euo pipefail

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BIN="$DIR/target/release/calc-tui"

if [[ ! -x "$BIN" ]]; then
    echo "首次运行,正在构建 (cargo build --release)..."
    cargo build --release --manifest-path "$DIR/Cargo.toml"
fi

launch() {
    local term="$1"; shift
    "$term" "$@" "$BIN" &
}

if [[ -n "${TERMINAL:-}" ]] && command -v "$TERMINAL" >/dev/null 2>&1; then
    launch "$TERMINAL" -e
elif [[ -n "${WAYLAND_DISPLAY:-}" ]] && command -v foot >/dev/null 2>&1; then
    launch foot
elif command -v x-terminal-emulator >/dev/null 2>&1; then
    x-terminal-emulator -e "$BIN" &
elif command -v gnome-terminal >/dev/null 2>&1; then
    gnome-terminal -- "$BIN" &
elif command -v konsole >/dev/null 2>&1; then
    konsole -e "$BIN" &
elif command -v xfce4-terminal >/dev/null 2>&1; then
    xfce4-terminal -e "$BIN" &
elif command -v kitty >/dev/null 2>&1; then
    kitty "$BIN" &
elif command -v alacritty >/dev/null 2>&1; then
    alacritty -e "$BIN" &
elif command -v wezterm >/dev/null 2>&1; then
    wezterm start -- "$BIN" &
else
    exec "$BIN"
fi
