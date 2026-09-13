# 2026-09-14 独立声部实时混音验证

本轮在已有 Rust/ONNX 默认深度分析上接入实际 PCM 播放。音频设计和操作方式见 [独立声部混音](../reference/stem-playback.md)。保留之前的 [原生模型接入记录](2026-09-13-native-models.md) 作为当时仅分析、不播放声部的历史范围。

## 行为及并发

- 工作区常规回归：240 passed，0 failed，11 ignored；桌面常规回归：49 passed，0 failed，1 ignored。日志为 `target/stem-workspace-final.log` 和 `target/stem-desktop-final.log`。
- 覆盖全开等价原曲、隔离人声对照、共同 Cue/loop/reverse/keylock 时钟、源身份和尺寸校验、锁竞争、静音、无效增益，以及 AutoMix 双碟就绪回退和单声部手动接管。
- Rust 分配检测涵盖首次 callback、Cue、卸载，分配/释放/realloc 均为 0。首次运行发现 macOS `std::Mutex` 延迟分配；修复为流启动前初始化所有回调可见锁。它不拦截原生 C++ allocator，不能据此声称所有底层运行库零分配。
- 检查中发现 snapshot 在持有 buffer 锁时阻塞，已将声部就绪查询改为 try_lock；完整回归通过，包括原有锁竞争测试。

## 真实模型与播放

Apple M4 / 16 GiB，release 构建，48 kHz 输出、128 帧（2.667 ms）块。测试期间未启动本轮其他编译或压力测试。

使用本地真实曲目的 25 秒片段和固定 HTDemucs / Basic Pitch ONNX 模型，通过桌面默认队列验证缓存、数据库证据、规划器及已加载碟盘的声部 PCM 发布，然后将两碟人声/鼓增益分别设为 0.3 / 0.65，启用变速、移调、Loop，同时重新启动一次真实模型推理。

| 场景 | 采样块数 | 回调 p99 |
| --- | ---: | ---: |
| 原曲播放与默认分析队列并行 | 3,081 | 0.107 ms |
| 双碟声部 + 变速/移调 + 新一轮模型推理 | 2,912 | 0.326 ms |

实际 native 集成测试 1 passed，耗时 20.64 s；日志 `target/stem-native-integration.log`。后者要求 p99 小于块期限的一半。计时是离线回调执行耗时，包含输出 Vec 的创建，未测量 CoreAudio 调度抖动。

双碟合成压力场景进一步包含两套 Loop、Reverb、内录，以及每 64 块同时重触发双碟 Cue（足以进入 p99）。首次运行 p99 1.035 ms，最大值 1.153 ms，验收线 1.333 ms；日志 `target/stem-performance-first.log`。

最终串行回调套件 5 passed：70 种 FX、普通/scratch/brake、Cue、内录及上述声部组合负载全部通过。声部组合 p99 1.044 ms，最大值 1.870 ms；其他回调 p99 均在各自验收线内。完整输出 `target/stem-callback-final.log`。最大单块耗时也低于 2.667 ms 期限，但该测试没有覆盖每一种 FX 同时叠加的全部组合。

真实完整曲目 Lost & Found / In the Echo 双向渲染在实际加载分轨 PCM 后完成，分别仍选择合法 DropCut / DryCut，没有声部自动衰减；日志 `target/stem-playback-pair.log`。偏好 PhraseBlend 的复查也回退到瞬切。因此这两次渲染证明真实 PCM 兼容既有切换，不能算作实际歌曲的 AutoMix 长重叠声部包络验收。包络运行由引擎回归验证，真实增益播放由上述并发测试验证。

## 构建与界面

最终 release 桌面构建通过，并通过 Rust 工具打包到 `target/app/Mixless.app`。应用 Info.plist 校验通过，包内可执行文件与 `target/release/mixless` 的 SHA-256 一致：`b7f8e0c409e8ab77754bb2cf2dcff479beaf85c577cf8a2d7d00d311b36aa7c7`。运行库静态链接；动态依赖清单为系统框架/库，没有 Python 或额外 ONNX dylib。

原生界面验收使用隔离数据库、离线设备模式，以及将原型 CLI 的裸 hash 缓存映射到相同音频内容的桌面 `blake3-` hash 的测试缓存。直接观察到拖拽上碟的 Loading、双碟三声部控件与 VOCAL OFF 状态。发现较矮窗口的声部行挤到曲速推子后，将该行移入唱盘列，并把控件标签放到旋钮下方。重启后自动化曾返回 timeoutReached，重置工具会话后恢复；最终 release 界面确认布局互不重叠、A 碟人声可连续拖到 60%、B 碟可独立关闭人声，以及双击 A 旋钮、重新开启 B 人声都恢复为 100%。默认分析链路没有依赖此测试缓存映射，已由上面的真实模型集成测试独立验证。

本轮声部相关文件通过对应 Rust edition 的 rustfmt 检查，`git diff --check` 与更新文档的本地链接检查通过。全仓 `cargo fmt --all -- --check` 仍报告其他原有文件的格式差异，未做无关格式整理。

## 验证边界

后续界面调整：曲速细推子放在 SHIFT 下方，三声部旋钮移到 SYNC 下方，与推子共用剩余高度；标签下置、居中、不显示百分比，未加载分轨时隐藏。A/B 的控制列、SYNC/KEY 排列左右镜像，使两侧 SHIFT/曲速都靠外。最终 release 重新构建并打包，SHA-256 为 `d27f10b28381ceaee8a7dd43969fddb3f097ccedb3b902346ba094a59a6956f9`。隔离原生界面实际加载 In The Echo 到双碟，确认镜像、标签与间距；格式、diff、Info.plist 和包内二进制一致性检查通过。此次仅调整布局，未重复音频性能测试。

当前每声部控制的是增益，EQ/filter/FX 位于声部合成后的通道。没有新增声部独立 FX 机架。instruments 含原曲重构残差，分离泄漏及音符识别准确率仍需标注和盲听评价。本轮自动数值验证不等于人工听感验收，也不能外推为其他硬件或所有系统负载下的回调保证。
