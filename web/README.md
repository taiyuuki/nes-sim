# nes-sim 网页演示

基于 [nes-wasm](../nes-wasm)(npm 包 `nes-wasm`)的纯静态网页模拟器演示,无需任何 JS 构建工具。

## 构建与运行

```sh
./web/build.sh                        # 编译 wasm + 复制 ROM + 生成清单
python3 -m http.server -d web 8000    # 本地静态服务器(直接开 html 文件无法 fetch wasm)
# 打开 http://localhost:8000
```

## 自动部署(GitHub Pages)

`.github/workflows/pages.yml` 在 push main 时自动构建并部署到
<https://taiyuuki.github.io/nes-sim/>。首次使用需在仓库
Settings → Pages → Build and deployment → Source 选择 **GitHub Actions**。

ROM 来源(可选):仓库变量 `ROMS_URL` 指向一个内含 `.nes` 的 zip
(建议把 `build.sh` 白名单内的 ROM 打包为 `homebrew-roms.zip` 上传到本仓库
Release,填入资产下载地址)。CI 会下载后由 `build.sh` 白名单过滤复制;
未设置时演示页不带预置卡带,拖拽本地 ROM 仍可游玩。

`build.sh` 做三件事:

1. `wasm-pack build nes-wasm --target web --release` 并复制 `nes-wasm/pkg` → `web/pkg`
2. 复制仓库根目录 `roms/` 下的自制 ROM(NESdev 社区竞赛作品,无版权问题)→ `web/roms/`
3. 生成 `web/roms/roms.json` 清单(浏览器无法列目录,页面靠它渲染卡带架)

`web/pkg`、`web/roms` 均被 gitignore,不入库。

## 功能

- 卡带架选游戏 / 本地文件打开 / 拖拽 `.nes` 进页面
- 音频时钟驱动时序(队列水位补帧),对 60/120/144Hz 显示器均按真实速度运行
- 键盘(方向键/WASD、Z=B、X=A、Enter=Start、Shift=Select、P=暂停、按住 Backspace 倒带)与 Gamepad API
- 触屏虚拟手柄:主指针为触屏的设备自动显示(8 向 D-pad 支持滑动换向、多指并发)
- 倒带:每帧快照环形缓冲(约 6 秒),按住即约 3× 速回退
- 暂停、复位、快照存档/读档(localStorage,按 ROM MD5 隔离)、静音、全屏
- FPS / 帧号 / ROM 校验和显示

## 目录

```
web/
├── index.html   页面结构(CRT + 卡带架 + 说明书)
├── style.css    1983 红白机风情样式
├── app.js       模拟器宿主逻辑(音频、输入、主循环)
├── build.sh     一键构建脚本
├── pkg/         wasm-pack 产物(生成,不入库)
└── roms/        自制 ROM + roms.json(生成,不入库)
```
