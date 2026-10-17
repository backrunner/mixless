# 05 — AI Mixing

两阶段。规划绝不进 callback。生产路径在 host 后台连续准备当前 set，共享列表预览与执行计划；逐对传递入点、上一段结束源时间和速度/调性，整组发布依赖决策。歌单、顺序或准备证据变化会更新后续计划；未开始且仍有安全提前量的交接可以在后台完成解码和编译后替换候选。已经开始或来不及安全替换时保留当前交接，将新候选放到下一位。

## 2026-09 平滑策略实现补充

默认 `PlannerOptions.smooth=true`：逐对迁移 BPM 和音乐调性，不锁整张歌单。可靠网格、兼容 sounding key、低前景冲突时使用 `BeatBlend`；共同 BPM 走五次平滑曲线，最大对数变化率约 0.35%/s，低频在重拍附近一个拍内按等功率交接。半倍／双倍仍使用 2:1 拍映射。`energy_hold` 可保留偏移，下一对读取实际 rate/pitch。

不满足长混条件时使用 `PhraseBridge`：出碟末段滤波／轻 Echo，在其乐句结束重拍启动原速、原调的入碟。出碟干声在重拍前约 40 ms 内淡出，入碟第一拍直接接管；不把两条不兼容的鼓点或主旋律长时间叠加。用户 In/Out、前景 Verse → Chorus 禁区和音频边界约束同样适用。已测出的静音不能成为接入点。

目前支持十一种策略；其中十种目录表及归一化评分保留在 `smooth=false` 路径，金样继续验证，DryCut 仅作为平滑路径的安全策略。平滑路径使用 DryCut、BassSwap、PhraseBlend、EnergyHold、EchoOut 和受结构约束的 DropCut；ScratchCut 只有完整 cue 证据才会进入。`who_stretches`、literal 和其他显式目录选择属于目录路径。完整现状、试用和验收限制见[当前 AutoMix 参考](reference/AUTOMIX.md)；主设计 §7 同步记录本补充。

2026-09-09 修订：低频交接搜索共同 downbeat，优先共同乐句，不再固定中点；有底鼓时一拍交接，两侧底鼓都弱时渐进叠混。入碟高频轻度渐入，局部 RMS 按真实窗口时长加权，正增益检查入点后完整峰值覆盖。具备整段保守打击乐证据时允许跨调 layering；缺失证据不授权。连续超过一拍的双前景不能被长窗口平均值掩盖。实现、八项新增回归、教学来源和真实样本渲染边界见 `AUTOMIX.md`。

2026-09-09 性能动作修订：Loop Roll 只在稳定 grid、连续 kick、低 vocal 风险和乐句尾部建立 1／2 小节真实 loop，并在释放前协同 filter／echo。`ScratchCut` 只接受可靠 grid、出入边界、强 kick、出碟低人声和入碟用户 Hot cue；生成半小节三阶段 `ScratchOp`（touch、半拍回拉峰值、slip release），通过引擎现有 jog target 执行，释放后恢复原播放头。任一条件失败都回退安全桥接，禁止把随机 jog 或长时间搓碟自动化。

FX 决策层位于平滑规划器之前：先做 grid、phrase、vocal、kick、harmonic 和 cue 的硬否决，再选择 DryCut、EchoOut、FilterBridge、DropCut、LoopRoll 或 ScratchCut。干净的和声／节奏交接不叠 FX；Outro/Break 的尾句优先 Echo；调性冲突的短桥优先 Filter；FX 不能修复不可靠节拍或连续双前景。

2026-09-21 修订：`ANALYSIS_VERSION=16` 修正半速曲目误读（comb 门控与 3/4、4/3 别名候选）、抑制 <4 小节假高潮、结构切点吸附乐句网格并新增软 build-up。规划侧禁止无交换的 drop 前退出与叠混内入碟高潮，FilterSweep 限 16 小节，`major_peaks` 驱动高潮罚分；新增 Spinback 与 LoopOut，技法按 (曲对, 退出, 进入) 哈希确定性轮换。过渡期外新增 live moves（滤波渐升、鼓声部抽离，用户触碰即交还该 lane）。细则与审计见 [AUTOMIX 参考](reference/AUTOMIX.md) 与 `history/2026-09-21-melodic-dubstep-audit.md`。

2026-09-21 恢复窗口修订：短恢复窗口无法容纳整段 BPM 迁移时，可保持当前 sounding tempo 并传递入碟 rate；不再把高潮前固定 8.5 小节一律当作已进入 BuildUp。具备完整低人声测量的恢复尾句可使用一小节原速重叠，减少曲尾硬切。真实蓄势、高潮完整性、手工 cue 与缺失证据的安全约束继续生效。实曲差异和验证边界见 [恢复窗口交接](history/2026-09-21-recovery-handoffs.md)。

