# Automix 实现与试用

基于 `.agents/05-ai-mixing.md` 和 `design.md` §7，实现了离线分析、下一对规划、实时包络执行及 GPUI 入口。默认逐首迁移速度和音乐调性，不把歌单锁在同一 BPM／key。听感优先于强行长混。

## 试用

启动桌面应用，选择至少两首本地歌曲的列表，点击顶部 **AUTO**。若已有一首在播，从该曲在列表中的位置继续；否则从第一首开始。下一首必须在空闲唱盘上准备。再次点击 AUTO 停止自动控制，保留当前播放及混音参数。

过渡就绪后可以 PAUSE / RESUME / SKIP。手动操作 crossfader、gain/fader、EQ、filter、send、rate、pitch 会取消对应自动化 lane。跳 cue、jog 抓盘、反转、手动播放暂停或换歌会停止当前计划；重新开启 AUTO 时读取当前实际 rate/pitch。

无需音频设备即可用与桌面相同的分析器生成过渡试听：

```sh
cargo run -p mixless-analyze --example automix-real -- a.wav b.wav preview.wav
```

自动分析 BPM、调性、网格与逐 bar 特征，输出浮点立体声 WAV，包含最多四秒前文和八秒后文，打印选用模式、接点及输出峰值。不会覆盖已有文件。支持引擎解码器接受的本地格式。

保留仅输入 BPM 的通路检查示例；它没有可信 key/grid/bar 特征，因此通常选择短桥接：

```sh
cargo run -p mixless-engine --example automix -- a.wav b.wav preview.wav 128 126
```

该示例从过渡起点开始，并附两秒尾部。

## 默认平滑策略

- `mixless-mixplan` 只依赖 `protocol`，每次搜索最多 8 个出点 × 8 个入点 × 两种交接模式。
- `BeatBlend`：两轨网格可信、sounding key 兼容、前景冲突低时，以 16／32 bars 为基础安排长混。共同 BPM 使用五次平滑曲线，限制最大对数变化率约 0.35%/s；出碟从当前实际速度开始，入碟静音时先对拍，再随共同速度迁移。常规恒定速度歌曲结束时回到入碟自身 BPM。
- 低频在重拍附近一个拍内等功率交接，并补偿 crossfader 衰减，避免两条底鼓长时间同时响或两轨低频同时消失。中频前景有风险时对次要声部轻度衰减。
- `PhraseBridge`：速度差距过大、调性不兼容、节拍不可靠或前景冲突时，出碟末段低频衰减、滤波和轻 Echo；最后约 40 ms 淡出干声，在乐句结束重拍启动入碟，保留其速度和调性。桥接不再提前一拍抢入，也不把入碟第一拍藏在长 crossfade 里。
- 80/160 默认使用 2:1 拍映射，rate 保持接近 1。`energy_hold` 保留 sounding rate/pitch；下一对读取实际偏移，可继续迁移，不累计到统一歌单速度。调性转换通过音乐内容交接完成，默认不在可听人声上做 pitch glide。
- 独立检查每轨用户 In/Out 范围，使用源采样率；自动 cue 和 Hot 不作硬约束。桥接也不能绕过 cue、已知人声 Verse → Chorus 禁区或音频边界。已测出的静音不作入点，无解返回 `failure_reason`。
- 依据接点附近 RMS 做最多 6 dB 的局部响度补偿，并参考入碟峰值限制正增益。引擎继承出碟当前 gain 和两侧 fader 比例，减少下一次交接时的响度跳变。这是启发式补偿，尚非 LUFS 标准化。

原九种目录节点表、归一化评分、`who_stretches`、literal 半／双倍和额外恢复段保留在 `PlannerOptions { smooth: false, .. }` 路径，其金样继续验证。默认平滑路径使用 BassSwap、EnergyHold、EchoOut，其他显式目录选择请使用目录路径。

## 分析与执行

- 桌面 AUTO 使用独立离线解码的完整 `TrackAnalysis`，含源域 beat/downbeat、逐 bar RMS／峰值／频段能量、kick 显著性、持续前景风险和保守 Intro／Outro／Break／Silence 标签。
- 下拍由低频重音估计；均匀节拍没有足够证据确定 bar one 时降低置信度。按局部窗口检验与全局网格的符合度，变速区域不能继承其他稳定段的可信度。尚未实现完整局部 tempo tracking。
- SQLite `track_analysis` 按 content hash 和 `ANALYSIS_VERSION` 缓存；当前版本 3 使旧置信度结果失效。缓存命中同样更新唱盘 BPM。
- 宿主预计算每 32 个设备帧一个控制点，通过源时间曲线积分控制 rate，滤波使用对数插值。音频线程读不可变数据，支持 A→B、B→A、暂停／恢复、Skip 和单 lane 接管。规划不进音频线程。
- 后台准备下一首并读取当前 rate/pitch；取消后丢弃未发布的解码结果。手动换歌会停止当前计划，重新开启 AUTO 才重规划。

## 验证

```sh
cargo test --workspace --exclude mixless-desktop
cargo check -p mixless-desktop
```

当前工作区（不含桌面）76 项测试通过，既有性能基准 1 项默认忽略。包含 20 项规划器、5 项分析、47 项引擎以及资料库和 Spotify 测试；桌面编译通过。

新增验收覆盖升／降速斜率、80/160 双向映射、低频包络功率连续性、hold 后继承再释放、静音接点、短桥接重拍及人声/cue 硬约束。44.1/48 kHz 源音轨通过实际渲染引擎时，在 128→132 BPM 合成夹具上测得双向最大播放头节拍相位差约 **0.000246 拍**；这是特定夹具的 transport 测量，不是所有音乐的声学对拍或听感保证。

## 当前范围

- 人声值是持续前景风险代理，不能替代声部分离；结构是启发式，ONNX 未接入，因此分析仍标记 `partial`。实际乐曲中有歧义的下拍、拍号、局部变速和调性变化仍需更可靠模型与试听校准。
- 当前界面没有新增 mix-in/out 编辑菜单；规划器和资料库已有 cue 类型接口。曲线 overlay、手动换歌后无缝重规划、shuffle、分析任务队列与事件订阅不在本次实现内。
- 暂停保留已编译剩余包络并暂停双碟；恢复继续该包络。Skip 立即完成当前交接，让入碟成为下一对的出碟。手动改动某 lane 后，该 lane 在本计划内不会被重新接管。
- 尚未验收真实音频设备的延迟、全自动化负载 p99、连续多首音乐试听。32 帧控制精度不等于 Signalsmith 的听感响应延迟。现有滤波器在 30 Hz～18 kHz 范围映射，表中 20 Hz／20 kHz 为全开端点。
