#!/bin/bash
set -euo pipefail

usage() {
    cat <<'EOF'
用法：./dev.sh [--check | --build | --help]

  默认      增量编译并重启本仓库的开发实例；Ctrl+C 停止
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
# A local shell may retain the channel used for a release rehearsal. Local
# development must never download a public release over unshipped changes.
export MIXLESS_CHANNEL=dev

build_development() {
    cargo build --locked -p mixless-desktop --bin mixless
    build_root="${CARGO_TARGET_DIR:-$project_dir/target}"
    desktop_bin="$(cd -- "$build_root/debug" && pwd)/mixless"
    desktop_app="$project_dir/target/app/Mixless.app"
    cargo run --locked -p mixless-tools -- bundle "$desktop_bin" "$desktop_app" --channel dev
}

printf '项目：%s\nXcode：%s\n' "$project_dir" "$DEVELOPER_DIR"
case "$mode" in
    --check)
        cargo --version
        rustc --version
        printf '开发启动环境已就绪。\n'
        ;;
    --build)
        build_development
        ;;
    run)
        printf '正在编译开发版本；编译成功后重启本仓库的实例。\n'
        build_development
        # Match the executable mapping, not a shell command/substring. This
        # also recognizes ./target/debug/mixless and leaves other checkouts alone.
        for pid in $(/usr/bin/pgrep -x mixless || true); do
            same_binary=false
            while IFS= read -r entry; do
                if [ "$entry" = "n$desktop_bin" ] || [ "$entry" = "n$desktop_app/Contents/MacOS/mixless" ]; then same_binary=true; fi
            done < <(/usr/sbin/lsof -a -p "$pid" -d txt -Fn 2>/dev/null || true)
            if [ "$same_binary" = true ]; then
                printf '停止本仓库的旧实例：PID %s\n' "$pid"
                kill -TERM "$pid" 2>/dev/null || true
                for attempt in 1 2 3 4 5 6 7 8 9 10; do
                    kill -0 "$pid" 2>/dev/null || break
                    sleep 0.1
                done
                if kill -0 "$pid" 2>/dev/null; then
                    fail "旧实例尚未退出，请关闭其窗口后重试。"
                fi
            fi
        done
        printf '启动：%s\nCtrl+C 停止。\n' "$desktop_app"
        exec "$desktop_app/Contents/MacOS/mixless"
        ;;
esac
