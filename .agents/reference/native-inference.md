# Rust 原生分轨、音符与 AutoMix

2026-09-13 接入。代码入口：`crates/mixless-stems`、`crates/mixless-analyze/src/stems.rs`、`apps/desktop/src/analysis/deep.rs`。运行链路、预处理、推理编排、音符解码、缓存和打包均由 Rust 实现；不调用 Python/PyTorch/FFmpeg 子进程。

## 默认行为

桌面设置的 **Stem and note analysis for AutoMix** 默认打开。选择列表和导入后的准备流程先提供基础 BPM/key/结构与波形，再将分轨送入独立的后台队列；基础分析和分轨并发数由机器的可用 CPU 并行度自动分配。手动上碟和 AutoMix 不等待冷模型推理；新证据完成后刷新后续规划与列表预览，已经运行的过渡保持原计划。

列表在波形旁显示模型下载、分轨、音符分析进度。失败时保留基础分析和可播放状态，显示基础分析提示；右键 **Reanalyze** 可重试。关闭设置或退出应用会在块边界取消任务。文件内容发生变化或用户重新分析后，旧任务不能发布过时结果。

标准版在第一次使用时下载模型到应用数据目录的 `models/`；内置模型版通过 `mixless-tools bundle --models DIR` 将相同的校验后权重装入 `Contents/Resources/models/`，运行时只读加载，不在 App 内写锁或产生第二份下载缓存。清理下载缓存不删除内置权重。`MIXLESS_MODEL_DIR` 显式覆盖仍优先，方便隔离验证。只发送公开模型下载请求，不上传音频。权重以固定版本 URL 和 SHA-256 校验，未完成/损坏的下载不能交给推理引擎。

## 模型与音频