2026-09-24 修订：自动退出必须落在已识别结构边缘、具有有效置信度及变化证据的乐句边界，或真实文件末尾；固定小节计数不单独授权退出。交接按各曲活跃段相对 RMS 检查能量承接，避免强段落接入安静淡入。频谱证据驱动保守、仅衰减的分段 EQ 与叠混频段平衡；缺失证据保持中性。整组计划按实际队列发布，同一播放轮次的入、出窗口不得重叠。验证记录见 [set 规划检查](history/2026-09-24-automix-set.md)。

2026-09-24 跨歌单入点补充：短切按入碟首个源小节评估接管能量，避免后面较响的四小节平均值掩盖起始淡入；EOF 兜底比较全部安全入点，不再按时间取第一个。手动 IN、合适的起始入点及叠混评估保持有效。见[复现与验证](history/2026-09-24-cross-playlist-entry.md)。

2026-10-17 完整乐句混出修订：set 规划允许在较早的完整主体高潮之后混出，前提是高潮结尾有实际变化证据、其后有完整恢复乐句，并同时测得能量、低频和节奏活动回落。较长的后续高潮不再单独强制整轨播完；证据不足仍保留原有主高潮约束。通用自动 OUT 建议不变，手动 OUT 仍优先。跨度搜索保留准确落到入碟高潮的长度和较近的乐句长度，避免均匀裁剪遗漏可行的叠混。见 [Alone → Ember 复现与验证](history/2026-10-17-phrase-release-mixing.md)。

2026-10-17 声部编排修订：一个完整主体 Drop 足以满足本轮播放，不再因后续 Drop 更长而继续等候。同步叠混在 `composition/` 中搜索每侧人声、鼓、乐器和低音的独立交换时机与淡变宽度；主要混合区间 crossfader 中置，Level 配合声部比例决定实际贡献，EQ/filter 保留一个中高频前景。最终曲线必须保留仍在演唱的词句和正在持续的高潮能量。预备对速使用两碟绝对速度预算的交集，并继承前一对的真实偏移；终端瞬切可在静音入碟上预设有利于下一首的速度。跨度搜索同时保护出碟编曲起点。见[声部编排复现与验证](history/2026-10-17-layered-live-mixing.md)。

2026-10-17 对拍修订：原拍与半/倍拍关系在共享对速前选择；准备窗口同时遵守双碟绝对速度及各自继承速度的变化预算。安静弱起可延后至局部可靠下拍做实际分轨引入。原速桥接禁止两个可听节奏前景重叠，包括鼓之外的琶音、脉冲低音与人声切片；同步编排另按实际秒数惩罚突兀的乐器增益展开。`ORDER_VERSION=3`。见[对拍、和弦引入与低潮入口记录](history/2026-10-17-rhythm-and-cue.md)。

## Mix 时钟

2026-10-17 大曲速差修订：双碟 stem PCM 就绪的原速 PhraseBridge 改为按实际声部能量搜索独立包络，不再在开头一次性撤掉鼓和低音。搜索入碟源域重拍、旋律交接区间及两碟各声部淡变宽度，联合评估能量损失与节奏冲突。每碟 EQ 按重叠频段贡献独立衰减，通道音量补偿实际交叉推子定律并缓慢抑制能量突增。失同步鼓组与低音不叠加；调性不确定时所有有音高声部分开交接。新增能量凹陷检查以相同时刻的可用源能量为参考，避免把 breakdown 开头的一次 impact 当作整段应维持的响度。源码在 `bridge/stems.rs`；所有计算在 host 完成，callback 只消费曲线。

- Master = **出碟**，除非策略声明（`energy_hold` 在 xf 过 0 后把 master 交给入碟，仍用出碟 sounding 网格）。
- `length_bars` 按 master grid。
- 入碟起始源帧：rate 折线之后，选定 downbeat 重合。
- `who_stretches ∈ {A,B,Both}`：1× 时按 **sounding** BPM 分配差（`rate_a = sounding_bpm_B / sounding_bpm_A`）。2× 见下。
- 入碟结束写 `PerformanceOffset { rate, pitch_semitones }`，传入下一对计划。音高是实际完成值；有移调但没有安全独奏恢复窗口时继续保留，不能假定未被 hold 就一定为零。

## Cue 约束（分轨）

只有 `kind=in|out` 且 `user_set=1` 硬约束。Hot cue **不是** `[first, last]` 最小跨度。

```
covers_user_range_a_out(t_in_a, t_out_a, cues_a):
  无 in/out → true
  仅 out → t_in_a ≤ out ≤ t_out_a   # 可更早进、更晚出
  仅 in  → t_in_a ≤ in  ≤ t_out_a
  两者   → 窗口覆盖 [min, max]

covers_user_range_b_in(t_in_b, t_end_b, cues_b): 对称
covers_user_range = A-out ∧ B-in
```

违反丢弃。v1 右键设 mix-in / mix-out。

