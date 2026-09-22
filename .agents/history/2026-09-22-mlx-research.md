# ONNX → MLX 调研与本机实测

2026-09-22。结论：`onnxruntime-ep-mlx` 已能直接运行 Mixless 现有 HTDemucs 和 Basic Pitch ONNX，在本机热推理中优于 CPU，HTDemucs 也优于当前 CoreML 图适配。适合作为下一个实验后端；本次没有更改生产后端、依赖或默认选择。

## 路线与版本

- [onnxruntime/onnxruntime-ep-mlx](https://github.com/onnxruntime/onnxruntime-ep-mlx)：独立 Rust `cdylib` 插件；ORT 将支持的 ONNX 子图交给 MLX 编译、执行，未支持节点保留在 ORT CPU。保留现有 ONNX 权重和 ORT 调用方式，不需要手工重写 HTDemucs，也不需要在应用里部署 Python。
- 阅读源码快照 `b5efa5823dcc56d112734d046685e1f41c80d3e1`；实际测试的是发布的 PyPI `onnxruntime-ep-mlx==0.29.6`，不是从 main 编译。main 已有后续 IR 重构，因此两者不能当成同一构建。
- 插件请求 `ORT_API_VERSION >= 29`，本次配 `onnxruntime==1.29.0`。Mixless 当前静态 ORT 为 1.28，不能直接加载这个插件。Rust `ort 2.0.0-rc.13` 本身已提供 `Environment::register_ep_library`、`devices` 和 `SessionBuilder::with_devices`，不必为此改成 Python，但仍需升级/重新打包底层 ORT 并验证 ABI、构建和 CoreML 回归。
- 包元数据仍标为 Alpha；README 的其他模型性能不等于 HTDemucs 性能，也不能替代我们的模型验证。

## 实测方法

Apple M5 Max、18 logical CPUs、64 GiB，macOS 27 / Darwin 27 arm64。隔离环境 `target/mlx-research/venv`，Python 3.11.16、ORT 1.29.0、MLX EP 0.29.6。没有修改系统 Python。

使用仓库固定 `target/htdemucs-fp16.onnx` 与 `target/basic-pitch.onnx`。主对照输入取自 In The Echo 的 145 s 处真实音频：HTDemucs 输入 `[1,2,343980]`、44.1 kHz、约 7.8 s，并归一化；Basic Pitch 输入 `[1,43844,1]`，由真实音乐混音降采样到 22.05 kHz。后者测试张量推理，不是实际分轨后的音符解码。

所有后端使用 ORT 1.29.0、4 个 intra-op 线程、1 个 inter-op 线程、顺序执行、关闭线程 spinning、Level3 图优化；串行运行各组。CPU 和 MLX 使用原始 ONNX；CoreML 使用应用现有的两个 iSTFT `output_shape` 等价适配、MLProgram、CPUAndGPU。关闭 Python ORT 的整 session 自动 fallback，并检查实际 provider 与 profile，避免把注册成功当作加速成功。

每组连续运行 12 次，去掉首轮，报告其余 11 次的中位数。计时包含 `session.run` 输出回到 CPU，不包含 session 加载、音频解码、分块重叠相加、音符后处理或磁盘缓存写入；这不是完整歌曲或桌面队列 benchmark。所有组开启 ORT profiling，数字只代表这台机器上的本次测量。

| 模型 | ORT CPU | ORT CoreML + CPU | ORT MLX + CPU | MLX 相对 CPU |
| --- | ---: | ---: | ---: | ---: |
| HTDemucs，7.8 s 输入 | 790 ms | 360 ms | 236 ms | 3.34× |
| Basic Pitch，约 2 s 输入 | 8.33 ms | 未测试 | 1.98 ms | 4.21× |

HTDemucs 的 MLX 热推理相对 CoreML 为 1.52×。Basic Pitch 各轮存在 GPU 升频/调度波动，MLX 热运行范围约 1.24–5.85 ms，不将中位数表述为每次固定耗时。

首次在此环境加载/运行时，HTDemucs MLX session 加载 1.286 s，首轮 Run 10.021 s；Basic Pitch 首轮 Run 1.402 s。同一环境后续新进程中，HTDemucs 加载 1.053 s、首轮 Run 0.328 s，Basic Pitch 首轮 0.019 s。这说明启动状态会显著影响延迟；没有清除系统 Metal 缓存来量化所有冷启动场景，也没有验证持久编译缓存契约。CoreML 本次全新独立缓存加载 22.687 s，缓存重载 1.204 s。不能从热运行比值推算首次整曲总耗时。

## 实际分配与数值

- MLX 插件报告 HTDemucs 接管 1446/1452 个分区前节点（99.6%），形成 5 个 MLX 子图。ORT profile 确认执行 5 个 MLX 子图及 6 个 CPU `ConvTranspose`。Basic Pitch 接管 246/246 个节点，profile 为 1 个 MLX 子图，无 CPU 节点。
- 6 个 CPU 节点为两个 `/real_istft/ConvTranspose*` 和四个 `/tdecoder.{0..3}/conv_tr/ConvTranspose`。插件当前只接管符合约束的二维反卷积，这些一维形式不支持。MLX EP 不需要 CoreML 使用的模型适配就能自然避开它们。
- HTDemucs 热 profile 的 CPU 节点平均约 143 ms，其中两个 iSTFT 合计约 100 ms；MLX 子图平均约 99 ms。因此“接管 99.6% 节点”不代表 99.6% 的耗时在 GPU。后续优化应关注反卷积/频谱实现、分区与复制开销。我们的 ONNX 用 Conv/ConvTranspose 表示 STFT/iSTFT，插件支持 DFT/STFT 不会自动把这些卷积变成 FFT。
- 主对照所有输出有限、形状一致。MLX 相对同版本 CPU，归一化四声部输出最大绝对差 `4.67e-4`，RMS 差 `3.37e-5`；Basic Pitch 三个输出最大绝对差 `1.85e-6`。重复同一输入时，本轮 MLX 输出相同；不据此承诺跨设备/版本的确定性。
- 第二个独立输入：Lost & Found，100 s 起；CPU/MLX 各运行 4 次。HTDemucs 最大绝对差 `4.54e-4`，Basic Pitch 最大绝对差 `7.63e-6`，全部有限。该轮耗时波动明显，仅用作数值交叉检查，不纳入上表性能结论。
- 未做整曲 overlap-add、解码音符事件对齐、试听、并发播放或低内存机器验收。ORT profile 和插件 GPU device 绑定证明 MLX EP 实际执行；未单独做每个 Metal kernel 的 Instruments 能耗/占用率测量。

## 正式接入的具体工作

1. 将底层 ORT 升至兼容 API 29 的版本，并保留 CoreML/CPU 的构建、离线模型、打包与回退验证。不能只在现有 ORT 1.28 session 上追加插件。
2. 为 MLX 使用固定线程持有的 session。当前 `Processor` 的 `Vec<Mutex<State>>` 允许不同调用线程轮流取得同一个 session；mutex 只保证互斥，不能满足线程绑定。已对发布 wheel 实测：第一次在主线程 Run 后，即使串行地换到另一个线程 Run，也会返回 `EP_FAIL: MLX eval is thread-affine`。可采用专用 worker 线程通过队列接受任务。
3. 将插件、`libmlx.dylib`、`libmlxc.dylib`、`mlx.metallib` 放入应用并处理定位、签名与公证。此 wheel 的四个原生文件合计约 154.6 MiB；现成 dylib 的 `LC_BUILD_VERSION minos` 均为 14.0、仅 arm64，而当前应用声明 macOS 12.0。MLX 应按系统/架构可用性加载，旧系统继续 CoreML/CPU；不能据此默默提高整个应用最低系统版本。
4. 初期作为显式实验选项，例如 `MIXLESS_STEMS_BACKEND=mlx`（尚未实现），继续保留已落地的 CoreML 默认策略。验收应覆盖多个真实歌曲、首次/缓存启动、整曲总耗时、输出和音符差异、取消及出错回退、后台并发播放、低内存 Mac。通过后再决定是否提高 MLX 优先级。

## 其他路线

| 项目 | 实际能力 | 对当前需求的判断 |
| --- | --- | --- |
| [Meshona-ai/tnnx](https://github.com/Meshona-ai/tnnx) | ONNX → 生成 `model_mlx.py` + `weights.npz`，也支持 JAX；当前要求 Python 3.14 | 是另一条真正的 ONNX → MLX 路线。不过[算子表](https://github.com/Meshona-ai/tnnx/blob/main/docs/operators.md)的 70 个 ONNX 映射中没有 `ConvTranspose`，不能原样转换我们的 HTDemucs；还会增加独立生成模型和 Python/原生桥接的维护工作。本次未运行。 |
| [ml-explore/mlx-onnx](https://github.com/ml-explore/mlx-onnx) | 当前仓库只有 README、LICENSE、gitignore | 没有可用转换器或推理实现。 |
| [skryl/mlx-onnx](https://github.com/skryl/mlx-onnx) | MLX → ONNX 导出 | 方向相反，不解决现有 ONNX 在 MLX 上执行。 |
| [ssmall256/demucs-mlx](https://github.com/ssmall256/demucs-mlx) | 原生 MLX 版 Demucs，包含频谱与融合 Metal 实现 | 可以作为模型专用优化的对照，但不消费当前 ONNX，需要另一套模型实现、权重转换和一致性验证。作者 M4 Max 的性能数据不能与本机 ONNX 数字直接比较；本次未实测该路线。 |

当前优先研究 MLX EP 比通用 Python 转换器更符合“保留 ONNX + Rust”的要求；若后续要继续压低 CPU iSTFT 开销，再考虑向插件补一维反卷积支持或针对频谱图做等价改写，必须重新验证性能和音频输出。

## 本地证据与复现

`target/mlx-research/probe.py` 是隔离探针；`*-all-steady.json` 为上述 12 次数据，`*-all-lost.json` 为第二输入数据；对应 `.log` 含插件 claim summary，带时间戳 JSON 为 ORT profile。模型输出保存在 `*-output.npz`。实验文件、模型和音频没有加入 Git。

```sh
rtk proxy target/mlx-research/venv/bin/python target/mlx-research/probe.py separator cpu --runs 12 --suffix=-steady
rtk proxy target/mlx-research/venv/bin/python target/mlx-research/probe.py separator coreml --runs 12 --suffix=-steady
rtk proxy env ONNXRUNTIME_EP_MLX_VERBOSE=1 target/mlx-research/venv/bin/python target/mlx-research/probe.py separator mlx --runs 12 --suffix=-steady
rtk proxy target/mlx-research/venv/bin/python target/mlx-research/probe.py notes cpu --runs 12 --suffix=-steady
rtk proxy env ONNXRUNTIME_EP_MLX_VERBOSE=1 target/mlx-research/venv/bin/python target/mlx-research/probe.py notes mlx --runs 12 --suffix=-steady
```

复现 CoreML 命令会使用/生成 `target/mlx-research/coreml-cache`，不触碰应用的 stem 缓存。
