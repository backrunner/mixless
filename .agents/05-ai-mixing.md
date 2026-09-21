# 05 — AI Mixing

两阶段。规划绝不进 callback。生产路径只规划 **下一对**，并可预计算再下一对。`plan_playlist` 仅测试。

## 2026-09 平滑策略实现补充

默认 `PlannerOptions.smooth=true`：逐对迁移 BPM 和音乐调性，不锁整张歌单。可靠网格、兼容 sounding key、低前景冲突时使用 `BeatBlend`；共同 BPM 走五次平滑曲线，最大对数变化率约 0.35%/s，低频在重拍附近一个拍内按等功率交接。半倍／双倍仍使用 2:1 拍映射。`energy_hold` 可保留偏移，下一对读取实际 rate/pitch。

不满足长混条件时使用 `PhraseBridge`：出碟末段滤波／轻 Echo，在其乐句结束重拍启动原速、原调的入碟。出碟干声在重拍前约 40 ms 内淡出，入碟第一拍直接接管；不把两条不兼容的鼓点或主旋律长时间叠加。用户 In/Out、前景 Verse → Chorus 禁区和音频边界约束同样适用。已测出的静音不能成为接入点。

目前支持十一种策略；其中十种目录表及归一化评分保留在 `smooth=false` 路径，金样继续验证，DryCut 仅作为平滑路径的安全策略。平滑路径使用 DryCut、BassSwap、PhraseBlend、EnergyHold、EchoOut 和受结构约束的 DropCut；ScratchCut 只有完整 cue 证据才会进入。`who_stretches`、literal 和其他显式目录选择属于目录路径。完整现状、试用和验收限制见[当前 AutoMix 参考](reference/AUTOMIX.md)；主设计 §7 同步记录本补充。

2026-09-09 修订：低频交接搜索共同 downbeat，优先共同乐句，不再固定中点；有底鼓时一拍交接，两侧底鼓都弱时渐进叠混。入碟高频轻度渐入，局部 RMS 按真实窗口时长加权，正增益检查入点后完整峰值覆盖。具备整段保守打击乐证据时允许跨调 layering；缺失证据不授权。连续超过一拍的双前景不能被长窗口平均值掩盖。实现、八项新增回归、教学来源和真实样本渲染边界见 `AUTOMIX.md`。

2026-09-09 性能动作修订：Loop Roll 只在稳定 grid、连续 kick、低 vocal 风险和乐句尾部建立 1／2 小节真实 loop，并在释放前协同 filter／echo。`ScratchCut` 只接受可靠 grid、出入边界、强 kick、出碟低人声和入碟用户 Hot cue；生成半小节三阶段 `ScratchOp`（touch、半拍回拉峰值、slip release），通过引擎现有 jog target 执行，释放后恢复原播放头。任一条件失败都回退安全桥接，禁止把随机 jog 或长时间搓碟自动化。

FX 决策层位于平滑规划器之前：先做 grid、phrase、vocal、kick、harmonic 和 cue 的硬否决，再选择 DryCut、EchoOut、FilterBridge、DropCut、LoopRoll 或 ScratchCut。干净的和声／节奏交接不叠 FX；Outro/Break 的尾句优先 Echo；调性冲突的短桥优先 Filter；FX 不能修复不可靠节拍或连续双前景。

2026-09-21 修订：`ANALYSIS_VERSION=16` 修正半速曲目误读（comb 门控与 3/4、4/3 别名候选）、抑制 <4 小节假高潮、结构切点吸附乐句网格并新增软 build-up。规划侧禁止无交换的 drop 前退出与叠混内入碟高潮，FilterSweep 限 16 小节，`major_peaks` 驱动高潮罚分；新增 Spinback 与 LoopOut，技法按 (曲对, 退出, 进入) 哈希确定性轮换。过渡期外新增 live moves（滤波渐升、鼓声部抽离，用户触碰即交还该 lane）。细则与审计见 [AUTOMIX 参考](reference/AUTOMIX.md) 与 `history/2026-09-21-melodic-dubstep-audit.md`。

## Mix 时钟

- Master = **出碟**，除非策略声明（`energy_hold` 在 xf 过 0 后把 master 交给入碟，仍用出碟 sounding 网格）。
- `length_bars` 按 master grid。
- 入碟起始源帧：rate 折线之后，选定 downbeat 重合。
- `who_stretches ∈ {A,B,Both}`：1× 时按 **sounding** BPM 分配差（`rate_a = sounding_bpm_B / sounding_bpm_A`）。2× 见下。
- 入碟结束写 `PerformanceOffset { rate, pitch_semitones }`，传入下一 `plan_pair`。入碟未被 hold 时 offset = `{1.0, 0.0}`。

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
- `phrase_blend`：32 bar 慢 xf，可选 B pitch glide 回原调。
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
