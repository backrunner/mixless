# Mixless 文档索引

Mixless 是基于 GPUI 0.2 的本地 AI Mixing / AI DJ 桌面应用（**v1 只做 macOS**）：双唱盘、本地资料库与离线预分析、Spotify 歌单浏览、把歌单曲目获取成本地文件后再混。实时音频与分析在 Rust，原生 GPUI 只渲染 UI。

**一句话**：像 djay Pro 的双碟工作台；歌单走 Spotify，音频落到本地后再分析、预听、自动混。

## 如何阅读

按角色选入口。下表保留 Draft 0.3.1 设计背景；最新实现记录见下方。

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

## 当前实现记录

上表是早期设计资料，不能作为已实现功能或当前验收结果。原先的许可证、固定混音长度和设备默认值描述存在过时内容；当前代码、根目录 README 与 LICENSE 优先。

- [AutoMix 使用和工程参考](reference/AUTOMIX.md)
- [最新过渡改动与验证](history/2026-09-13-automix.md)
- [DJ 教学、分轨与音符模型调研](reference/automix-research.md)
- [Rust 分轨、音符与正式 AutoMix 接入](reference/native-inference.md)
- [独立声部实时播放与性能](reference/stem-playback.md)
- [DSP 与实时性能](reference/AUDIO_DSP.md)
- [自动更新链路检查与验证](history/2026-09-17-updater.md)
- [FX 目录](reference/FX_CATALOGUE.md)
- [对拍参考](reference/BEAT_SYNC.md)
- [SoundAnalysis 部署边界](reference/ANALYSIS_ENHANCEMENT.md)
- [历史 UI 验收](history/DJ_WORKSPACE_CHECKS.md)

根目录仅保留用户入口、贡献规范与许可证声明。AI 工作资料放在这里；已经被后续改动替代的重复逐轮报告已合并删除。历史结果按日期理解，不能当作当前版本重新验证过的结论。
