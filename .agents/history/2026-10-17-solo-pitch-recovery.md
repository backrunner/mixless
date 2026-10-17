# 独奏段回原调与下一曲前瞻

## 问题与边界

基线 `2547e9d` 已在重叠中保持匹配后的 sounding pitch，并把它传给下一对。它没有独奏回调，因此连续匹配可能长期承接前曲调性。单曲 ±6 半音的限制不等于整组 set 会回到各自原调。

本次把恢复写入正式计划的独奏尾段。交叠内的 pitch 不变；不把所有曲目无条件归零。`plan_next` 仍是纯配对方案，set 路径 `plan_with_following[_cues]` 加入恢复决策。没有可用窗口时保留已匹配的音高。

## 规划与执行

- 完整 stem 测量、局部可靠网格、段落和邻域响度共同限定窗口。只在较弱的 Intro、Break、Breakdown 或 Outro 搜索 2、4、8 个本地小节，不能把段落标签单独当作允许变调的证据。缺失 bar/grid 覆盖拒绝恢复。`partial` 是现有分析器路径标记，实际完整测量不能因该字段而被误判为缺失。
- 活跃人声和高置信度持续人声音符否决。邻域比较要求至少约 3 dB 响度松弛。普通恢复的音高最大速度为 1 半音/秒；持续器乐只允许至多 1 半音、最大 0.25 半音/秒的较慢变化。这些是保守策略边界，不是听感已验证的阈值。
- 比较原调偏差、下一首所需移调及本次移动幅度，选择完整或部分恢复。下一对从恢复后的 rate/pitch 和源位置重新规划，检查实际下一首移调代价、出场质量、重叠长度及显式 IN/OUT。允许质量相近的较晚出口；不把原来的长重叠退化成硬切来腾出恢复时间。
- filter、EQ 和可用 tonal stems 先衰减，五次平滑 pitch 曲线在中间段完成，再恢复中性参数。鼓组不随 tonal stem 衰减。原来的交叠包络被固定在原结束值，避免新增节点向前插值到重叠中。
- `t_end_b`、时钟、可用源映射和 `incoming_offset_end` 一起更新。此实现把等待窗口和恢复纳入当前计划的独奏尾段；后台可继续预热下一曲，但下一次装载/交接在尾段完成后进行。计划生命周期可能显著长于实际交叠。
- 手动接管相关 EQ、filter、stem、速度或 loop 会取消剩余 pitch 自动化。SKIP 保持已经到达的 pitch，不跳到还未执行的目标。下一对执行读取实际 snapshot，缓存键也包含下一曲完整证据和 cue。
- 没有更换 pitch DSP，也没有在 callback 中加入分析、分配或锁等待；新增 callback 行为仅是 SKIP 时保留音高。

## 数据与实曲

使用正式库的 SQLite 一致性备份，分析版本 20。75 份当前分析均已用于窗口扫描，不能再称为独立 holdout；未覆盖曲风不作泛化承诺。stem 分析版本 2，separator SHA-256 `d05c269d0178d2a72ad484b10b11dd370193fc923201c3b27a99f848745db70a`，notes SHA-256 `2c3c1d144bfa61ad236e92e169c13535c880469a12a047d4e73451f2c059a0ec`。

实曲音频 BLAKE3 均重新核对，文件、原路径、数据库和 WAV 仅保存在忽略的 `target/pitch-recovery-review-20261017/`：

| 录音 | BLAKE3 |
| --- | --- |
| Brain Power (75) | b353093afdf3d9a55e6666237c9c90e513996bef280e66541da5b465dd43279d |
| Angelic Sphere (76) | 9efb6228685886e82116b733aff5032aed5062a13f0510990245be1da0498175 |
| Alone (77) | 911eb5c0d599c2cdbef3f45d522ab2dd47b87f4845bde57fbebf803a3d748abf |
| EON BREAK (58) | 1b07f26deada820de5300983320d9e72662bb6f6b8f3fb6869cf221cde68d366 |

四份歌单的 71 对相邻计划：原有交叠时间点均保持不变，全部满足出入段不重叠和 rate/pitch 承接。开放序列末尾 Angelic Sphere → Alone 入场 +1 半音，在 Alone 源域 136.938–143.698 秒恢复为 0，wall duration 6.713 秒，rate 保持 1.007041。filter 最深约 2800 Hz，low EQ -1 dB，mid/high -0.5 dB，tonal stems -1 dB，结束时中性复位。

循环顺序另检查 29 次交接，闭环承接合法；在这份循环顺序及其后继约束下没有安排独奏恢复。不能把开放序列的正例描述成当前循环歌单必然会执行的效果，也不能为获得更多触发而放开人声或时间窗口保护。

真实 44.1 kHz 源经四 stem PCM 和 48 kHz 离线引擎渲染：

- 单对含独奏恢复共 110.37 秒，peak 0.5072，stem 控制实际生效 1137 blocks，最终 pitch 0、rate 1.00704。前后对照使用相同源时间，改前对照保持 +1 半音；不把该对照当作旧版在线播放的录音。
- 连续四曲验证把 EON BREAK 作为恢复完成后追加的后继。75 → 76 完成源位置 3.815 秒；76 → 77 完成源位置 143.700 秒、pitch 0、rate 1.007041；77 → 58 从该状态接入并完成源位置 52.564 秒。共 646.02 秒，peak 0.44429，三次交接全部完成，逐次断言最终源位置和 rate/pitch，所有样本有限且不越界。
- 这条追加后继验证实际承接；下一曲已知时的前瞻选择另由 12 种调性及显式 OUT cue 回归验证，不能混为同一项证据。

## 检查

- Workspace：540 passed，27 ignored
- Desktop：185 passed，7 ignored
- Mixplan：140 passed，1 ignored
- 新增反例覆盖人声、持续音、强段落、时间不足、缺失证据和弱网格；覆盖共同增益与源时间平移、手动调音基准、不同后继调性和硬 OUT cue
- 引擎在两个物理 deck 方向验证正常完成、mask 接管和中途 SKIP，不发生目标 pitch 跳变
- Release callback budgets：5 passed；双 deck、pitch、FX、录音和 cue 的 stems 用例 p99 1.030 ms，预算 2.667 ms
- 双语文档：24 pages、132 search records，0 errors、0 warnings

环境为 macOS 27.0.1 (26A434)。音频证据为离线渲染，不代表已完成主观试听或真实设备监听；当前播放中的原生应用没有被重启。`./dev.sh --build` 成功。包为 dev 0.1.0-beta.4，构建标识 `v0.1.0-beta.4-10-g2547e9d-dirty`（本次提交前构建），可执行文件 SHA-256 `dfe42ddfab1f69dba01826b18ede264ab36a7b700cf5727565ce5e9bf547b6a1`。
