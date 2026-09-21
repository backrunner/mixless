# 04 — 离线分析

开发和评审分析规则时使用 [mixless-audio-analysis](skills/mixless-audio-analysis/SKILL.md)。故障曲目用于回归；不能仅凭这些曲目、固定小节数或规划成功率证明泛化。

预分析是 library 一等公民。**v1 入队来源**：显式 `ImportFiles`，以及获取层写入 `acquired/` 的文件。Watch-folder = **P1**。播放只读产物。分析 decode 与播放 cache **独立**（同一文件，分析侧降到 22.05 kHz mono）。`suspect` 获取文件照常分析，但不自动进 Automix。

## 寻址与产物

```
hash = blake3(file_len || mtime_secs || first_64k_bytes || last_4k_bytes)
```

**不含 inode**。复制且头尾/mtime/size 相同 → 命中。产物：`$APPDATA/mixless/analysis/{hash}/`。

## 队列

优先级：正在 load 的曲 > playlist 下 2 首 > 刚导入 > 全库后台。

- rayon：`max(1, num_cpus-2)`；播放中减半。
- 可取消；`RetryAnalysis` 再入队一次。
- 目标：4 分钟 44.1 k stereo，无 ONNX < 8 s。结构另计，允许 `partial`。

## Metadata

`lofty` / `symphonia`。损坏标签回退文件名。封面 `artwork/{hash}.jpg`。

## BPM / Beat / Downbeat

步骤：flux → tempogram 70–180（含 half/double）→ 全局 BPM → 8–16 s 滑窗 + PELT → 低置信折叠为单 BPM → Ellis 2007 beat → 4/4 vs 3/4 downbeat。

`ANALYSIS_VERSION=15`：16 beat 局部窗口里与全局值偏差 ≥1.5% 的估计，只有相邻窗口同向且各有置信度 ≥0.55 才保留为真实变速（单个离群窗口或 2/3、3/4、4/3、3/2 节拍别名一律回退）。过滤后全部一致的曲目按恒定速度处理：beat 按单周期单相位生成，pulse_confidence 取全局测量与局部测量的较大值，安静 break 不再把网格可靠性拖到零。

`ANALYSIS_VERSION=16`：onset flux 先做均值去除再自相关；候选 tempo 增加 3/4 与 4/3 别名，并由 comb（1×/2×/4× lag）自相关对所有候选做门控与排序——半速曲目不再被读成倍数别名（实测 92 BPM 曾读为 122.66）。`driving` 状态抑制短于 4 小节的响度孤岛，不再产生 2 小节的假 drop。结构切点在 ≥3 个可移动切点相位一致时对齐 4 小节乐句网格；静音边缘的切点固定不动。新增软 build-up 标签：段落首尾对比 onset density ×1.25、RMS ×1.15、spectral tilt +2 dB，长度 ≤16 小节且不能是第一发声段。vocal 描述符按 3 小节平滑，权重降为 0.5。

失败：120 均匀 grid，`confidence=0`。

`ANALYSIS_VERSION=17`：短停顿不能单凭低能量成为出点。相邻 Drop/Chorus 先合并为连续高潮；两个持续高潮之间不超过 6 个局部小节的 Break/Breakdown/BuildUp/Silence/Unknown 保守视为待恢复的停顿，自动 Out、mix region 与规划器共用判断，完整 8 小节 breakdown 仍可混出。Build-up 允许末尾至多 2 小节静音，且在结构上保留至后续 drop；确认蓄势时不把最后一小节的单次鼓声回归作为上升证据，补充持续高频上升和低频退让的证据。自动 In 排除已标记 Silence 的小节。重分析同步刷新自动 cue，保留用户 cue。实曲与验证见 [停顿分析复核](history/2026-09-21-pre-drop-pauses.md)。

`ANALYSIS_VERSION=18` 取代 v17 的固定 6 小节停顿判据：间隔不得超过较短相邻高潮时长的一半，内部已有可信乐句边界则保留为独立段落，并且需要对两侧都测得至少约 3 dB 响度退让、6 dB 低频退让、1/3 起音活动下降和后续恢复。测量按实际重叠时长加权，缺失覆盖不确认停顿。时长只限制候选，不能独立定义语义；这些对比阈值仍是需校准的工程启发式，不是所有曲风的定律，也不保证所有 8 小节 breakdown 都可混出。新增变换与难反例验证，见 [泛化复核](history/2026-09-21-analysis-generalization.md)。

规划器必须用 `bpm_at_beat(map, beat)` 读 **局部** `TempoSegment.bpm`，不是只读 `global_bpm`。

P1：手拖 grid。

## Key

全局 chroma + KS 24 模板。一个主调 + Camelot。`alt_key` 不进 UI / 主分。失败 `Unknown`。

## Waveform / 按小节频谱

着色：`<150 Hz` 暖红，`150–2 kHz` 绿，`>2 kHz` 蓝。Phrase：每 bar 一列 32–64 bin。

`waveform.bin`：

```
magic b"MLWF" | version u16 | sample_rate u32 | n_layers u16
layer[]: kind, hop_frames, n_bins, offset, nbytes
payload: IEEE f16 [rms, peak, low, mid, high] × n
```

前端必须 **f16 → f32**（或 `RGBA16F`），不能当 `Float32Array` 直接用。

## 结构

`Silence | Intro | Verse | BuildUp | Drop | Break | Breakdown | Chorus | Bridge | Outro | Unknown`

当前默认使用 2/4/8 小节多尺度音色变化与重复关系，生成带置信度的 `phrase_boundaries`；弱起和乐段偏移按实际 downbeat 处理。v16 起，≥3 个可移动切点对同一相位达成一致时，切点吸附到 4 小节乐句网格，修正系统性偏移；软 build-up 由首尾能量/密度对比标记（见 BPM 节版本说明）。macOS 12+ 使用 Apple Sound Analysis 系统分类器补充人声证据，2 秒分块预算、单任务、超过 10 分钟跳过。未接入自定义 ONNX 结构权重；未知结构继续为 `Unknown`，分析保持 `partial`。部署与实测见[分析增强参考](reference/ANALYSIS_ENHANCEMENT.md)。

自动 cue 生成在本阶段（M4 / PR14），`user_set=0`，不覆盖用户垫。

## 逐 bar 特征

`rms, crest, low/mid/high_db, chroma[12], chord, local_key, onset_density, kick_salience, hat_salience, vocal_presence, energy_slope, section`

全部进入 `protocol::TrackAnalysis`。

## 入/出点候选

出点：Drop→Break 前 8–16 bars；Outro 下降；Chorus 结束；**显式 Out**。

入点：Intro 后半；第一 Drop；Build→Drop；Break 重启；**显式 In**。

Hot cue **不**进硬候选。top-K=8。

## 失败回退

| 阶段 | 回退 |
|------|------|
| decode | job failed，不可上碟 |
| tempo | 120 均匀 |
| key | Unknown |
| waveform | 仅 RMS 单色 |
| structure | 能量切段 Unknown |
| bars | 缺省 0，planner fallback |
