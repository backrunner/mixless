# AutoMix DnB 调试记录 · 2026-09-12

本次使用导入的 DnB Set 全部 14 首原始音频重新分析，检查顺序相邻曲对及最后一首回到第一首。分析版本升级为 9；旧版本缓存会在准备歌曲时重新分析。原始音频、数据库和个人文件路径未加入仓库。

## 行为与实现

- 播放按钮图标从 14 px 放大至 21 px。AUTO 从空唱盘启动时载入队列第一首可播放歌曲，并按音频时钟渐入播放；失效文件会被跳过。
- 用持续低频、响度和起音密度共同识别驱动段，使用进入/退出不同阈值抑制抖动。短 fill / bass mute 不会自动结束 drop；低频退去后的 break 和重新积累能量的 build-up 分开识别。
- 相邻 Drop / Chorus 变化段合并为需要完整播放的区间。自动混音窗口不能与这些区间重叠；先播完当前播放过程中的首个可用完整 drop，后续已经进入的 drop 同样受保护。自动入点也不能落在连续 drop 的中间。用户明确指定的 IN / OUT 保持优先。
- break 内优先评估与下一首 intro / build-up 的 8、16、24 bars 叠混，并保留其他有乐句依据的时长。24 bars 是本次产品策略，非所有 DJ 过渡的固定长度。节拍、和声、人声冲突等约束仍先于评分。
- build-up 结束切入下一首 drop，必须匹配两侧实际段落边界，保留 build-up 到最后，并避免 echo 尾音遮住新 drop。该技法不同时叠放两条鼓轨，无需强制两首同速。
- 没有可用 break 的末尾 drop，后备路径保留原曲至结束，再启动下一首；仅在最后约 40 ms 防止切换爆音。渲染测试覆盖 EOF 边界上的新唱盘启动。
- 弱证据的局部 BPM 不再将稳定全局网格拉到 70 / 116 等别名值。真正具有高置信度的速度变化仍保留。

## 真实音频结果

以下时间来自算法测量，不是人工标注的乐段真值。三首歌曲的数值特征已保留为无音频的回归夹具，检查持续段落而非锁死边界的小数点。

| 歌曲 | 旧分析问题 | 重新分析的关键结果（秒） |
| --- | --- | --- |
| Punching Holes | 100–142 秒附近标为 unknown | Drop 56.21–100.36；随后 Break / BuildUp；下一 Drop 141.75 开始 |
| In The Echo | 约 22–179 秒整体标为 Drop | Drop 38.33–82.48；Break 82.48–93.51；随后 BuildUp；下一 Drop 129.38 开始 |
| Power | break 与 build-up、drop 混在一起 | Drop 67.94–134.16；随后 Break / BuildUp；下一 Drop 167.27 开始 |

14 个曲对都能规划或后备交接，没有混音窗口或切换点截断已识别的连续 drop。策略分布为 9 个 DropCut、3 个 EchoOut、1 个 FilterSweep、1 个末尾 DryCut。

| 出曲 → 入曲 | 出曲过渡窗口/切换点（秒） | 入点（秒） | 技法 |
| --- | --- | --- | --- |
| Loving, Loathing → Take Control | 240.42 | 87.36 | DropCut |
| Take Control → Let me bloom | 189.43–194.95 | 22.71 | EchoOut |
| Let me bloom → Punching Holes | 122.03 | 56.21 | DropCut |
| Punching Holes → Divided Sky | 141.75 | 42.45 | DropCut |
| Divided Sky → Flicker | 199.66–202.42 | 22.16 | EchoOut |
| Flicker → Lost & Found | 154.56–158.69 | 0.07 | EchoOut |
| Lost & Found → In The Echo | 165.62 | 38.33 | DropCut |
| In The Echo → Orbit | 129.38 | 76.95 | DropCut |
| Orbit → We Are The Energy | 236.93 | 0.00 | 末尾 DryCut |
| We Are The Energy → Power | 169.31–174.83 | 48.63 | FilterSweep |
| Power → Feel This Good | 167.27 | 67.46 | DropCut |
| Feel This Good → Underwater | 202.66 | 75.58 | DropCut |
| Underwater → Heaven | 166.64 | 67.65 | DropCut |
| Heaven → Loving, Loathing | 164.20 | 134.35 | DropCut |

这组音频仍有半拍 BPM / 拍号起点歧义，尤其 Flicker 的全局 116 BPM 别名尚未消除；所有曲对未达到长鼓轨叠混要求，因此实际选择短混音或原速切换。8 / 16 / 24 bars 的长混音选择由可靠网格夹具验证，不能把这些测试当作本次真实 set 已完成长混音的听感证明。全曲单一 Drop 标签也不能提供可验证的完整段落边界；未知结构仍采用保守后备策略。

## 验证与复现

macOS 27.0（26A428），离线引擎 48 kHz / 256 frames；未打开物理音频设备。14/14 曲对实际渲染完成，新唱盘继续播放，样本全部有限，最大输出峰值 0.98。工作区测试 191 项通过（8 项按默认配置跳过），桌面测试 40 项通过，开发版构建通过。自动渲染检查有限样本、输出峰值、计划完成及新唱盘持续播放，不代替人工试听或 UI 动画测量。

```sh
rtk cargo test --locked --workspace --exclude mixless-desktop
rtk cargo test --locked -p mixless-desktop
rtk proxy ./dev.sh --build
```

设备无关的工具：`structure-audit INPUT.json OUTPUT.json [AUDIO_PATHS.json]` 接受 TrackAnalysis 数组，可从既有数值特征重分类，也可重新分析音频；`render-audit ANALYSES.json AUDIO_PATHS.json OUTPUT_DIRECTORY` 为全部相邻曲对输出包含前后文的试听 WAV，并检查引擎交接。已有 WAV 不会被覆盖。

工程拆分：`smooth/` 分离候选检查、时序计算、评分、控制曲线；`features/` 分离节拍估计与测试；`structure/` 分离持续动态判断与真实数据回归；桌面 `state/` 分离 AUTO 会话、播放控制、拖拽、载入、导入和结果轮询。未向音频回调加入分析、文件 I/O 或规划工作。

## 技法参考

[Crossfader 的 DnB 演示与拆解](https://wearecrossfader.co.uk/blog/how-to-mix-drum-bass-free-dj-tutorial/)讨论乐句、EQ、编排与 double drop；[DJ.Studio 的 DnB 技法说明](https://dj.studio/blog/mix-drum-and-bass)说明 breakdown 交接、quick cut，以及避免同时堆叠两条低频主线。本次据此保留分阶段 EQ 交接和两类不同时长的过渡，具体保护条件与评分属于本项目的工程决策。
