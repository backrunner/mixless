# Automix 实现与试用

基于 `.agents/05-ai-mixing.md` 和 `design.md` §7，实现了离线分析、下一对规划、实时包络执行及 GPUI 入口。默认逐首迁移速度和音乐调性，不把歌单锁在同一 BPM／key。听感优先于强行长混。

## 试用

启动桌面应用，选择至少两首本地歌曲的列表，点击顶部 **AUTO**。若已有一首在播，从该曲在列表中的位置继续；否则从第一首开始。下一首必须在空闲唱盘上准备。再次点击 AUTO 停止自动控制，保留当前播放及混音参数。

过渡就绪后可以 PAUSE / RESUME / SKIP。手动操作 crossfader、gain/fader、EQ、filter、send、rate、pitch 会取消对应自动化 lane。跳 cue、jog 抓盘、反转、手动播放暂停或换歌会停止当前计划；重新开启 AUTO 时读取当前实际 rate/pitch。

无需音频设备即可用与桌面相同的分析器生成过渡试听：

```sh
cargo run -p mixless-analyze --example automix-real -- a.wav b.wav preview.wav
```

自动分析 BPM、调性、网格与逐 bar 特征，输出浮点立体声 WAV，包含最多四秒前文和八秒后文，打印选用模式、接点及输出峰值。不会覆盖已有文件。支持引擎解码器接受的本地格式。

保留仅输入 BPM 的通路检查示例；它没有可信 key/grid/bar 特征，因此通常选择短桥接：

```sh
cargo run -p mixless-engine --example automix -- a.wav b.wav preview.wav 128 126
```

该示例从过渡起点开始，并附两秒尾部。

## 默认平滑策略

- `mixless-mixplan` 只依赖 `protocol`，每次搜索最多 8 个出点 × 8 个入点，分别搜索 8／16／32 bars 混合与短桥接。
- `BeatBlend`：两轨网格可信、拍号一致、sounding key 兼容、前景冲突低时，联合搜索 8／16／32 bars 的乐句窗口。共同 BPM 使用五次平滑曲线，限制最大对数变化率约 0.35%/s；出碟从当前实际速度开始，入碟静音时先对拍，再随共同速度迁移。常规恒定速度歌曲结束时回到入碟自身 BPM。
- 低频交接搜索过渡中间 25%～75% 的共同 downbeat，优先两首歌的共同乐句边界及有底鼓的入段，不固定在中点。强节奏段在一个拍内等功率交换；两侧底鼓都弱时使用最多四小节的渐进低频叠混。推子分两段五次平滑，与实际交接点一致，补偿低频衰减；入碟高频从 −3 dB 渐入，中频前景有风险时对次要声部轻度衰减。
- `PhraseBridge`：速度差距过大、调性不兼容、节拍不可靠或前景冲突时，出碟末段低频衰减、滤波和轻 Echo；最后约 40 ms 淡出干声，在乐句结束重拍启动入碟，保留其速度和调性。桥接不再提前一拍抢入，也不把入碟第一拍藏在长 crossfade 里。
- 80/160 默认使用 2:1 拍映射，rate 保持接近 1。`energy_hold` 保留 sounding rate/pitch；下一对读取实际偏移，可继续迁移，不累计到统一歌单速度。调性转换通过音乐内容交接完成，默认不在可听人声上做 pitch glide。
- 独立检查每轨用户 In/Out 范围，使用源采样率；自动 cue 和 Hot 不作硬约束。桥接也不能绕过 cue、已知人声 Verse → Chorus 禁区或音频边界。已测出的静音不作入点，无解返回 `failure_reason`。
- 依据实际低频交接窗口的 RMS 做最多 6 dB 的局部响度补偿，按 bar 与窗口重叠的时长加权，静音也计入。入碟从接点至文件结束的峰值证据限制正增益，避免安静 intro 导致后续 Drop 被额外过度放大；缺少峰值覆盖时不提升增益。引擎继承出碟当前 gain 和两侧 fader 比例，减少下一次交接时的响度跳变。这是启发式补偿，尚非 LUFS 标准化。

目前支持十一种策略；其中十种目录节点表、归一化评分、`who_stretches`、literal 半／双倍和额外恢复段保留在 `PlannerOptions { smooth: false, .. }` 路径，其金样继续验证，DryCut 仅作为平滑路径的安全策略。默认平滑路径使用 DryCut、BassSwap、PhraseBlend、EnergyHold、EchoOut，以及 Build→Drop 的 DropCut；ScratchCut 只有完整 cue 证据才会进入。