- 分轨：[HTDemucs ONNX](https://huggingface.co/StemSplitio/htdemucs-onnx)，固定提交 `d54ed9eb60e258ea82131c6ee14578628816456a` 的 FP16 权重存储模型，float32 推理。输入 44.1 kHz、双声道、343,980 帧，输出 drums/bass/other/vocals；将 bass+other 合并为 instruments。
- 音符：[Spotify Basic Pitch v0.4.0](https://github.com/spotify/basic-pitch/tree/v0.4.0)，固定 ONNX 模型，对 vocals 和 instruments 分别推理。输入经抗混叠重采样为 22.05 kHz 单声道；带重叠的模型窗口按真实样本位置解包，输出源时间下的音高、起止时间和置信度。事件解码为原生 Rust 实现，不声称与 Python 后处理逐事件相同。
- 推理：[ort 2.0.0-rc.13](https://docs.rs/ort/2.0.0-rc.13/ort/) 驱动 ONNX Runtime 1.29（支持时）或内置的 1.28。启动后统一检测可用 CPU 并行度，预留两个逻辑核的预算给播放和界面（后台预算至少为 1）；基础分析最多 8 路，分轨最多 2 路，每路模型默认 1–4 个推理线程。两类任务还共享进程级 CPU 配额，低核机器会交替执行；等待配额的分轨任务可取消。10 核机器默认是 4 路基础分析、2 路分轨、每路 2 个模型线程。此策略按机器核数分配，不是实时 CPU 占用率反馈调参。`MIXLESS_ORT_THREADS` 仍可手动覆盖模型线程数（1–16，可能超出自动预算），macOS 分轨工作线程设为 utility QoS。模型关闭等待忙轮询，避免空闲会话占用 CPU。macOS 构建保留静态运行库作为兜底，并在应用内附带新版 ORT/MLX；不依赖 Python 或系统动态库搜索路径。Rust 最低版本调整为 1.88。

### Apple Silicon MLX → CoreML → CPU

macOS arm64 默认按 **MLX → CoreML → CPU** 选择后端，每个模型独立处理。macOS 14+ 从 `Contents/Frameworks/` 延迟加载 ORT 1.29 与 MLX EP 0.29.6，Metal 内核放在 `Contents/Resources/inference-runtime/mlx.metallib` 并在注册插件前通过 mlx-c 设置路径；旧系统、库缺失或加载失败时使用内置 ORT 1.28 的 CoreML/CPU。整个进程在创建第一个 ORT tensor/session 前固定 API，不在已有值之间切换运行时。

HTDemucs 和 Basic Pitch 优先运行原始 ONNX；MLX 支持的子图在 MLX 执行，未支持的一维反卷积等算子由 ORT CPU 执行。每个模型 session 由固定 utility QoS 后台线程创建、运行、恢复和释放；上层队列仍可从不同线程复用同一个 `Inference`。线程间移动输入所有权，不复制整块 PCM。

降级到 CoreML 时，HTDemucs 使用 `MLProgram`、`CPUAndGPU`。固定模型的两个 iSTFT `ConvTranspose` 节点在内存中补充等价的 `output_shape=[351232]` 属性，使 ORT 1.28 将它们留在 CPU；其余受支持节点由 CoreML 执行。未经适配的模型在 M5 Max 上热推理约 24 s/7.8 s 片段，这两个算子是主要瓶颈。原始模型文件、权重、SHA-256、音频缓存身份与三声部接口保持一致。调整固定模型或 ORT 版本时必须重新验证此适配及 CPU/CoreML 数值一致性。

Basic Pitch 的 MLX 失败后也经过同一选择链；由于该固定图的 CoreML MLProgram 存在已验证的卷积 padding 编译错误，将 CoreML 判为不支持而转入 CPU。任何后端加载/编译失败时尝试下一项；运行出错、输出非有限值、缺失或形状异常时释放故障 session，以相同输入依次重试下一后端，成功降级后不会再提升。CPU 仍失败才报告分析错误。取消在等待和块边界传播，不触发降级；已进入的原生调用完成后才返回，避免遗留后台任务。其他受支持平台使用 CPU，Intel macOS 仍只播放已有缓存。进程级 native abort/崩溃不属于可捕获的 session 错误。

桌面编译缓存放在 `stem-cache/.coreml/`，按原始模型摘要、图适配版本、ORT 版本和后端配置隔离；编译锁避免多个会话/进程同时写同一缓存。不会写入 App 或 DMG。Storage 的全部分轨缓存清理包括这些编译产物；单曲清理保留它们。公开 `Inference::load` 诊断接口使用 `MODEL_DIR/.coreml/`；只读内置模型验证不保存 CoreML 编译缓存，并实际执行两个模型的合成输入；MLX/Metal 可能使用系统自身的缓存。

诊断可设 `MIXLESS_STEMS_BACKEND=auto|mlx|coreml|cpu`（默认 `auto`；`auto`/`mlx` 使用完整降级链，`coreml` 从 CoreML 开始，`cpu` 仅 CPU）。`coreml` 仍允许失败回退；调用 `Inference::backends()` 并检查 ORT profile 才能确认实际使用的后端。`MIXLESS_ORT_PROFILE_DIR=DIR` 输出各会话的算子执行 profile；MLX/CoreML EP 归属并不能单独证明 CoreML 内部每个算子实际运行在 GPU。首次编译与重复推理须分开计时。

运行时原生文件由 `mixless-tools prepare-inference-runtime DIR` 在开发/打包阶段下载并双重验证 archive/file SHA-256；已安装的应用不下载可执行代码。`./dev.sh` 自动准备，CI 将同一文件装入标准版及内置模型版，并先签署 dylib 再签署 app。使用裸 Cargo 前可运行：

```sh
rtk proxy cargo run --locked -p mixless-tools -- prepare-inference-runtime target/inference-runtime
```

Cargo 程序从 target 祖先目录查找 `inference-runtime/VERSION`；打包 app 仅查自身 Frameworks。`MIXLESS_INFERENCE_RUNTIME_DIR=DIR` 为显式诊断覆盖，指向包含 dylib 的版本目录；指向空目录可验证静态运行时回退。

```sh
MIXLESS_STEMS_BACKEND=cpu MIXLESS_ORT_THREADS=4 cargo run --release --locked -p mixless-stems --example benchmark -- song.wav target/models target/bench-cpu 60 30 3
MIXLESS_STEMS_BACKEND=coreml MIXLESS_ORT_THREADS=4 MIXLESS_ORT_PROFILE_DIR=target/bench-coreml/profiles cargo run --release --locked -p mixless-stems --example benchmark -- song.wav target/models target/bench-coreml 60 30 3
MIXLESS_TEST_BUNDLED_MODELS="$PWD/target/models" MIXLESS_STEM_TEST_AUDIO="$PWD/song.wav" cargo test --locked -p mixless-stems native_coreml_matches_cpu_and_unwritable_cache_falls_back -- --ignored --nocapture --test-threads=1
```

基准绕过已分轨音频缓存，记录加载、分轨、音符识别耗时和实际后端，保存最后一次三轨 WAV 与音符 JSON，供逐样本比较。设置 `MIXLESS_BENCH_REFERENCE_DIR=CPU_OUTPUT_DIR` 可在每次推理后检查全部采样与 CPU 参考的最大误差，超过 `1e-3` 即失败。初期性能与输出检查见 [CoreML 验证](../history/2026-09-22-coreml.md) 和 [MLX 调研](../history/2026-09-22-mlx-research.md)；接入后的验证见 [MLX 接入](../history/2026-09-22-mlx-integration.md)。

分块采用 25% 重叠及权重归一化，首尾采样保持覆盖。三轨按相同源时间对齐，保存未单独归一化/裁剪的 float WAV。stem 之和不保证精确重构原曲，记录残差供评估；残差不是分轨音质评分。[实时声部路径](stem-playback.md) 将残差归入 instruments，保持全开时与原曲一致。

## 缓存与资源

`stem-cache/<hash>/` 的键由原曲内容 hash、模型 hash 和证据版本构成；三轨完整写入后再发布 JSON manifest。跨进程锁防止重复写入，损坏/不完整缓存不能标记为 Ready。缓存按最近使用淘汰，目标上限 6 GiB；写入前检查并保留 512 MiB 磁盘余量，只清理自己的分轨缓存。当前单曲深度分析上限 20 分钟。

SQLite 的 `TrackAnalysis.stems` 保存完整的轻量证据，`ANALYSIS_VERSION=16`。已有旧分析在正常准备时刷新；重启复用 SQLite，不重新推理。分轨任务按 CPU 预算并行处理不同曲目，同一内容通过跨进程锁合并；清理缓存仍需要独占锁。已上碟与即将播放的曲目提升到队首（`deep::promote`），队列空闲约 30 秒后释放模型会话。分析使用内存和 CPU，首次整轨推理不是实时操作。

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
