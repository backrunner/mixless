# ONNX MLX 默认后端与自动降级

2026-09-22，Apple M5 Max、18 逻辑核、64 GiB，macOS 27 arm64。实现入口为 `crates/mixless-stems/{src/runtime.rs,src/session.rs,src/worker.rs}`，当前行为见 [原生推理参考](../reference/native-inference.md)。前置可行性和独立热推理测量见 [MLX 调研](2026-09-22-mlx-research.md)。

## 实现

- macOS 14+ Apple Silicon 默认使用 ONNX Runtime 1.29.0 + MLX EP 0.29.6；保留静态 ORT 1.28。首次创建任何 ORT tensor/session 前选定进程级 API，动态库不可用时保留静态运行时，不在现有 session/value 间切换运行时。
- 两个模型分别按 MLX → CoreML → CPU 加载。加载错误、执行错误、输出缺失、形状异常或非有限输出会释放故障 session，以同一个输入尝试下一后端；成功降级后不会在该 session 中重新提升。Basic Pitch 的固定图已确认不兼容 CoreML MLProgram padding 转换，因此 MLX 失败后跳过不可用的 CoreML，进入 CPU。
- 每个模型有固定 utility QoS 线程，负责 session 创建、执行、恢复及析构，满足当前 MLX EP 的线程约束。上层 Processor 可在不同调用线程之间复用；输入所有权通过有界通道移动。取消不触发降级，等待正在执行的原生调用完成后返回，析构回收工作线程。
- 原始 ONNX、权重校验和三声部接口不变。CoreML 使用已有 HTDemucs iSTFT 图适配，并按运行时版本隔离编译缓存。
- Rust 打包工具在构建阶段下载固定 wheel，验证 archive 和各原生文件 SHA-256，只提取原生文件；应用不需要 Python，也不在用户运行时下载可执行代码。标准版和内置模型版均携带运行时，增加约 186 MiB 未压缩文件。
- 四个 dylib 位于 `Contents/Frameworks/`；Metal 内核位于 `Contents/Resources/inference-runtime/mlx.metallib`，通过 mlx-c 的路径 API 在插件初始化前设置。发布流程先用同一 Developer ID 签署 dylib，再签署开启 Hardened Runtime 的 app。
- `MIXLESS_STEMS_BACKEND=auto|mlx|coreml|cpu` 支持诊断；`MIXLESS_INFERENCE_RUNTIME_DIR` 支持独立的运行库缺失测试。正常启动日志包含实际后端。`--check-bundled-models` 实际执行两个模型的合成输入，不能仅以 session 加载成功作为通过依据。

## 验证结果

产物和原始日志位于 `target/mlx-integration/`，常规开发包为 `target/app/Mixless.app`。

| 检查 | 结果 |
| --- | --- |
| stems/tools 全目标 `cargo check --locked` | 通过 |
| desktop/stems/tools 常规测试 | 105 通过，7 个原生/集成测试默认忽略，0 失败；`tests.log` |
| 实际 MLX → CoreML → CPU 恢复 | 通过；注入失效 session 后依次实际执行下一后端，并比较 CPU 输出；`native.log` |
| 固定线程、跨调用线程复用、取消后复用 | 通过；`native.log`，最终 Metal 路径改动后再次通过 `final-checks.log` |
| CoreML 图适配数值一致性、缓存不可写回退 | 通过；`native.log` |
| 清理下载缓存后的内置模型离线分析 | 通过，两个模型实际使用 MLX；`offline.log` |
| 插件缺失 / 整个可选运行时缺失 | 分别使用 ORT 1.29 / 静态 1.28，实际完成 CoreML 分轨与 CPU 音符分析；`no-plugin.log`、`no-runtime.log` |
| 默认准备队列 → SQLite → 分轨缓存 → AutoMix 规划和播放渲染 | 实际模型集成通过；`playback.log` |
| Intel macOS stems 全目标交叉编译 | 通过；Intel 产品行为仍为播放已有缓存 |
| `./dev.sh --build` | 通过；`build.log` |
| app 签名结构及独立工作目录下实际 MLX 执行 | `codesign --verify --deep --strict` 通过，`/tmp` 下执行内置模型包通过；`package.log`、`bundled.log` |

恢复测试单块输出与 CPU 的最大绝对差：MLX `0.000119015574`，CoreML `0.0001181066`，均低于 `1e-3` 验证阈值。取消不改变后端，失败后没有重新提升；Basic Pitch 从失效 MLX 到 CPU 的实际输出比较通过。

真实音频使用 release Rust 全链路，每个片段 CPU/默认后端各运行三次，包含重采样、重叠分块、分轨合成与音符解码：

| 30 秒片段 | 默认后端 | 三次输出最大绝对采样差 / CPU | 音符数量 |
| --- | --- | --- | --- |
| In The Echo，145–175 s | 两模型 `mlx+cpu` | `0.0002059340476989746` | 357，与 CPU 相同 |
| Lost & Found，100–130 s | 两模型 `mlx+cpu` | `0.0001843571662902832` | 170，与 CPU 相同 |

音符音高顺序一致，少量事件边界变化，最大为 11.610 ms；不声称逐事件完全相同。ORT profile 确认 HTDemucs 使用 5 个 MLX 子图、6 个 CPU ConvTranspose 节点，Basic Pitch 使用 1 个 MLX 子图。打包后两图合计 MLX claim 1692/1698 节点（99.6%），日志确认绑定 GPU。

真实推理并发时，离线播放回调 40,800 块的 p99 为 0.326 ms；双声部分轨 deck 加 keylock/pitch 与新推理并发时，1,112 块 p99 为 0.435 ms，均低于 2.667 ms 回调期限。这是离线渲染回调证据，未覆盖物理声卡、界面验收或主观听感。

## 打包发现与证据边界

普通版本子目录放入 `Frameworks` 会被 codesign 当成嵌套代码 bundle，Metal 数据文件放入 `Frameworks` 也不满足签名结构要求；最终布局已纠正，并实际执行验证了 Resources 中的 Metal 内核。

本地 ad-hoc 签名没有 Team ID，开启 Hardened Runtime 时动态库会因 Team ID 校验被拒绝。该情况下应用实际完成了静态 CoreML/CPU 降级（`hardened-adhoc-fallback.log`）。本地 MLX 打包验收使用常规、不启用 Hardened Runtime 的 ad-hoc 开发签名；生产配置保持 Hardened Runtime 和同一 Developer ID 的嵌套签名，未添加关闭库校验的 entitlement。本轮未执行 Developer ID 签名、公证或正式发布。

本轮共享机器有明显并发 CPU/GPU 和内存负载，CPU 和 MLX 多次端到端耗时波动较大，不据此给出新版本加速倍数。前置调研的独立热推理基准可说明可行性，但不能替代当前集成在空闲机器上的持续性能测量。

普通 Clippy 完成，但严格 `-D warnings` 未通过已有告警（protocol 大枚举、stems 的循环及 chunks/is_multiple_of 风格建议）；没有宣称严格静态检查全绿。进程级 native abort、崩溃或永久挂起无法由 session 错误恢复机制捕获。本轮实际 GPU 运行验证覆盖 M5 Max，不代表所有 Apple Silicon 设备均已实测。