## 接歌规则修订（2026-09-09）

- **打击乐分层**：不兼容或未知调性只有在至少一轨整段具备严格打击乐证据时才允许 BeatBlend：低前景／人声风险、有起音和底鼓、无和弦／局部调性标记、Chroma 不集中。缺失 Chroma 不代表纯打击乐；证据失败退回桥接，不改变人声 pitch。这是启发式门限，不能当作 stem 分离或准确率保证。
- **避免低频空洞**：有强底鼓的出段不能将低频直接交给缺底鼓的入段；继续搜索其他交接点、窗口或桥接。八小节重叠内部可能没有完整乐句，允许共同 downbeat，但有证据的共同乐句优先。
- **人声句子保护**：保留重叠比例上限，另外拒绝连续超过一拍的双前景；Verse／Chorus 冲突双向检查，长过渡不能用平均值掩盖一整句叠唱。
- **分析数据检查**：非有限频段、Chroma、RMS、人声概率以及相互重叠的 bar 范围直接报错，不让异常缓存绕过门限。
- **Loop Roll 与 Roll FX**：只有网格稳定、入碟有连续底鼓、低人声风险且交接落在乐句尾部时才建立 1／2 小节量化 loop；在释放前一小节收窄 LP 并短暂送 Echo，释放点与 incoming downbeat 对齐。真实 loop 保持源域长度，避免把重复段误当作连续播放。
- **搓碟切割**：`ScratchCut` 是受限的三段式性能动作，不是任意 jog 抖动。规划器必须同时看到可靠网格、出入乐句边界、两侧强 kick、出碟低人声、入碟 Hot cue；动作只持续半小节，回拉峰值约半拍，使用 vinyl/slip 释放回到原播放头，再在入碟重拍交接。缺少任一证据、用户 cue 或音频边界时拒绝 scratch，回退到 EchoOut／PhraseBridge。引擎以不可变的 `ScratchOp` 驱动现有 `SetJogTouch`／`jog_target` 通道，释放时自动关闭 slip，避免跳帧和失控拖盘。
- **FX 决策顺序**：先做硬否决，再选技巧，不用“用了更多 FX”增加分数。稳定网格＋兼容调性＋低前景冲突优先 Dry Cut／Bass Swap；Loop Roll 需要量化网格、连续 kick 和低人声；Scratch 还需要用户 Hot cue；Build→Drop 且入碟重拍明确才用 Drop Cut；出碟是 Outro/Break 或有人声尾句时用 EchoOut；调性冲突但仍需填补短桥时用 FilterBridge；网格、人声或乐句证据不足时禁止 Loop/Scratch，回退 PhraseBridge。策略摘要中的 `filter_sweep`、`echo_out`、`drop_cut`、`loop_construct`、`scratch_cut` 对应这套选择结果，便于 UI 展示原因。
- **实时边界**：新决策和曲线采样都在宿主规划阶段，沿用每 32 帧执行一次的音频线程控制结构，没有添加 callback 分配或模型推理。

新增八项回归覆盖非中点的共同乐句交接及低频功率连续性、低频缺失、后段峰值和证据缺口、时长加权音量、跨调打击乐及其反例、连续人声碰撞、异常特征、弱底鼓渐进叠混。原有 8/16/32 小节、80/160、44.1/48 kHz 双向实际渲染、用户 cue 和手动接管测试保留。

本次在 macOS 27.0 执行全工作区测试与桌面构建，并用资料库两首本地音乐离线导出过渡（音频留在 `target/auditions/`，不进入版本库）。第一方向选择受约束 EchoOut，20.56 秒，渲染峰值 −4.23 dBFS。局部网格不可靠时仍可能主要使用桥接；没有以少量样本或合成测试证明“百大 DJ 水平”，也没有完成多人盲听、真实音频设备试听或整场选曲能量控制的验收。

## 分析与执行

