#!/usr/bin/env bash
# 构建网页演示:编译 wasm → 复制 pkg → 复制自制 ROM → 生成清单
set -euo pipefail
cd "$(dirname "$0")/.."

# 只复制确认无版权问题的自制 ROM（NESdev 社区竞赛作品）。
# 列表外的 ROM（包括任何商业 ROM）一律不进 web 构建。
HOMEBREW_ROMS=(
  "Atmo Sphere.nes"
  "BlobQuest.nes"
  "Chocoblip_Alpha_0.1.nes"
  "FINes.nes"
  "Immunatio (nesdemia).nes"
  "Spacegulls-1.1.nes"
  "Upsad_down_v0_6.nes"
  "WolfSpirit.nes"
  "altercation-0.1.nes"
  "arkade-rush.nes"
  "bare-metal-6b2e249b.nes"
  "blobert.nes"
  "guntner(29).nes"
  "nogoodchoice.nes"
  "trouble_at_2a03-nesdev-submission-build.nes"
  "witchnwiz_2021_02_22_nes_dev_v_1_0_0.nes"
)

echo "==> 构建 nes-wasm (wasm32, release)"
wasm-pack build nes-wasm --target web --release

echo "==> 复制 pkg → web/pkg"
rm -rf web/pkg
cp -r nes-wasm/pkg web/pkg

echo "==> 复制自制 ROM → web/roms"
rm -rf web/roms
mkdir -p web/roms
copied=()
for f in "${HOMEBREW_ROMS[@]}"; do
  if [ -f "roms/$f" ]; then
    cp "roms/$f" web/roms/
    copied+=("$f")
  else
    echo "  跳过（不存在）: $f" >&2
  fi
done

echo "==> 生成 roms/roms.json"
manifest="["
sep=""
for f in "${copied[@]}"; do
  size=$(stat -f%z "web/roms/$f" 2>/dev/null || stat -c%s "web/roms/$f")
  manifest+="${sep}{\"file\":\"${f}\",\"size\":${size}}"
  sep=","
done
manifest+="]"
printf '%s' "$manifest" > web/roms/roms.json

echo ""
echo "完成 ✓  本地预览:"
echo "  python3 -m http.server -d web 8000"
echo "  打开 http://localhost:8000"
