# Rust 原生分轨、音符与 AutoMix

2026-09-13 接入。代码入口：`crates/mixless-stems`、`crates/mixless-analyze/src/stems.rs`、`apps/desktop/src/analysis/deep.rs`。运行链路、预处理、推理编排、音符解码、缓存和打包均由 Rust 实现；不调用 Python/PyTorch/FFmpeg 子进程。

## 默认行为

桌面设置的 **Stem and note analysis for AutoMix** 默认打开。选择列表和导入后的准备流程先提供基础 BPM/key/结构与波形，再将分轨送入独立的后台队列；基础分析和分轨并发数由机器的可用 CPU 并行度自动分配。手动上碟和 AutoMix 不等待冷模型推理；新证据完成后刷新后续规划与列表预览，已经运行的过渡保持原计划。

列表在波形旁显示模型下载、分轨、音符分析进度。失败时保留基础分析和可播放状态，显示基础分析提示；右键 **Reanalyze** 可重试。关闭设置或退出应用会在块边界取消任务。文件内容发生变化或用户重新分析后，旧任务不能发布过时结果。

模型只在第一次使用时下载到应用数据目录的 `models/`；只发送公开模型下载请求，不上传音频。权重以固定版本 URL 和 SHA-256 校验，未完成/损坏的下载不能交给推理引擎。

## 模型与音频

- 分轨：[HTDemucs ONNX](https://huggingface.co/StemSplitio/htdemucs-onnx)，固定提交 `d54ed9eb60e258ea82131c6ee14578628816456a` 的 FP16 权重存储模型，float32 推理。输入 44.1 kHz、双声道、343,980 帧，输出 drums/bass/other/vocals；将 bass+other 合并为 instruments。
- 音符：[Spotify Basic Pitch v0.4.0](https://github.com/spotify/basic-pitch/tree/v0.4.0)，固定 ONNX 模型，对 vocals 和 instruments 分别推理。输入经抗混叠重采样为 22.05 kHz 单声道；带重叠的模型窗口按真实样本位置解包，输出源时间下的音高、起止时间和置信度。事件解码为原生 Rust 实现，不声称与 Python 后处理逐事件相同。
- 推理：[ort 2.0.0-rc.13](https://docs.rs/ort/2.0.0-rc.13/ort/) 驱动 ONNX Runtime 1.28。启动后统一检测可用 CPU 并行度，预留两个逻辑核的预算给播放和界面（后台预算至少为 1）；基础分析最多 8 路，分轨最多 2 路，每路模型默认 1–4 个推理线程。两类任务还共享进程级 CPU 配额，低核机器会交替执行；等待配额的分轨任务可取消。10 核机器默认是 4 路基础分析、2 路分轨、每路 2 个模型线程。此策略按机器核数分配，不是实时 CPU 占用率反馈调参。`MIXLESS_ORT_THREADS` 仍可手动覆盖模型线程数（1–16，可能超出自动预算），macOS 分轨工作线程设为 utility QoS。模型关闭等待忙轮询，避免空闲会话占用 CPU。当前 macOS 构建静态链接原生运行库，无 Python 或外部 ONNX 动态库搜索路径依赖。Rust 最低版本调整为 1.88。

分块采用 25% 重叠及权重归一化，首尾采样保持覆盖。三轨按相同源时间对齐，保存未单独归一化/裁剪的 float WAV。stem 之和不保证精确重构原曲，记录残差供评估；残差不是分轨音质评分。[实时声部路径](stem-playback.md) 将残差归入 instruments，保持全开时与原曲一致。

## 缓存与资源

`stem-cache/<hash>/` 的键由原曲内容 hash、模型 hash 和证据版本构成；三轨完整写入后再发布 JSON manifest。跨进程锁防止重复写入，损坏/不完整缓存不能标记为 Ready。缓存按最近使用淘汰，目标上限 6 GiB；写入前检查并保留 512 MiB 磁盘余量，只清理自己的分轨缓存。当前单曲深度分析上限 20 分钟。

SQLite 的 `TrackAnalysis.stems` 保存完整的轻量证据，`ANALYSIS_VERSION=15`。已有旧分析在正常准备时刷新；重启复用 SQLite，不重新推理。分轨任务按 CPU 预算并行处理不同曲目，同一内容通过跨进程锁合并；清理缓存仍需要独占锁。已上碟与即将播放的曲目提升到队首（`deep::promote`），队列空闲约 30 秒后释放模型会话。分析使用内存和 CPU，首次整轨推理不是实时操作。

开发/隔离验收可指定 `MIXLESS_MODEL_DIR`；`MIXLESS_MODELS_OFFLINE=1` 禁止模型下载；应用数据仍可用 `MIXLESS_DATA_DIR` 隔离。这些环境变量不是用户必需的安装步骤。

## AutoMix 实际使用的证据

- 分离的人声 RMS、频段和活动区间补充逐 bar 的人声概率，并定位更短的人声空隙。
- 高置信音符跨越切点时保护正在发声的音符；有明确编曲变化且一个新音符恰好开始时，可在开始处交接。没有检测到音符时仍保留混合频谱检查。
- 对整个重叠窗口比较实际移调后的音符 Chroma 进行，持续不相容会降低普通叠混评分。
- 分离鼓的低频变化和声部频段用于低频冲突、EQ/filter 与人声渐退决策；不把音符模型当成新的拍网格。

分析和规划已接入默认 AutoMix，2026-09-14 继续接入了[三声部增益和实时混合](stem-playback.md)。分轨泄漏、复音乐器错音和调性歧义仍需标注集/试听评估；不会把模型估计称为真实乐谱。

## 验证与原生工具

```sh
cargo run --locked -p mixless-stems -- song.wav target/models target/stems
cargo run --locked -p mixless-analyze --example deep-audit -- analyses.json paths.json target/models target/stems enhanced.json
cargo run --locked -p mixless-analyze --example render-audit -- enhanced.json paths.json target/renders
MIXLESS_MODEL_DIR=target/models MIXLESS_STEM_TEST_AUDIO=sample.wav cargo test --locked -p mixless-desktop native_models_reach_default_preparation_sql_cache_and_planner -- --ignored --nocapture --test-threads=1
./dev.sh --build
cargo run --locked -p mixless-tools -- bundle
```

结果与试听文件见 [本轮验证记录](../history/2026-09-13-native-models.md)。原 Python 分析原型、依赖清单和 Python 打包脚本已删除；旧的 MLX 原型测量只保留在历史记录中。
