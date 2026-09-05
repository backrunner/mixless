#!/bin/bash
set -euo pipefail

usage() {
    cat <<'EOF'
用法：./dev.sh [--check | --build | --help]

  默认      增量编译并启动 Mixless 开发版本；Ctrl+C 停止
  --check   检查 Rust / Xcode / Metal 环境，不编译、不启动
  --build   只编译开发版本
  --help    显示帮助

可通过 DEVELOPER_DIR 指定 Xcode；未指定时自动查找。
EOF
}

mode="${1:-run}"
case "$mode" in
    --help|-h) usage; exit 0 ;;
    run|--check|--build) ;;
    *) usage >&2; exit 2 ;;
esac
if [ "$#" -gt 1 ]; then
    usage >&2
    exit 2
fi

fail() {
    printf '启动失败：%s\n' "$1" >&2
    exit 1
}

[ "$(uname -s)" = Darwin ] || fail "目前仅支持 macOS。"
project_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
cd -- "$project_dir"

# Finder/non-login shells may omit Rust and Homebrew from PATH. Preserve any
# toolchain explicitly selected by the caller ahead of these fallback paths.
export PATH="${PATH:-/usr/bin:/bin}"
for tool_dir in "${HOME}/.cargo/bin" /opt/homebrew/bin /usr/local/bin; do
    if [ -d "$tool_dir" ]; then
        PATH="$PATH:$tool_dir"
    fi
done
command -v cargo >/dev/null 2>&1 || fail "找不到 Cargo，请先通过 https://rustup.rs 安装 Rust。"
command -v rustc >/dev/null 2>&1 || fail "找不到 rustc，请安装或更新 Rust 工具链。"

has_metal() {
    [ -d "$1" ] && DEVELOPER_DIR="$1" /usr/bin/xcrun --find metal >/dev/null 2>&1
}

if [ -n "${DEVELOPER_DIR:-}" ]; then
    has_metal "$DEVELOPER_DIR" || fail "DEVELOPER_DIR 指定的 Xcode 无法提供 Metal 编译器，请检查 Xcode 安装。"
else
    selected_developer="$(/usr/bin/xcode-select -p 2>/dev/null || true)"
    for candidate in "$selected_developer" \
        /Applications/Xcode.app/Contents/Developer \
        /Applications/Xcode-beta.app/Contents/Developer \
        /Applications/Xcode*.app/Contents/Developer \
        "${HOME}"/Applications/Xcode*.app/Contents/Developer; do
        if has_metal "$candidate"; then
            export DEVELOPER_DIR="$candidate"
            break
        fi
    done
    [ -n "${DEVELOPER_DIR:-}" ] || fail "需要完整 Xcode 和 Metal 编译器；仅 Command Line Tools 不够。安装并首次打开 Xcode 后重试。"
fi
export DEVELOPER_DIR

printf '项目：%s\nXcode：%s\n' "$project_dir" "$DEVELOPER_DIR"
case "$mode" in
    --check)
        cargo --version
        rustc --version
        printf '开发启动环境已就绪。\n'
        ;;
    --build)
        exec cargo build --locked -p mixless-desktop --bin mixless
        ;;
    run)
        printf '正在启动开发版本，首次编译可能较久；Ctrl+C 停止。\n'
        exec cargo run --locked -p mixless-desktop --bin mixless
        ;;
esac
