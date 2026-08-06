#!/usr/bin/env bash
# calc-tui 一键发布脚本
# 用法: ./scripts/publish.sh <GitHub用户名> [仓库名, 默认 calc-tui]
#
# 前置条件(在有 github.com 网络访问的机器上执行):
#   1. 安装并登录 gh CLI:  https://cli.github.com  (gh auth login)
#   2. 或已配置 SSH key / git 凭据
#
# 本脚本执行:
#   1. 构建 release 二进制(本地验证)
#   2. 写入 Cargo.toml 的 repository 字段
#   3. 提交并推送源码到 GitHub(仓库中只放源码, target/ 已被忽略)
#   4. 打 v0.1.0 标签推送, 触发 .github/workflows/release.yml 自动构建多平台二进制(musl 静态/Windows/macOS)并发布到 Releases
set -euo pipefail

USER="${1:?用法: ./scripts/publish.sh <GitHub用户名> [仓库名]}"
REPO="${2:-calc-tui}"
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VERSION="0.1.0"
TAG="v${VERSION}"
URL="https://github.com/${USER}/${REPO}"

cd "$DIR"

echo "==> 1/6 本地构建 + 测试"
cargo build --release
cargo test

echo "==> 2/6 写入 repository 字段: ${URL}"
if ! grep -q '^repository' Cargo.toml; then
    sed -i "s|^description = .*|description = \"终端 TUI 科学计算器: 三角函数/对数/阶乘/求和Σ/定积分/求导, 支持角度与弧度切换, 组合按键输入符号\"\nrepository = \"${URL}\"|" Cargo.toml
fi

echo "==> 3/6 提交源码"
git add -A
git diff --cached --quiet || git commit -m "release: v${VERSION} (发布者 JADE, 2026.08.05)"

echo "==> 4/6 创建 GitHub 仓库 ${USER}/${REPO}"
if ! gh repo view "${USER}/${REPO}" >/dev/null 2>&1; then
    gh repo create "${USER}/${REPO}" --public --source . --push
else
    git remote remove origin 2>/dev/null || true
    git remote add origin "https://github.com/${USER}/${REPO}.git"
    git push -u origin main
fi

echo "==> 5/6 打标签 ${TAG} 并推送(触发 Release 工作流)"
git tag -a "${TAG}" -m "calc-tui v${VERSION} — 发布者 JADE, 2026.08.05" || true
git push origin "${TAG}"

echo "==> 6/6 完成"
echo "   仓库:  ${URL}"
echo "   Release 将在 GitHub Actions 完成后自动生成(约 3~5 分钟): ${URL}/releases"
echo "   产物: 5 个平台二进制 (Linux musl 静态 x2 / Windows / macOS x2), 详见 README"
