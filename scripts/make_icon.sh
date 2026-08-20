#!/usr/bin/env bash
# 从 docs/icon/raw_icon.png 生成应用图标源图：居中裁方 + 缩放 1024×1024
# 产物 frontend/src/assets/icon.png，兼作 UI 侧栏图标与 cargo tauri icon 输入源。
# 依赖：macOS 自带 sips（无第三方依赖；Linux 可用 imagemagick convert 等价替换）
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RAW="$ROOT/docs/icon/raw_icon.png"
OUT="$ROOT/frontend/src/assets/icon.png"
SIZE=1024

[[ -f "$RAW" ]] || { echo "原图不存在: $RAW" >&2; exit 1; }

W=$(sips -g pixelWidth "$RAW" | awk '/pixelWidth/{print $2}')
H=$(sips -g pixelHeight "$RAW" | awk '/pixelHeight/{print $2}')
SIDE=$(( W < H ? W : H ))
echo "原图 ${W}x${H} → 居中裁方 ${SIDE}x${SIDE} → 缩放 ${SIZE}x${SIZE}"

TMP=$(mktemp "${TMPDIR:-/tmp}/icon.XXXXXX.png")
trap 'rm -f "$TMP"' EXIT

# 已是正方形时裁切为 no-op，统一走同一路径
sips -c "$SIDE" "$SIDE" "$RAW" --out "$TMP" >/dev/null
sips -z "$SIZE" "$SIZE" "$TMP" --out "$OUT" >/dev/null

echo "已生成 $OUT"
# 生成全平台图标集（src-tauri/icons/）：
#   cargo tauri icon "$OUT"
