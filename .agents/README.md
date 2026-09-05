# Mixless 文档索引

Mixless 是基于 Tauri 2 的本地 AI Mixing / AI DJ 桌面应用（**v1 只做 macOS**）：双唱盘、本地资料库与离线预分析、Spotify 歌单浏览、把歌单曲目获取成本地文件后再混。实时音频与分析在 Rust，WebView 只渲染 UI。

**一句话**：像 djay Pro 的双碟工作台；歌单走 Spotify，音频落到本地后再分析、预听、自动混。

## 如何阅读

按角色选入口。本文档与主设计文档（Draft **0.3.1**）必须保持一致。

| 顺序 | 文件 | 读什么 |
|------|------|--------|
| 1 | [00-requirements.md](./00-requirements.md) | 谁在用、验收、非目标、许可 |
| 2 | [01-architecture.md](./01-architecture.md) | 进程/线程、crate 边界、双输出、数据流 |
| 3 | [02-modules.md](./02-modules.md) | 各 crate 职责与公开接口 |
| 4 | [03-audio-pipeline.md](./03-audio-pipeline.md) | callback、stretch/jog/seek/FX、headphone cue |
| 5 | [04-analysis.md](./04-analysis.md) | metadata、BPM、调性、波形、结构、逐 bar |
| 6 | [05-ai-mixing.md](./05-ai-mixing.md) | cue 分轨约束、过渡目录、归一化评分、包络表 |
| 7 | [06-spotify.md](./06-spotify.md) | OAuth、浏览、本地匹配 |
| 8 | [09-acquire.md](./09-acquire.md) | 歌单 → 本地音频的渠道链（YTM / yt-dlp / 可选闭源） |
| 9 | [07-ui.md](./07-ui.md) | 布局、Hot Cue vs PFL、jog/SYNC/automix |
| 10 | [10-mixer-fx.md](./10-mixer-fx.md) | mixer 齐备、FX 目录、性能验收 |
| 11 | [08-roadmap.md](./08-roadmap.md) | 里程碑、PR、人力 |
| — | [design.md](./design.md) | 完整主设计（含 Key Decisions / PR Plan） |

## 已锁定、全文通用的决策

- 实时图 100% Rust + `cpal`。Web Audio 不是播放引擎。
- **v1 只验收 macOS**（CoreAudio）。Windows 不做。
- 主仓库 **Apache-2.0**。Spotify CDN 获取若实现，放闭源插件，不进开源树。
- Spotify = 歌单目录。音频通过获取层落到磁盘。默认渠道：本地匹配 → YouTube Music（spotDL/yt-dlp 路线）→ YouTube 回退。
- 官方 DJ Partner **不申请**。开源树不依赖 librespot / zotify。
- Time-stretch 默认 Signalsmith（音乐档 20–40 ms）；scratch 走 0–2 ms 重采样。
- v1 只有 2 个 deck。Master = 系统默认输出。**PFL 进 v1**（第二 CoreAudio）。无第二设备只禁 PFL，**Hot Cue 跳转永远可用**。
- Mixer / FX 按 [10-mixer-fx.md](./10-mixer-fx.md) 齐备；全开时 callback p99 < 50% block。
- 播放真相在 Rust；Svelte store 只镜像 `EngineSnapshot`。前端是 Svelte 5 + SCSS。
- v1 jog SLA：**p99 < 30 ms** @ 128 frames / scratch 路径。
- 只有 `kind=in|out` 硬约束 Automix；hot cue 是软锚。
- `mixplan` **不**依赖 `analyze`；`TrackAnalysis` 在 `protocol`。
- Watch-folder = P1；v1 仅显式导入 + 获取落盘。
- 图跑设备原生 SR，预分配按 `actual_sr`，上限 96 kHz。
- Loop 在 stretch 之前；callback 禁止 `read_at` / mmap。
- 2× 匹配 = 2:1 bar map、rate≈1；两个快 bar = 一个慢 bar。
- Mix 时钟 = 出碟；`PerformanceOffset` 带入下一对；打分用 sounding BPM/key。
- v1 有 SYNC。

## 仓库目标布局

```
apps/desktop/                 # Tauri 2 + Svelte 5 + SCSS（macOS）
crates/mixless-engine
crates/mixless-analyze
crates/mixless-mixplan
crates/mixless-library
crates/mixless-spotify        # 浏览 only
crates/mixless-acquire        # Resolver
crates/mixless-acquire-yt     # yt-dlp sidecar
crates/mixless-protocol
models/
third_party/                  # Signalsmith、yt-dlp binary
private/                      # 闭源 CDN 插件，不随开源发布
```

## 状态

- 日期：2026-08-15
- 仓库：greenfield
- 设计状态：Draft 0.3.1
