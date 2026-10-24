#!/usr/bin/env bash
# 构建网页演示:编译 wasm → 复制 pkg → 复制自制 ROM → 生成清单
set -euo pipefail
cd "$(dirname "$0")/.."

echo "==> 构建 nes-wasm (wasm32, release)"
wasm-pack build nes-wasm --target web --release

echo "==> 复制 pkg → web/pkg"
rm -rf web/pkg
cp -r nes-wasm/pkg web/pkg

echo "==> 复制自制 ROM → web/roms"
mkdir -p web/roms
cp roms/*.nes web/roms/

echo "==> 生成 roms/roms.json"
manifest="["
sep=""
for f in web/roms/*.nes; do
  file=$(basename "$f")
  size=$(stat -f%z "$f" 2>/dev/null || stat -c%s "$f")
  manifest+="${sep}{\"file\":\"${file}\",\"size\":${size}}"
  sep=","
done
manifest+="]"
printf '%s' "$manifest" > web/roms/roms.json

echo ""
echo "完成 ✓  本地预览:"
echo "  python3 -m http.server -d web 8000"
echo "  打开 http://localhost:8000"
