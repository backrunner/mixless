# AutoMix 实现与试用

更新：2026-09-13。当前行为以代码和测试为准；本轮验证见 [运行记录](../history/2026-09-13-automix.md)，教学及模型研究见 [研究依据](automix-research.md)。早期设计稿不是已实现功能清单。

## 使用与显示

选择播放列表会在后台准备分析和相邻曲对，包括末曲到首曲。点击顶部 **AUTO** 后，从当前在播曲继续；空碟从列表首个可播放条目开始。支持顺序及逐轮乱序循环。下一首在空闲碟盘提前载入，静音时设置接点、速度、key lock、可选移调和初始 EQ/filter。入碟开始播放时自动选中它。

仅开启 AUTO 时，列表全曲波形显示计划的 **IN / OUT** 和混音区间：上半部青绿色是进入窗口，下半部紫色是退出窗口，竖线标出边界；瞬切显示点位，不把准备时间画成混音。实际运行的计划覆盖后台预估；乱序只显示已经确定的当前曲对。编号 Cue 和 A/B 实际播放位置仍保留。关闭 AUTO 后隐藏计划标记。

拖动整行调整的顺序存入 SQLite，列表刷新和重新导入会保留现有曲目的相对顺序，新曲追加到后面，删除的条目退出队列。重复曲目按出现次数保留。应用记忆选中的列表；正常退出会等待尚未完成的排序写入。

过渡条显示源时间范围、技法、阶段、倒计时和实际控制值。PAUSE / RESUME 同时暂停和继续当前双碟计划；SKIP 完成交接。手动改变控制 lane 会接管该 lane。Cue / Play 关闭 AUTO 会话；手动换歌会保留队列意图并重新准备。关闭 AUTO 保留当前播放与控制位置。

## 音乐安排

- 混音起点来自测得的编曲、乐句与用户 Cue，不要求在 build-up/drop 前后固定多少小节。允许在高潮仍播放时提前铺入合适素材，最终退出不能截断已识别的高潮。高潮后的 break 可从真实边界开始混合。
- 普通叠混搜索实际乐句长度以及 4/8/16/24/32/48/64 小节等候选，窗口上限 128 小节。相容且可靠的长窗口有持续性评分；没有统一最小时长，也不为达到某个数字而跨越不合适的乐段。
- **DropCut** 需要完整出碟 build-up 直接衔接入碟 drop 的证据。结构和声音接点允许时，也可在一个完整 drop 后瞬切到另一首 break。瞬切计划长度为零，不提前一小节启动入碟；引擎仍做短去点击处理。
- Bass Swap 是持续重叠中的低频交接阶段，不等于整个混音时长。高频、低频、中频、Level、filter 和 crossfader 可分阶段迁移，包络使用平滑曲线。提前铺入低能量素材时，通过频段和音量保留出碟高潮的主导地位。
- 持续音与人声活动检查用于避免正在发声的中间硬切。不是强制播完整个 Verse；可使用测得的间隙、EQ 和渐退控制双前景冲突。证据不足时不会把“没检测到人声”当作无人声。
- 出碟人声未释放（`voice_released`：切点处有可测呼吸/间断、唱音正好落在边界上结束、或切点后约 0.4 s 内活动低）的位置，不允许放瞬切、DropCut 或桥接类结尾。唱到边界前一瞬才停是合法切点，不要求切点前先行安静；人声延续过切点才阻止。用户显式 OUT Cue 例外。
- 对整段重叠检查网格、tempo、局部调性、频段和前景风险；额外比较移调后的 Chroma 进行及持续冲突。Chroma 是混合音频的音高分布，不是已识别的乐谱或 stem。
- 双碟都有可用 stem PCM 时允许声部层叠混合：入碟先只铺鼓，交接小节前再渐入人声与乐器，出碟人声优先在可测呼吸处渐退。此类计划标记 `requires_stems`；引擎在两碟声部未就绪时拒绝执行，宿主随即按保守方式重新规划。
- Loop 需要稳定网格、鼓与前景证据，并量化释放；Echo/Filter 由声音和结构需要决定，不以特效数量加分。低置信度网格不能通过放长混音或堆叠 FX 变成可靠对拍。
- 出碟退出落在自身下一个 Drop/Chorus 开始前 8.5 小节内、且退出点本身不是高潮结尾时，只有 build-up 交换成立：入碟高潮起点必须精确落在交接点（叠混/LoopRoll 为 `b_end`，瞬切为 `bin`），否则该候选被否决。对称地，叠混重叠区间内不允许入碟 Drop/Chorus 开始——它只能落在 `b_end` 或之后。
- FilterSweep 叠混上限 16 小节（一侧为纯打击乐时除外）；stem 声部层叠窗口限制在 8–32 小节。
- `drops::peaks` 忽略短于 3.5 小节的 Drop/Chorus；`major_peaks` 要求 ≥7.5 小节且中位 RMS 不低于最响高潮约 −1.2 dB，用它计算"主体高潮已播完"罚分，退出早于曲目 40% 时另加提前退出罚分。出点候选保留范围从曲目 25%（上限 60 s）开始，用户 OUT Cue 例外。
- **Spinback**：出碟末半小节加速回拉的 backspin 停碟（`ScratchOp.accelerate`），低通收至 2.5 kHz、低频与增益同步关闭，入碟在其自身 Drop/Chorus 起点落下。要求双网格可靠、乐句对齐 ≥0.75/0.6、出碟末段 kick ≥0.5 且人声 <0.5、入碟 kick ≥0.55。确定性轮换：与合格 DropCut 竞争时约 40%，无竞争约 70%。
- **LoopOut**：BeatBlend 的 4/8 小节窗口中，出碟循环其开头 1–2 小节并被逐层抽离——人声 1/4 小节内归零、乐器在中点前归零（双碟 stem 就绪时用声部包络并标记 `requires_stems`，否则用 −12 dB 中频与 −6 dB 高频）；低频在中点交接，结束时释放 loop 并关闭出碟。约 1/3 合格叠混启用，条件含入碟结构为 Intro/Break/Breakdown 或其高潮恰好落在 `b_end`。
- 技法轮换（Spinback、LoopOut 以及 Echo/Filter 结尾互换）由 `(曲对 id, 退出点, 进入点)` 的确定性哈希决定：同一输入永远得到同一策略；Outro 结尾保留 Echo 不换 Filter。

