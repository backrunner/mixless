# 2026-09-14 AutoMix 网格拟合与人声门控验证

本轮修复两类 AutoMix 规划缺陷：单个错误的局部 BPM 窗口相位偏移整条 beat 网格，以及出碟人声未停时允许硬切/桥接。方法为库副本上的离线审计，不改线上数据库。

## 审计方法

- `cargo run --release -p mixless-library --example automix-audit -- LIBRARY-COPY.db VERSION`：对库内列表逐对运行 `Planner`，打印每对的过渡模式/策略、长度、得分、退出/进入点人声活动和 stem 证据，并汇总失败数、各策略计数与“短过渡切在仍活跃人声上”计数。`MIXLESS_AUDIT_STEM_PLAYBACK=1` 时向 `PlannerOptions.stem_playback` 传真。
- `cargo run --release -p mixless-library --example reanalyze-copy -- LIBRARY-COPY.db OLD_VERSION NEW_VERSION`：对副本中每个存在的文件重跑 `Analyzer::analyze_track`，旧版本里的 stem 证据用 `Analyzer::apply_stems` 复挂（不重新推理），按新版本号 `save_analysis` 写回。本库 16 首全量重分析用时 17.7 s。
- 副本由 `sqlite3 "file:...library.db?mode=ro&immutable=1" ".backup /tmp/xxx.db"` 生成；线上库始终只读。

## 网格修复前后（ANALYSIS_VERSION 14 → 15）

修复内容：16 beat 局部窗口的离群 BPM 估计仅在相邻窗口同向且各有置信度时保留（持续变速判定），节拍别名一律回退；过滤后全一致的曲目按恒定速度处理（单周期单相位、全局 pulse_confidence）。否则一个错误窗口会把其后所有 beat 相位偏移约一拍，并导致 `grid_reliable` 拒绝整个混音窗口。

| 曲目 | 修复前（可靠/弱，离群 BPM） | 修复后 |
| --- | --- | --- |
| Underwater | 39 段中 7 可靠 8 弱；165.0@0.96、178.5 | 39/39 可靠、0 弱，全程 174.0 |
| Feel This Good | 58 段中 2 可靠 15 弱；179.7、171.3、置信度 0 | 58/58 可靠、0 弱，全程 174.0 |
| Lost & Found | 43 段中 9 可靠 20 弱；170.6、170.9、多处 0 | 44/44 可靠、0 弱，全程 174.0 |
| Crystal Horizon | 33 段中 30 可靠 3 弱；尾部 166.6、153.5 | 33/33 可靠、0 弱，全程 171.0 |

全局 BPM 并非全为 174：Crystal Horizon 171.0、Fresh 176.5、Loving, Loathing 172.0，为各自真实拟合值。

## AutoMix 审计结果（playlist_G7SAZ_DnB_Set，14 对）

| 运行 | 失败 | BassSwap | FilterSweep | PhraseBlend | DryCut | 短过渡切在活跃人声 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| v14 基线（人声门控前） | 3 | 4 | 1 | 0 | 6 | 2 |
| v14 + 人声门控（stem 关） | 3 | 4 | 1 | 0 | 6 | 1 |
| v15（stem 关） | 0 | 3 | 9 | 2 | 0 | 0 |
| v15 + `stem_playback` | 0 | 3 | 8 | 3 | 0 | 0 |

人声门控（`voice_cut_safe` 边界豁免加严 + `voice_released` 门控）把 `Let me bloom -> Divided Sky` 的 145.1 s 人声处瞬切改到 203.1 s Outro。`voice_released` 按释放语义判定：唱到边界前一瞬才停算合法切点（build-up 收尾秒切），人声延续过切点才阻止；用户显式 OUT Cue 例外。v15 网格修复后全部 14 对产出 BeatBlend（v14 时仅 5/14），三对原先失败的组合均落到 4–16 小节混合；开启 `stem_playback` 后 `Let me bloom -> Divided Sky` 由 FilterSweep 回退变为 `requires_stems` 的 16 小节 PhraseBlend 声部层叠计划，是真实库上首个 stem-layer 计划。

## 推理性能测量（弃用与保留）

CoreML 执行后端实验（`ort` `coreml` feature，环境变量启用）在相同 60 s 片段上比 CPU 4 线程慢约 86 倍（1354.8 s vs 15.67 s），输出与 CPU 一致；已移除该代码路径，保留环境变量线程覆盖。

`MIXLESS_ORT_THREADS` 对同一 60 s 片段的 CPU 推理（冷缓存）：

| 线程 | 用时 | 峰值 RSS |
| ---: | ---: | ---: |
| 2 | 19.04 s | 3.80 GiB |
| 4 | 15.67 s | 4.82 GiB |
| 6 | 14.83 s | 5.29 GiB |
| 8 | 14.96 s | 5.17 GiB |

默认线程取 `min(4, 可用并行度-2)`，4 线程较原默认约快 18%，6 线程后饱和。

## 验证边界

以上为单一 16 首 DnB 曲库上的规划器级审计数据，衡量的是计划选择与网格/人声证据门控，不是试听验收；混合听感、声部包络的自然度和结构标签的正确性均未由此证明。桌面 `auto_repeats_playlist_and_can_resume_from_a_manual_overlap` 在未改动的 HEAD 上以相同方式超时，属本机 20 s 截止的既有抖动。
