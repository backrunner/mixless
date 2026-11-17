# AutoMix 补偿生命周期代码复查

本轮针对前一轮实曲评估后的未提交实现，复查发布／撤销、手动接管、后台 stem
升级和预览配置。没有修改音乐策略、cue、曲序或补偿的拟合目标与数值阈值。

## 发现和修正

1. **GAIN 接管时重复应用补偿。** UI 从有效 GAIN 值开始拖动，旧实现让手动 GAIN
   的 3 ms 平滑器上升到该值，同时让 AUTO 用 150 ms 退出，两者短时间相乘。
   回归中显示目标为 5.99 dB，接管后组合增益到达 10.135 dB。现在先把当前实际
   乘积移交给一个平滑器，再向手动目标过渡；正、负补偿都验证过渡始终落在当前
   实际值与目标之间，容差 0.02 dB，并检查接管 callback 无分配／释放。
2. **发布会覆盖并发的手动撤销。** `commit_plan` 读取 revision 后检查 PCM/stem
   身份，随后复位 `gain_cancelled`；旧版非 FX 命令不持有相同控制锁，可能在中间
   修改 master 并撤销补偿，随后又被发布重新启用。回归用源锁暂停身份检查，
   让 master 修改在这个窗口发生，旧实现确实保留了过期补偿。现在命令的撤销和
   参数写入、lane 接管、Sync、源加载／移除、stem 发布与 commit 使用同一短锁。
   准备在复制候选配置前记录控制版本、生成包络前固定源身份，不锁住耗时 DSP。
   callback 仍只 try_lock，遇到竞争继续渲染，现有相关回归继续覆盖这一行为。
3. **后台四声部升级继续使用旧测量。** 三声部变四声部后，声音路径已经改变，
   但旧实现仅在准备／发布时验证源身份，没有撤销正在播放的补偿。回归确认旧
   AUTO 仍非零。现在替换 stem 时撤销补偿，保留原接歌计划，已有平滑器负责退出。
   同一源的重复无变化附件仍保持幂等；精确准备好的 replacement 仍能正常发布。
   大 PCM 的退休清理移到 stem 发布锁之外。
4. **部分输出变化没有令测量失效。** 旧实现遗漏 FX 自动淡化和 roll。旧测试在
   准备后关闭 FX 自动淡化，再发布，仍应用旧补偿。现在补齐相关通知，预览也
   复制实际 FX 自动淡化设置。显式 Stop 与加载 replacement 所需的内部 Stop
   分开处理，前者撤销正在准备的补偿。预览不支持的 live roll／brake／reverse／
   loop／jog／cue-preview 状态保留原计划、跳过可选补偿。

前三项新增回归先在修正前失败；FX 自动淡化的失效回归也先失败。修正后的五项
生命周期测试与原有 gain/EQ/stem 集成测试合计 26 项通过，包括接管、暂停恢复、
跳过、计划不变、精确 replacement 身份、独立三频／声部和输出图观测一致性。

## 验证

完整日志与最终 Rust 源哈希位于忽略目录 `target/automix-lifecycle-review-20261117/`。

- `cargo test --locked --workspace --exclude mixless-desktop -- --test-threads=2`：
  814 passed，30 ignored，0 failed。
- `cargo test --locked -p mixless-desktop automix:: -- --test-threads=1`：
  74 passed，4 ignored，0 failed。
- `./dev.sh --build`：成功生成 dev 0.1.0 build 1，未启动。
- 无并行重负载的 release `callback_budget`：5 passed。双 deck、stems、pitch、
  FX、录音、cue 组合 p99 为 0.952 ms、max 为 0.999 ms，块预算为 2.667 ms；
  48 kHz / 256-frame 音频 p99 为 0.201 ms，192 kHz 为 0.212 ms。
- 最终源码哈希、修改 Rust 文件的 rustfmt 检查和 `git diff --check` 通过。

本轮确认的问题已修复，没有遗留的已确认缺陷。五项新增回归分别验证实际增益
接管、信号／Stop 编辑后的旧准备失效、已有 roll 的保守回退、四声部升级失效、
以及 master 编辑与发布交错时撤销不被覆盖。

## 边界

macOS 27.2.1，Apple Silicon。此轮是源码与离线回归验证，没有打开音频设备、人工
试听、安装、提交或发布。上一轮六套／100 次实曲交接的报告与源哈希保留为那个
版本的证据；本轮没有重跑整套语料，也不把之前的计数改写成新版本的实曲结论。
新增覆盖针对上一轮正常播放流程没有触发的并发和交互边界。通过这些检查不等于
数学上证明不存在任何 bug，也不等于听感验收。
