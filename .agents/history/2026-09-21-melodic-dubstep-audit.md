# 2026-09-21 Melodic Dubstep 审计与 AutoMix 修订

对 "Melodic Dubstep Set 260112"（11 曲）在 `ANALYSIS_VERSION=15` 下的审计暴露节奏误读、假高潮、切点偏移与过渡规划缺陷，随后在库副本上完成 v16 重分析与规划器修订。方法沿用库副本离线审计（`reanalyze-copy` / `automix-audit` / `live-moves-audit` / `render-audit`），不改线上库。

## v15 缺陷

| 类别 | 实例 |
| --- | --- |
| 节拍别名 | Better Here With You 读为 122.66 BPM，真实约 92 BPM（4/3 别名；独立 comb 自相关复核确认） |
| 假高潮 | 2 小节 "drop"：Idle World 19.1–21.9、Hollow 11.6–14.9、Take You Home 164.2–167.3 |
| 切点偏移 | 系统性提前约 1 小节：Moments 11.22/164.83/267.23 → v16 修正为 12.83/166.43/268.82 |
| 标签缺失 | Idle World 104.6–112.9 的 build 被标为 Breakdown |

v15 计划后果：Idle World→Find My Way 恰在 112.9 的 drop 起点 DryCut 进入 intro；Find My Way 在自身 184.7 drop 前 4 小节退出；Stay With Me 在 112.0 drop 前 1 小节退出；Hollow 于 240 s 曲目的 53 s 处退出；Who Am I Now→Emotional 使用 54 小节 FilterSweep，两首副歌整段叠唱。

## v16 分析与规划修订

- 分析：onset flux 均值去除后自相关；comb（1×/2×/4× lag）对含 3/4、4/3 别名的 tempo 候选门控排序；`driving` 抑制 <4 小节响度孤岛；≥3 个可移动切点相位一致时吸附 4 小节网格；软 build-up 标签；人声描述 3 小节平滑、权重 0.5。
- 规划：drop 前 8.5 小节内的退出仅允许 build-up 交换（入碟高潮落在交接点）；入碟高潮不得在叠混重叠内开始；FilterSweep ≤16 小节（纯打击乐除外）；stem 层叠 8–32 小节；`major_peaks`（≥7.5 小节、中位 RMS 距最响 −1.2 dB 内）驱动高潮罚分与 40% 提前退出罚分；出点候选从 25%（上限 60 s）起。
- 新技法：Spinback（加速回拉停碟，入碟落在自身高潮起点）、LoopOut（出碟循环尾句、声部/EQ 抽离、中点低频交接），与既有技法按 `(曲对, 退出, 进入)` 哈希确定性轮换，均非唯一选择。
- Live moves：过渡期外的滤波渐升与鼓声部抽离（`live_moves` Off/Subtle/Active），经 `Engine::perform` 下发不触发计划接管标记，用户触碰即交还 lane。

## 结果

两条歌单共 24 对全部成功（0 failed）。

| 审计 | BassSwap | DryCut | FilterSweep | DropCut | Spinback | LoopOut | PhraseBlend |
| --- | --- | --- | --- | --- | --- | --- | --- |
| audit-c.txt | 8 | 7 | 7 | 1 | 1 | 0 | 0 |
| audit-c-stems.txt | 7 | 7 | 6 | 1 | 1 | 1 | 1 |

Spinback 在 Who Am I Now→Emotional 于 171.6 s 触发，入碟落在 Emotional 66.3 s 的副歌；LoopOut 在 We Are The Energy→Power（stems 就绪）触发，出碟 121.4–132.4 s 循环 2 小节。引擎实际渲染（`/tmp/mdub/renders`）峰值 0.42 / 0.41，无削波，回拉方向经源帧递减验证。

Live moves 抽查（Subtle）：Moments 96.0–102.4、Idle World 107.4–112.9、Hollow 59.6–66.2 的滤波渐升均落在实测 build→drop 边界；鼓抽离均在 ≥16 小节主要高潮内的 8 小节边界。

## 边界

规划级审计，未做主观试听；本地无 kpop/jpop/hyperpop 素材，该类鲁棒性来自构造约束（乐句吸附需 ≥50% 相位一致、comb tempo 门控）而非实测。