- 桌面 AUTO 使用独立离线解码的完整 `TrackAnalysis`，含源域 beat/downbeat、逐 bar RMS／峰值／频段能量、kick 显著性、持续前景风险、多尺度结构边界，以及带有证据限制的乐段标签。macOS 12+ 还会尝试系统人声分类增强。
- 下拍由低频重音估计；均匀节拍没有足够证据确定 bar one 时降低置信度。按局部窗口检验与全局网格的符合度，变速区域不能继承其他稳定段的可信度。当前局部窗口估计仍可能发生相位漂移，尚非完整逐拍跟踪。
- SQLite `track_analysis` 按 content hash 和 `ANALYSIS_VERSION` 缓存；当前版本 6 还会清除静音／缺乏节奏证据时的默认网格，并刷新 DJ 乐段标签。缓存命中同样更新唱盘 BPM 和有效节拍网格。
- 宿主预计算每 32 个设备帧一个控制点，通过源时间曲线积分控制 rate，滤波使用对数插值。音频线程读不可变数据，支持 A→B、B→A、暂停／恢复、Skip 和单 lane 接管。规划不进音频线程。
- 后台准备下一首并读取当前 rate/pitch；取消后丢弃未发布的解码结果。手动换歌会停止当前计划，重新开启 AUTO 才重规划。

## 验证

```sh
cargo test --workspace --exclude mixless-desktop
cargo check -p mixless-desktop
```

测试包含规划器的乐句窗口、cue 约束、速度迁移与低频包络回归，分析器的音色边界、鼓花、弱起、静音与反相立体声回归。系统模型测试默认忽略，在 macOS 上使用下方命令单独验收；具体本机结果见 [ANALYSIS_ENHANCEMENT.md](ANALYSIS_ENHANCEMENT.md)。

新增验收覆盖升／降速斜率、80/160 双向映射、低频包络功率连续性、hold 后继承再释放、静音接点、短桥接重拍及人声/cue 硬约束。44.1/48 kHz 源音轨通过实际渲染引擎时，在 128→132 BPM 合成夹具上测得双向最大播放头节拍相位差约 **0.000246 拍**；这是特定夹具的 transport 测量，不是所有音乐的声学对拍或听感保证。

## 当前范围

- `vocal_presence` 是持续前景风险代理，`vocal_confidence` 是可选的人声分类证据，二者都不能替代声部分离。结构仍是启发式，分析继续标记 `partial`。实际乐曲中有歧义的下拍、拍号、局部变速和调性变化仍需更可靠模型与试听校准。
- 当前界面没有新增 mix-in/out 编辑菜单；规划器和资料库已有 cue 类型接口。曲线 overlay、手动换歌后无缝重规划、shuffle、分析任务队列与事件订阅不在本次实现内。
- 暂停保留已编译剩余包络并暂停双碟；恢复继续该包络。Skip 立即完成当前交接，让入碟成为下一对的出碟。手动改动某 lane 后，该 lane 在本计划内不会被重新接管。
- 尚未验收真实音频设备的延迟、全自动化负载 p99、连续多首音乐试听。32 帧控制精度不等于 Signalsmith 的听感响应延迟。现有滤波器在 30 Hz～18 kHz 范围映射，表中 20 Hz／20 kHz 为全开端点。

## 乐段与进出点增强（分析版本 6）

- 结构检测使用 2／4／8 小节窗口，比较响度、频段分布、Chroma、起音和前景风险；通过窗口内变化量抑制短鼓花，避免单个峰值或歌曲固定百分比决定乐段。局部均值直接计算变化强度，不分配整首歌的平方大小相似度矩阵。
- 保留音色不同但标签相同的乐段边界。重复乐段、相对能量、持续上升和可选歌声证据用于保守地区分 Build、Drop、Verse、Chorus、Intro、Outro、Breakdown；不确定时保留 Unknown。
- `phrase_boundaries` 独立记录源时间、边界置信度和变化强度。在测得的边界上按实际 downbeat 向后推导 8 小节乐句，处理弱起及不按全曲第 8n 小节开始的乐段。强变化边界可以不在全曲 8n 位置。
- 规划器先剔除已错过的出点，再排序并限制候选数。边界质量、能量落差、底鼓差异、前景冲突实际进入最终评分；可信乐句边界约束入点、出点和长混两端。用户显式 In／Out 保留原覆盖语义。
- 同时搜索 8／16／32 小节，避免为了固定 16 小节长混而跨过下一段 hook。Build 完整结束、入碟是明确 Drop 时可直接 DropCut，不叠加 Echo 尾音。其余不适合长混的曲对仍尝试受约束桥接。

