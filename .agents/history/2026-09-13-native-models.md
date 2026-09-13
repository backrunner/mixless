# Rust 原生模型接入与验证

日期：2026-09-13。本记录接续 [此前的离线原型与 AutoMix 改动](2026-09-13-automix.md)，不将旧 MLX/Python 测量当成当前 Rust 链路结果。部署与代码入口见 [原生分析参考](../reference/native-inference.md)。

## 本轮范围

- 新增 `mixless-stems`，通过 Rust `ort` 调用原生 ONNX Runtime，运行固定 HTDemucs 和 Basic Pitch 权重。音频重采样、分块、三轨合并、音符事件解码、特征提取和缓存均由 Rust 完成。
- 桌面默认启用独立后台队列。基础分析立即可用于手动加载及 AutoMix；分轨成功后更新 SQLite、后续过渡、预览与已加载 Cue 元数据，不 seek 或替换运行中的混音计划。模型失败保留基础分析，重分析可重试。
- 分轨人声活动、持续音符、实际移调后的音符进行与分离鼓低频参与切点、EQ/filter 和重叠窗口判断。修复“新音符在结构边界起音”被误作“持续音中途被切断”的通用判断，没有曲名或曲目 ID 特判。
- 模型 hash 校验、跨进程锁、内容/模型版本缓存、失效任务发布保护、磁盘余量检查、6 GiB 淘汰及空闲释放模型会话。
- 删除 Python 分析原型、依赖清单和打包脚本，新增 Rust `mixless-tools bundle`。保留许可证并随 app 打包。旧虚拟环境只留在被忽略的 `target/`，不参与构建和运行。

## 验证证据

- `target/native-workspace-verified.log`：workspace（不含 desktop）234 passed、0 failed、10 ignored。
- `target/native-desktop-verified.log`：desktop 49 passed、0 failed、1 ignored；涵盖缺少模型时继续手动播放和基础分析、重新分析更新 Cue 时保持播放位置。
- 实际模型测试覆盖默认异步队列、持续音频渲染、推理中再次准备曲目不等待模型、轻量证据写入 SQLite、重开分析复用缓存以及规划器接收模型数据。`target/native-integration-verified.log`：1 passed，实际 25 秒音频，8,991 个 128-frame block，p99 0.135 ms / 2.667 ms，测试整体 36.21 秒。渲染使用 offline callback 路径，并非 CoreAudio 设备录回测试。
- 两首完整原曲经 Rust 模型运行：Lost & Found（237.3747 秒，2,246 个估计音符）、In The Echo（183.5363 秒，863 个估计音符）。数据在 `target/deep-pair-analysis.json`；float stems 在 `target/native-stem-full/`。
- 首次完整分析墙钟分别为 109.03 秒和 456.70 秒（`target/native-full-pair.log`）。后者与编译/其他系统负载重叠，不能作为可比速度或实时保证。进程 RSS 曾观察到约 2.65 GiB。正因推理时间可能超过剩余播放时间，默认链路改为独立队列，不阻塞下一首准备。
- `target/native-pair-render3.log`：这两首双向接歌均完成。Lost & Found → In The Echo 保留 build-up 末端接 drop 的 DropCut；8.34 秒输出包含上下文，并非 8.34 秒重叠。
- `target/native-dnb-render.log`：14/14 组真实 DnB 接歌渲染完成，有限且未超范围，最大峰值 0.3617。只有上述两首替换为完整模型分析，另外 12 首复用先前混合频谱分析；不能称为 14 首完整分轨验证。

可复核音频：`target/native-dnb-render/9-10.wav`，以及该目录下其他 13 组 WAV。离线渲染验证执行、时序、数值边界；本轮未完成主观试听、标注音符准确率或跨机型性能测试。

## 运行边界

当前完成的是默认分析和 AutoMix 规划接入。播放仍是原曲加现有音量/EQ/filter 控制，没有三轨独立推子、Neural Mix 级实时声部播放或独立 stem seek/loop 音频图。音符是模型估计，不能当作真实乐谱。首次推理会消耗 CPU/内存，在后台显示进度；本机已验证不等待模型的准备路径，不保证所有负载和设备均满足同一 callback 预算。

## Release 音频预算

首轮 `target/native-callback-verified.log` 为 2 passed / 2 failed：Spiral p99 0.3843 ms，超过单 FX 0.12 ms 目标；128-frame 快速 Cue p99 1.692 ms，未达到 callback 50% 的余量目标，但仍低于 2.667 ms 完整期限。测量时无本任务其他编译/推理运行，系统存在其他活动负载，不能直接证明失败成因。

未修改代码或阈值，以同一 release 二进制直接串行复测（`target/native-callback-recheck.log`）4/4 通过：70 种 FX 最坏 p99 0.0883 ms / 256 frames；双碟普通播放 p99 0.433 ms / 256 frames；快速 Cue p99 1.093 ms / 128 frames；内录 p99 0.114 ms / 128 frames。保留首轮失败，复测通过不构成所有负载下的实时保证。

## 构建与交付

`./dev.sh --build` 通过（`target/native-build-verified.log`）。Rust `mixless-tools bundle` 已生成并原子更新 `target/app/Mixless.app`，没有重启已有实例。Info.plist 经 `plutil -lint` 检查通过，app 内二进制与 `target/debug/mixless` 的 SHA-256 一致：`64fbb9a9b099d88e9bc31d404630e928f9e2536ee372d23c678a1abcab0523a3`。

`otool -L` 只列出系统框架和系统 dylib，没有外部 ONNX Runtime 或 Python 动态库；模型和运行库许可证已经复制到 app Resources。仓库 Markdown 本地链接检查无失效链接，`git diff --check` 通过。本轮没有执行原生窗口的视觉验收或 CoreAudio 听感验收；需要关闭旧实例后启动新包才能使用本次代码。
