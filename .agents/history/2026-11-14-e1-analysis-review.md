# E1 歌曲结构、cue 与 AutoMix 复核

## 结论与范围

E1 当前顺序确实出现多次曲尾交接。问题涉及结构表示、cue 候选和入点选择的连锁影响，不是简单把自动 Out 提前就能解决。当前实现对持续鼓点、响度变化小的器乐编排，以及带人声的低能量段，仍明显依赖 EDM 的高潮／恢复模型。

本轮只审计并保存证据，未修改生产算法、正式数据库、用户 cue、歌单或 Best order。用户要求保护其他目前表现较准的 set；下面的改进方向尚未实现或通过听感验收。

## 输入与复现

- 源码：`7966a1075f0933f6781e0e2e05ffdb169bdc9da4`，开始时工作区干净。
- `ANALYSIS_VERSION=27`，`ORDER_VERSION=28`。
- 正式库通过 SQLite backup API 读取已提交 WAL 数据，审计工具只操作副本。
- E1：6 首，全部 v27，全部有 v2 stem 测量，全部 cue 为自动 cue。
- 6 个源文件重新计算内容哈希，全部与库记录相符。
- 6 首重新解码分析，再附回原有 stem 测量一次；tempo、bars、sections 与存储结果逐首完全相同。当前生成的 cue 与存储 cue 也相同。因此下面的问题不能归因于旧版分析或陈旧 cue。
- 未重新运行分轨模型；源文件一致性不等于重新验证分轨或音符转录的准确性。
- E1 的实际 `pulse_confidence` 最低值均超过 0.70；不能把较低的 `TempoSegment.confidence` 当作规划器使用的时钟可靠度。这里也没有人工确认 160/180 BPM 与 80/90 BPM 的音乐拍层级。
- 使用当前源码编译的 `mixless-mixplan --features trace --example automix-audit`，传递实际入点、上一交接结束位置、rate/pitch，并包含一首前瞻。所有输入均没有手工 cue。
- 固定当前歌单顺序，额外接上开头两首，以检查循环回到第一首及其后续状态。不是重新运行 Best order，也不是已安装 app 的实时播放日志。
- 分别运行混合音频模式及 `--stems` 模式；后者是允许规划使用分轨的模拟，不证明运行中的 app 已具备 stem PCM。

本地原始证据位于忽略目录 `target/e1-review-2026-11-14/`：`library.db`、`e1.db`、`manifest.json`、`identity/sources.jsonl`、`e1.json`、`fresh.json`、`peaks.jsonl`、`plans-{mix,stems}.json`、对应日志及 `independent-*.json`。不提交个人数据库、完整分析或源音频。

主要复现命令（先创建一致性副本和 E1 导出）：

```sh
rtk cargo build --locked --release -p mixless-library --example reanalyze-copy
rtk cargo build --locked --release -p mixless-mixplan --features trace --example automix-audit
rtk cargo build --locked --release -p mixless-analyze --example analysis-evidence-audit --example cue-preview
rtk proxy target/release/examples/reanalyze-copy target/e1-review-2026-11-14/e1.db 27 27
rtk proxy target/release/examples/analysis-evidence-audit target/e1-review-2026-11-14/e1.db target/e1-review-2026-11-14/identity
rtk proxy env AUDIT_PLANS_OUT=target/e1-review-2026-11-14/plans-stems.json PAIR=0 target/release/examples/automix-audit target/e1-review-2026-11-14/e1-loop.json --stems
```

两个 crate 都有名为 `automix-audit` 的 example；这里必须使用 mixplan crate 的 JSON 输入版本，避免构建覆盖后误用 library crate 的数据库输入版本。

## 全部 E1 衔接

下表为开启分轨规划的源时间，非整组播放墙钟。括号为源文件总长。最后一行是循环衔接；不循环时最后一首自然播完不能算作规划缺陷。

| 出曲 → 入曲 | 出曲交接区间／总长 | 入曲起点 | 实际计划 |
| --- | --- | --- | --- |
| Time To Pretend → A Moment Apart | 275.29 s（314.61 s） | 121.66 s | 0 bar DryCut |
| A Moment Apart → Body Language | 231.66 s（234.06 s） | 0.04 s | 0 bar DryCut，接近曲尾 |
| Body Language → Amnesia | 63.48–107.62 s（201.43 s） | 121.41 s | 32 bar PhraseBlend，使用分轨 |
| Amnesia → I'm Not Alone | 202.94–206.94 s（206.94 s） | 150.71 s | 1 bar DryCut 兜底，到文件末尾 |
| I'm Not Alone → Teenage Dreams | 210.71 s（217.18 s） | 31.36 s | 0 bar DryCut，接近曲尾 |
| Teenage Dreams → Time To Pretend | 298.03–340.70 s（343.01 s） | 35.29 s | 32 bar StemSwap，最后约 45 秒才开始混合 |