## macOS 系统人声分类

默认在 macOS 12+ 尝试 Apple Sound Analysis `version1` 分类器。它在本机运行，无模型权重下载，无 Python 或 ONNX Runtime 依赖。输入来自本次分析已经解码的 PCM；不重新读文件，也不打开音频设备。仅使用 speech、singing、rapping、choir_singing、humming 的精确类别，避免把 singing_bowl 或 bird_vocalization 当成人声。

每次提供半秒单声道缓冲、1.5 秒分类窗、零重叠；只允许一个模型任务，遇到并发任务直接跳过。模型默认预算 2 秒（配置最多 5 秒），不足 1.5 秒或超过 10 分钟的文件跳过。预算在分块之间检查，**不保证抢占一次正在执行的系统推理**。超时、不可用或非法结果不发布部分模型证据，原 DSP 分析保持可用。分类结果只会提高前景重叠风险，模型低分不会清除主旋律风险；段内覆盖不足的人声置信度保持未知。

`Analyzer::with_options(AnalysisOptions { native_vocals: false, ..Default::default() })` 可关闭增强。现有桌面和导入流程使用默认配置，旧缓存通过 `ANALYSIS_VERSION=6` 刷新；没有新增偏好设置界面。`analyze_track_with_report` 返回解码、特征和模型阶段耗时及回退状态。

```sh
cargo build --locked --release -p mixless-analyze --example analysis-profile
/usr/bin/time -l target/release/examples/analysis-profile track.wav
/usr/bin/time -l target/release/examples/analysis-profile --dsp-only track.wav
cargo test --locked -p mixless-analyze native_model_runs_and_deadline_discards_partial_results -- --ignored
```

M4、16 GiB、macOS 27.0 的 Release 测量：两首 184／189 秒本地音乐和一个 240 秒鼓点夹具，各模式独立进程重复 3 次，模型阶段 0.327～0.445 秒，启用增强的进程最大 RSS 比同文件 DSP 模式高约 28 MiB。完整分析进程最大 RSS 约 127～199 MiB，其中包含整轨 PCM，不能把它全部算成模型占用。没有测量系统服务的总驻留内存或实际 ANE 算子分配，也没有把这些少量样本当成识别准确率或所有 Mac 的性能保证。

模型路线、测量表与下一阶段的验收目标见 [ANALYSIS_ENHANCEMENT.md](ANALYSIS_ENHANCEMENT.md)。

## 研究来源

本次查阅了以下教学文章，并从 Crossfader 正文取得对应 YouTube 教程链接；技法依据文章详解，没有声称逐帧观看或听完视频。

- [Crossfader：3 Ways To Mix House Music](https://wearecrossfader.co.uk/blog/3-ways-to-mix-house-music/)；[配套 YouTube 教程](https://www.youtube.com/watch?v=CTiFony36zs)。渐进分频混合、乐句起点上的 Bass Swap、打击乐／人声分层。这里将手动旋钮操作转换为有边界与功率检查的自动化包络，门限属于本项目实现选择。

- [Digital DJ Tips：接歌位置与歌曲结构](https://www.digitaldjtips.com/how-pro-djs-know-where-to-transition-plus-a-big-cheat/)
- [Crossfader：DJ Looping Guide](https://wearecrossfader.co.uk/blog/dj-looping-guide/)，量化 loop、释放时机和 build 延长。
- [Crossfader：5 DJ Transitions with Scratching](https://wearecrossfader.co.uk/blog/videos/5-dj-transitions-with-scratching-5-levels/)，baby／chirp 类短搓碟只作为 downbeat 前后的切割装饰。
- [Crossfader：12 High-Energy DJ Transitions](https://wearecrossfader.co.uk/blog/12-high-energy-dj-transitions/)，roll、echo、filter 与 drop cut 的组合边界。
- [FMP：Foote novelty 与结构边界检测](https://www.audiolabs-erlangen.de/resources/MIR/FMP/C4/C4S4_NoveltySegmentation.html)
- [Apple：离线音频文件分类](https://developer.apple.com/documentation/soundanalysis/classifying-sounds-in-an-audio-file)
- [Apple Core ML](https://developer.apple.com/documentation/coreml)
- [ONNX Runtime Core ML provider](https://onnxruntime.ai/docs/execution-providers/CoreML-ExecutionProvider.html)