Hot 软锚：候选 ±2 bars 内有用户 hot → `phrase_align + 0.05`（cap 1），从不硬过滤。

自动 cue（`user_set=0`）不参与约束。生成在 PR14。

## 2× 速度（默认不是 stretch 到 0.5/2）

```
sounding_bpm = bpm_at_beat(map, beat) * offset.rate
sounding_key = key.shift(offset.pitch_semitones)
r = sounding_bpm_A / sounding_bpm_B
```

`r`、Camelot、`who_stretches` 全部用 sounding。入碟未 hold → offset `{1.0, 0.0}`。禁止用裸分析 BPM/key。

- `|r-1|<8%`：1× stretch，`bar_map=1:1`。
- r≈2 或 0.5：默认 **`bar_map=2:1`，rate≈1**，`stretch_penalty=0`。墙钟：**两个 160 BPM bar 对齐一个 80 BPM bar**（`outgoing_is_double = true` 当出碟更快）。16 master bars @ 160 = 24 s = 8 bars @ 80。
- `literal_half_double`（默认关）才允许 rate∈{0.5,2}，满额 penalty + UI 警告。

## 过渡目录与编译

策略与默认 N：`dry_cut` 4、`phrase_blend` 32、`bass_swap` 16、`drop_cut` 4、`echo_out` 8、`filter_sweep` 16、`loop_construct` 16、`scratch_cut` 4、`break_to_intro` 32、`energy_hold` 16、**`fallback_swap_filter` 16**（一等目标）。

**插值**：xf equal-power；dB / send / rate / pitch 线性；lp/hp **对数频率**线性；loop / insert 阶跃。

完整 bar-offset 节点表（xf/gain/eq/filter/fx/rate/pitch/loop）在主文档 **§7.6**。实现必须按表出金样，禁止“凭感觉写包络”。摘要：

- `bass_swap`：u=0 xf=-0.3 A.low=0 B.low=KILL；u=N/2 xf=+0.2 低交换；u=N xf=+1 A.gain=KILL。
- `phrase_blend`：32 bar 慢 xf，重叠期间固定 B 音高；独奏恢复由独立证据窗口决定。
- `drop_cut`：前 3 bar xf=-1，最后 1 bar 快切 + A echo send。
- `echo_out`：A send 升、gain 降，B 直入。
- `filter_sweep`：A LP 20k→200，B HP 2.5k→20。
- `loop_construct`：B 1–2 bar loop 开在 u=0，u=10 关。
- `break_to_intro`：拉长版 bass_swap。
- `energy_hold`：重叠同 phrase_blend 缩到 16；B rate/pitch 锁 A sounding；写 offset。
- `fallback_swap_filter`：bass_swap EQ/xf **并** filter_sweep 的 lp/hp。

人类三技法 = 参数（`who_stretches` / glide / hold），不是旁路。

## 兼容矩阵

| A 出 | B 入 | 建议 |
|------|------|------|
| Drop-end → Intro | `break_to_intro` / `phrase_blend` |
| Build-end → Drop | `drop_cut` |
| Break → Intro/Break | `bass_swap` |
| Drop → Drop | 仅高分 `drop_cut` |
| 人声 Verse 中 → 人声 Chorus | **丢弃** |
| Unknown | fallback |

## 评分（归一化）

```
score = Σ w_i x_i / 8.7     # 0..1
W = 1.2+1.4+1.0+1.0+0.8+1.3+0.7+0.9+0.4
```

penalty 以 `(1 - p)` 进入分子。阈值 **0.35**。夹具（PR15 必测）：

A 128 BPM Drop-end 8A vs B 126 BPM Intro 9A，`bass_swap` 16，`rate_b≈1.016` →

`1.2+1.4+0.85+0.88+0.64+1.3+0.595+0.9+0.4 = 8.165`；**8.165/8.7 = 0.938** → 选用，不走 fallback。

## 接管

单 lane 取消；pause/resume/skip；换歌则带当前 offset 重算。

2026-10-17 独奏回原调：`plan_with_following_cues` 在重叠曲线确定后，为入碟搜索较弱且无活跃人声的独奏窗口。使用完整 stem 测量、局部网格、持续音和邻域响度判断，不能用 `partial` 字段代替覆盖检查。比较原调距离与下一首所需移调，允许部分恢复；以真实下一对计划确认出场窗口和手工 cue，避免恢复挤掉原有重叠。五次平滑 pitch 曲线位于 filter/EQ/stem 遮掩内部，恢复完成后更新 `t_end_b` 和 `incoming_offset_end`，下一对从该实际状态出发。无窗口保持音高，不在重叠中回调。手动接管相关声部、EQ、filter、速度或 loop 会取消剩余 pitch 自动化；SKIP 保留当前 pitch，避免跳到尚未执行的目标。下一对执行重新读取 snapshot。
