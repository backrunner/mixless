# 04 — 离线分析

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

失败：120 均匀 grid，`confidence=0`。

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

ONNX 优先；失败能量切段 + `Unknown`。Chorus 能量峰视作 drop-equivalent。

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