出碟从实际 rate/pitch 开始，入碟先静音同步，再按共同速度曲线迁移；恒速歌曲完成后可恢复入碟原 BPM。支持半/双倍拍映射。可信局部调性需要修正时，桌面可尝试最多 ±2 半音，交接后保持该偏移，下一对继承实际状态，不在可听人声上突然恢复原 key。源音频默认做响度标准化，规划器不再用接点附近安静 intro 的 RMS 任意放大整首歌。

## 分析与执行边界

`mixless-analyze` 生成源域 beat/downbeat、逐 bar 响度/频段/kick/Chroma、结构边界、多组进出区域，以及 50 ms 的持续音/前景/起音证据。当前 `ANALYSIS_VERSION=16`，桌面默认追加原生分轨和音符证据。结构标签仍有歧义，不能保证每首歌的 drop、下拍或 key 都正确。

macOS SoundAnalysis 是可选的系统声音分类补充；默认有时间预算，失败回退至 DSP 分析。它不分轨，也不输出音符。[Rust 原生模型链路](native-inference.md) 默认在后台分离 vocals/drums/instruments，提取音符并写入分析缓存；AutoMix 用它定位人声间隙、保护长音、比较音符进行与低频冲突。[独立声部实时混音](stem-playback.md) 提供三声部增益及可选的 AutoMix 声部包络；只有两碟匹配的 PCM 都准备好时执行声部包络，否则继续使用整轨 EQ/filter/Level 方案。

后台规划生成不可变控制曲线；宿主编译为每 32 个设备帧的控制点，音频线程只执行，不查询 SQLite、不分配模型、不做推理。计划和预览带列表版本，重排、改 Cue 或重新分析后刷新，避免显示上一对的窗口。

## Live moves（过渡期外的实时动作）

AUTO 正常播放期间，`mixless_mixplan::performance_moves` 按分析为出碟安排两类小动作，由 `live_moves` 偏好（Off / Subtle / Active，默认 Subtle）控制：

- **滤波渐升**：对止于高潮起点、且具备 build 证据（BuildUp 标签、`has_buildup`、或末 4 小节 onset density ≥首 4 小节 1.2 倍）的段落，在其最后 4（Subtle）/ 8（Active）小节把通道滤波从旁通升至高通约 400 Hz（`amount` 0.405；窗口人声偏强时用 60%），在高潮下拍前 5 ms 回到旁通。
- **鼓声部抽离**：≥16 小节的主要高潮内，在 8 小节边界的前一拍把 drums 静音并在下拍瞬间恢复（需出碟 stem 就绪；Subtle 只用最后一个合格边界，Active 用全部）。

动作要求整个窗口网格可靠，且完整落在"播放头 +0.5 s"到"过渡开始前一个拍"之间——放不下就整个放弃，绝不截断半个扫频。宿主 `Performer` 经 `Engine::perform` 下发，不写计划的 lane 接管标记；用户在任一 lane 上的手动操作使该 lane 在本曲内永久交还；动作结束、过渡开始或 AUTO 停止时把仍在动作中的 lane 恢复中立值。

## 验证入口

```sh
cargo test --locked --workspace --exclude mixless-desktop
cargo test --locked -p mixless-desktop
cargo test --locked --release -p mixless-engine callback_budget -- --ignored --nocapture --test-threads=1
./dev.sh --build
cargo run --locked --release -p mixless-analyze --example automix-real -- outgoing.wav incoming.wav preview.wav
# 库副本离线审计（不改线上库）
cargo run --locked --release -p mixless-library --example reanalyze-copy -- LIBRARY.db OLD_VERSION NEW_VERSION
cargo run --locked --release -p mixless-library --example automix-audit -- LIBRARY.db VERSION
cargo run --locked --release -p mixless-library --example live-moves-audit -- LIBRARY.db VERSION
```

测试包括零小节瞬切、临近边界才开启 AUTO、自然 EOF 接续、24/48/64 小节分层混合、移调后的进行比较、播放列表重复条目/刷新/退出保存，以及计划波形映射。真实曲对渲染检验完成交接、采样有限性和输出范围；这些不能替代主观试听或原生界面操作验收。
