# Mixless 图标

当前图标在历史版本的连续波形 M 上增加了柔和的珐琅质感：保留圆润的字母轮廓、比例与黄橙渐变，通过细腻的边缘高光、浅倒角和轻微阴影增加立体感。两侧与中央连接延续双唱盘和音乐交接的含义。

使用 **replicate-cli / `openai/gpt-image-2.5-flare` / `medium` / `1:1`** 编辑原始 M 图片，输出为 1024 × 1024 PNG。原始图作为实际图像输入提交，完整参考路径、哈希与生成参数均已保存。当前使用的是细化后的位图；历史矢量轮廓保留在归档中。

| 文件 | 用途 |
| --- | --- |
| `m-source.png` | 模型输出的原始 PNG，供本地生成器读取 |
| `m-generation.json` | 编辑提示词及生成参数，不含内联图片数据 |
| `m-generation-metadata.json` | 模型、质量、时间、prediction ID、参考图路径及哈希 |
| `app-icon-1024.png` | 1024 × 1024、RGB、不透明、sRGB 的方形母版 |
| `icon-preview.png` | 实际 macOS 图标的 256／128／64／32／16 px 预览 |
| `archive/wave-m/` | 原始扁平 M 的 SVG、PNG 与预览；本次编辑的参考来源 |
| `archive/first-mixer/` | 第一版橙色混音台历史方案 |
| `archive/dual-deck/` | 已替换的双唱盘主图标、生成记录与设计对比 |
| `variants/` | 双唱盘和银色混音台的历史备选及提示词，应用不使用 |
| `../../apps/desktop/resources/Mixless.icns` | macOS Finder／Dock 图标 |
| `../../apps/desktop/resources/Assets.xcassets` | macOS AppIcon 的完整 1×／2× 尺寸槽位 |

## 本地导出与构建

以下导出读取已经保存的图片，不调用 API，也不产生图像生成费用：

```sh
swift scripts/generate-icons.swift
./dev.sh --build
python3 scripts/bundle-macos.py
```

生成器输出不裁角、不带 Alpha 的方形 sRGB 母版。macOS ICNS 与 asset catalog 另行使用圆角轮廓和透明外边距；标题栏、About 窗口与开发模式 Dock 都嵌入同一套生成资源。`icon-preview.png` 显示这些桌面图标的真实尺寸。

导出和构建要求 macOS／Xcode；打包脚本要求 Python 3.11+，输出本地未签名的 `target/app/Mixless.app`。修改母版后须重新导出并构建，应用下次启动时会加载新图标。

## 重新请求模型编辑

在项目根目录运行。需要已经配置好 Replicate 认证，会创建新的生成任务。将原始 M 放入 `input_images` 数组，复现本次使用的图像输入方式：

```sh
python3 - <<'PY'
from pathlib import Path
import base64
import json

brand = Path('assets/branding')
payload = json.loads((brand / 'm-generation.json').read_text())
reference = (brand / 'archive/wave-m/app-icon-1024.png').read_bytes()
payload['input_images'] = ['data:image/png;base64,' + base64.b64encode(reference).decode()]
output = Path('target/imagegen/refined-m-input.json')
output.parent.mkdir(parents=True, exist_ok=True)
output.write_text(json.dumps(payload))
PY
replicate run openai/gpt-image-2.5-flare \
  --input-json target/imagegen/refined-m-input.json \
  --output target/imagegen/refined-m \
  --json
```

相同提示词和参考图可能产生不同结果。下载的新图不会自动覆盖当前母版；选定后更新 `m-source.png` 和生成记录，再运行本地导出。
