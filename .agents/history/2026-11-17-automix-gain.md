# AutoMix 总输出感知 gain 补偿

本文记录第一阶段的 gain-only 实现与实测；后续三频/stem 扩展见
[联合补偿记录](2026-11-17-automix-tone.md)。

## 范围

本次只升级 engine 的临时自动 gain。没有修改 planner、接歌策略、cue、顺序、
源时间映射、原有 EQ/filter、crossfader 或 stem 包络，也没有修改正式库。
`ORDER_VERSION` 不变。开始时基线为 `d886be468932bfe04468da7946a2355a138c0b8b`。

原实现以每碟局部 PCM RMS 和前后锚点分别给出正增益。新实现用相同播放图的
host preview 测量实际路由后的两碟、两碟之和、master 前及最终输出；gain 的
正负修正联合拟合，总响度和三个频段都参与约束。没有新增 EQ 自动化。

全声部、平直通道音色的同源时间参考只用于限制补偿，保持原有推子和源时钟。
它不是另一套接歌策略。完整前景交接还会使用交接前后的局部响度中位数，建立
缓慢的对数功率目标；每碟自身的实测强弱变化和频谱比例仍然保留。部分 crossfader
落点或上下文不足时不做这种端点匹配。原曲本身变安静不会被远处较响的段落拉高；缺少低频
也不会自动授权把剩余人声无限推高。同相叠加隆起可被衰减，反相抵消不授权提升。

## 播放与安全边界

- BS.1770 momentary 和 short-term，另测与播放 EQ 一致的 LR4 三段能量；
  分频段功率不是独立标准 LUFS，不能将它们相加解释为总响度。
- 预览经过真实 stem 残差重组、变速/移调、EQ/filter、insert/send、通道 limiter、
  推子、等功率 crossfader、master EQ/动态/limiter。
- 提升最多 6 dB，衰减最多 3 dB，随可听路由比例收缩；静音和极弱保留声部不追增益。
  3 dB 衰减预算对应双路同相等功率叠加相对功率和的最大隆起。
- 1 dB deadband；变化最多 1.5 dB/s，并结合 sounding BPM 限制为 0.75 dB/beat。
  边缘归零及现有 150 ms runtime smoothing 防止补偿退出突变。
- 候选必须改善两碟之和的误差且不能恶化经过基线 master 频响调整后的输出误差。
  实际候选渲染检查频段隆起、short-term、true peak，以及通道/master 的限幅压力。
  失败则保留原计划，关闭本次可选补偿。
- host 计算最多 10 秒；移动播放头距启动不足 250 ms 时放弃，绝不发布半条曲线。
  这可能导致临近启动或较长过渡没有额外补偿，不会改写策略或延后计划启动时间。
- 控制状态 revision 加实际 PCM/stem 身份约束阻止过期结果生效。保留 Arc 避免地址复用。
  在正常替换流程中加载同一份预备 PCM 不会误取消补偿；同 track ID 换了 PCM 则取消。
- 手动 GAIN 接管时，正负自动增益与 manual gain 的乘积逐样本保持在 -12..+12 dB。

## 验证

环境：macOS 27.2.1，Apple Silicon。使用离线引擎，未打开物理音频设备。

- 完整 workspace（排除 desktop）：797 passed，30 ignored。
- Release callback budget：5 passed，测试时没有并行运行本次的其他构建或渲染。
- 开发 app bundle 构建通过；没有安装到 Applications、启动物理播放或发布。
- 回归覆盖完整 MixPlan 不变、双 deck 方向、保留 stems、EQ 与 crossfader 权重、
  同相/反相、频段缺失、原曲静音与自然弱段、增益变换、pause/resume/skip/stop、
  正负 manual takeover、callback 无分配、master 改动和发布前 PCM/stem 替换。
- 完整声部、局部响度相差 12 dB 的合成曲对覆盖独立的短 crossfader 交接，
  补偿降低了交接处最大的 momentary 响度台阶；没有修改策略包络。
- 普通播放和观察模式在变速、移调、stems、filter 场景下逐样本一致；
  额外的 send 与 master EQ/压缩场景也进行同样的比较。
- 尝试严格 Clippy，被已有 protocol 告警阻断（Event 大枚举、旧代码 range loop、
  closure、last、return）；没有为此改动无关协议或规划逻辑。

### 冻结计划的真实音频开关对照

输入采用已有的四个冻结计划，重新读取实际音频与已对齐 stem PCM；没有重跑选曲策略。
8 个源文件的 SHA-256、本次 SQLite backup、计划/路径与输出位于忽略目录
`target/automix-gain-20261117/`。这些是开发/回归材料，不是未见评估集。

| 曲对 | gain 范围 | 局部 M 响度变化 | true peak（off / on） | 结果 |
| --- | --- | --- | --- | --- |
| 100 → 95 | 0 至 +2.10 dB | +0.802 LU | -6.411 / -6.411 dBTP | 使用补偿 |
| 38 → 40 | 0 dB | 0 LU | -6.514 / -6.514 dBTP | 原输出逐样本保留 |
| 106 → 107 | 0 dB | 0 LU | -7.756 / -7.756 dBTP | 原输出逐样本保留 |
| 51 → 49 | -0.90 至 +0.30 dB | -0.702 至 +0.176 LU | -8.156 / -8.156 dBTP | 使用补偿 |

四组都完成交接，发布计划相同，crossfader、fader、stem 与 transport 记录完全相同。
100 → 95 的整体 integrated loudness 从 -19.317 变为 -19.308 LUFS；修正主要发生在
局部交接中。51 → 49 的整体 integrated loudness 从 -22.100 变为 -22.136 LUFS，
实曲中也覆盖了负 gain。100 → 95 的准备时间约 7 秒；其他样本约 0.7–2.5 秒，
时间不是实时性能保证。局部 M 变化是 on 相对 off 的差值，不直接等同于改善量。

## 结论限制

预览采用最多三秒前置上下文和交接后三秒，复现确定的数字播放条件；它不是任意
运行中 FX 历史的完整内存快照，也不代表硬件或声学输出。播放 limiter 仍是
sample-peak limiter，额外 true-peak 检查不等于新增 true-peak limiter。

本次没有进行人工试听验收，也没有评估整个用户曲库。数值改善、逐样本观察一致和
跨曲对保持原输出，都不能替代“自然且平滑”的听感确认。