前五对有三对接近文件末尾才退出。循环的第六对虽然也结束很晚，但有 32 bar 重叠，不能和曲尾瞬切混为一谈。关闭分轨后，Body Language → Amnesia 变成 1 bar DryCut，循环衔接变成 EnergyHold；上述三个晚退结果不变。

## 发现

### P1：长时间持续推进被表示成一个必须完整播放的高潮

`structure.rs` 的边界检测比较 2/4/8 bar 的平均 19 维描述符，语义部分以 RMS、低频、kick、onset 形成的 driving 状态为主。重复判断比较整段平均描述符，不保留和弦、旋律或节奏事件的时间顺序。局部均值相近、鼓点持续的不同乐句可能没有足够 novelty；有声部退出再返回的段落才较容易通过现有恢复判据。

Teenage Dreams 的实测输出：

- 0–31.36 s：BuildUp。
- 31.36–191.36 s：Drop，连续 160 秒。
- 191.36–319.37 s：Drop，连续 128 秒。
- 319.37–324.70 s：Drop。
- 324.70–340.70 s：Outro。
- `relative_recoveries=[]`；`peak_ranges` 将上述 Drop 合并成 31.36–324.70 s。
- 已测出的 191.36 s 边界有 novelty 0.245、confidence 0.673，但 `main_peak` 仍要求到 324.70 s；`drops::owed_peak` 对连续 Drop 也会扩展到整段末尾。

这解释了为什么大量周期性 phrase 标记没有带来中间出口：周期标记的 novelty 为零，不能单独授权退出；已存在的内部边界又不自动结束整个受保护高潮。不能通过“每 8/16 bar 都可退出”绕过它，否则会损坏其他 set 的高潮完整性。

Time To Pretend 还有一个不同因素：第一轮 47.29–167.29 s 没有进入 `major_peaks`，唯一 major peak 是更响的 215.29–263.29 s。当前相对响度规则把前一轮降级，不能只用段落时长解释其晚退。

位置：`crates/mixless-analyze/src/structure.rs:19`、`:54`、`:217`；`crates/mixless-protocol/src/phrases.rs` 的 `peak_units/major_peaks/main_peak`；`crates/mixless-mixplan/src/drops.rs:29`。

边界：这些是可重现的表示与策略限制，尚不是逐段人工音乐标注。尤其不能据此宣称 191.36 s 已经是安全的旋律出口。

### P1：节奏／能量状态优先于曲式语义

分类顺序先判断 `!driven && earlier_drop && later_drop` 为 Break/Breakdown，或 `!driven && earlier_drop && !later_drop` 为 Outro，之后才考虑重复、人声、Chorus、Verse。另一条低 RMS / 低 kick 分支也在 Verse 之前。器乐重复高能段主要落入 Drop；虽然协议有 Bridge，当前 analyzer 没有生成 Bridge 的分支。

E1 的具体证据：

| 曲目／区间 | 当前标签 | 该区间 bar 人声置信度中位数 |
| --- | --- | --- |
| I'm Not Alone，0–45.71 s | Breakdown | 0.96 |
| I'm Not Alone，150.71–180.71 s | Breakdown | 0.93 |
| Body Language，107.62–143.48 s | Break | 0.91 |
| Body Language，143.48–154.51 s | Break | 0.99 |
| Amnesia，117.27–133.82 s | Breakdown | 0.87 |
| Amnesia，133.82–144.86 s | Breakdown | 0.98 |

有人声的 break 本身不一定错；问题是现有判定会在这些分支直接跳过曲式语义。标签同时又被拿来生成恢复窗口、入点和高潮保护区，导致“能量退让”和“乐段功能”混用。不能只把这些区间批量改为 Verse；应保留独立的人声、节奏、编排与功能证据，并用标注验证分类。

位置：`crates/mixless-analyze/src/structure.rs:273`；消费方包括 `crates/mixless-mixplan/src/recovery.rs:6`、`phrasing.rs`。

### P2：已识别的边界被 cue 的标签过滤丢弃

`automatic_cues` 在检查变化强度前直接排除相同标签的相邻段，并只允许特定的 next label；Verse 不在一般 Hot cue 候选内。两个不同的 Verse 或两个不同的 Drop 即使有充分变化证据，也不能成为 Hot cue。相比之下，结构检测器明确保留相同标签的不同段落。

例如：

- Amnesia 56.58 s 的 Verse → Verse，novelty 0.473，未生成 Hot cue。
- Amnesia 133.82 s 的 Breakdown → Breakdown，novelty 0.578，未生成 Hot cue。
- I'm Not Alone 120.71 s 的 Drop → Drop，novelty 0.291，未生成 Hot cue。
- Amnesia 第一轮 Drop 67.62 s 的 novelty 为 0.217，低于 cue 的统一 0.28 门槛，也没有 Hot cue。
- Teenage Dreams 只有 In 0、Out 324.70 s；其 191.36 s 的 novelty 0.245 同时受标签和阈值限制。

自动 cue 不是 AutoMix 的硬约束。增加 Hot cue 并不会直接修好上述 AutoMix 计划；这是共用分析下的两个不同消费问题。

位置：`crates/mixless-analyze/src/cues.rs:44`、`:58`。

### P1：晚入点与尾部回退放大了“播到结尾”

当前链将 A Moment Apart 从 121.66 s 接入，已经跳过其第一轮 49.66–81.66 s 高潮。独立从头规划 A Moment Apart → Body Language 时可在 129.66 s 退出；继承链内入点后变成 231.66 s。这是实际对照结果，不是只看标签推断。

Amnesia 从 121.41 s、I'm Not Alone 从 150.71 s 接入，也都跳过第一轮高潮。不过，独立从头规划这两对仍分别退出到 206.94 s 和 210.71 s，所以不能把所有晚退都归因于上一首。

`handoff` 的入点选择以人声 continuation 和当前能量承接排序；此处没有直接按“跳过多少主体内容”排序。即使全局前瞻存在，也需要审计候选进入前瞻前是否已经只剩晚入点。

Amnesia 的最后非 Silence 区间结束于 202.79 s，细 stem 测量在边缘仍有声尾。`handoff` 在该边缘无法确认人声释放时直接退到 206.94 s 文件末尾，没有在尾部进一步搜索释放后的时刻。203.5 s 之后最大 vocal activity 已低于 0.009，且源能量继续衰减。这提示应检查是否可以在完整尾音结束后更早接管，但不能把 202.79 s 的粗 Silence 标签直接当作无声真值。

大曲速差也是独立约束：160→120、120→174、174→128、128→180 的原速关系并不适合强行做长对拍混合。trace 的大量拒绝确实来自速度预算或人声安全检查；修复结构不能保证每一对都变成长混。安全的原速桥接、保留主体内容和合理排序应分别评估。

位置：`crates/mixless-mixplan/src/handoff.rs:128`、`:181`，`drops.rs:126`，`lookahead.rs`，`smooth/candidate.rs:109`。

## 其他 set 的保护要求

已冻结另外四个 set，共 76 个 track ID／76 个当前歌单条目：Melodic Dubstep 11、Electronic 22、MUG 29、DnB 14。它们不等于 76 个独立录音样本。Electronic 为 v27，其他三个为 v26；本轮只作存储基线检查，未把不同版本当作同条件泛化实验。

其中也存在“相同标签但 novelty ≥0.28”的边界：四个 set 分别为 4、13、16、5 处（只计相邻标签相同、置信度和时长已达到 cue 基础条件的边界，尚未经过八槽容量与间距竞争）。因此简单删除标签过滤也可能改变它们的 pads。MUG 还有四个超过 90 秒的 Drop/Chorus section，不能把长段问题视为 E1 专属。

若后续实施修改，应分开提交并分别验证：

1. 保留现有检测输出，先补充能表达时序／声部变化的边界证据与不确定性；已有 DnB、melodic 的强证据路径作为回归基线。
2. 将曲式功能与节奏／能量状态区分，避免单个标签同时承担命名、候选授权和高潮保护。
3. 独立改进 cue 的边界选择；对比八个槽位的增删、排序和间距，保留用户 cue，不把 Hot cue 等同于安全出口。
4. 独立评估晚入点的内容损失和后续可混性，以及尾音释放后的原速交接；不降低人声、持续音或时钟安全门槛。
5. 同一批源音频和 stem 测量，分别用冻结旧源码和候选源码完整重分析；不能仅改 version、只替换 tempo 或重复加权 stem。
6. 对 E1 和四个 set 同时跑固定顺序、Best order 及真实链内传递，比较段落边界、cue、主体保留、入点、提前截断、无计划率和曲尾回退。不能以平均指标改善抵消某个原本正常的衔接退化。
7. 试听所有受影响的真实衔接及负例；加入短 fill、持续鼓段、只有 bass mute、真正 breakdown、尾音、缺失 stems、增益／时间平移／半倍拍解释检查。E1 作为开发集；其他 set 一旦用于调参也不能再称作未见样本。
8. 持久化语义变化相应更新 analysis/order/cue 版本及刷新流程，然后完成仓库规定测试和开发 app 构建。版本更新不是本轮审计的一部分。

## 验证边界

- 已完成：源码追踪、6 首哈希验证与完整基础重分析、自动 cue 一致性、两种播放证据模式的全链计划、4 对独立规划对照、其他 set 的版本／结构／cue 候选基线。
- `cargo test --locked -p mixless-analyze structure::`：13 项通过。它说明现有结构回归没有失败，不证明 E1 的音乐标注正确。
- 未完成：人工逐段标注、新旧方案渲染试听、Best order 重搜、已安装 app／物理音频设备播放验收。没有生产算法修改，未进行“改后无退化”的声明。
- E1 包含不同电子音乐编排，但不是 pop、acoustic、jazz 等非 EDM 曲风的代表性独立评估集。
