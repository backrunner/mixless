# AutoMix 补偿后续复查

本轮继续检查发布锁顺序、准备状态失效、暂停／恢复、手动接管、crossfader 映射，
以及桌面自动演奏到引擎的命令路径。没有修改接歌搜索、cue、曲序、stem swap
策略、补偿拟合目标或数值阈值。

## 发现与修复

发现一个已有的命令接线遗漏：`performance_moves` 生成接歌前的段落 EQ 动作，
桌面 `Performer` 通过 `Engine::perform` 发出 `SetEq`，但这个入口只接受
channel filter 和 stem gain。因此 EQ 动作会返回错误，无法执行；桌面端忽略了
整批动作的错误，用户看不到拒绝原因。

现在 `perform` 接受有限数值的 `SetEq`，仍使用原来的 EQ 限幅和平滑处理。
自动演奏不会被当作手动接管；用户通过 `dispatch` 操作 EQ 的撤销规则保持原样。
这只是接通已经生成的动作，没有新增音乐判断或重新选择接歌。

新增集成回归验证接歌前的 EQ 动作实际生效、恢复中性值后原接歌包络接管，
已准备的补偿仍有效，公开计划和 plan ID 不变，非有限 EQ 参数被拒绝。
修复前首先复现 `perform accepts channel filter and stem gain only` 错误；
修复后该完整路径通过。测试搭建时另修正了 cue 设置：直接改 playhead 会被
尚未消费的 seek 覆盖，最终改为设置更晚的合法接歌时间。该次测试搭建失败的
日志单独保留，不计入最终验证结果。

## 验证

最终日志及源哈希保存在忽略目录 `target/automix-followup-review-20261117/`。

- `cargo test --locked --workspace --exclude mixless-desktop -- --test-threads=2`：
  815 passed，30 ignored，0 failed。
- `cargo test --locked -p mixless-desktop automix:: -- --test-threads=1`：
  74 passed，4 ignored，0 failed。
- `./dev.sh --build`：成功生成 dev 0.1.0 build 1，未启动。
- 无并行重负载的 release `callback_budget`：5 passed。双 deck、stems、pitch、
  FX、录音、cue 组合 p99 为 0.939 ms、max 为 0.988 ms，块预算为 2.667 ms；
  48 kHz / 256-frame 音频 p99 为 0.199 ms，192 kHz 为 0.207 ms。
- 本轮修改的 Rust 文件通过 rustfmt 检查，`git diff --check` 通过。
  验证记录中的 30 个 Rust 源文件哈希与最终工作区一致。

本轮确认的一处问题已修复。其余复查范围内没有发现新的已确认缺陷；这些检查
不能证明所有状态组合都不存在 bug，也不构成主观听感验收。

## 证据边界

本轮为 macOS 27.2.1 Apple Silicon 上的源码审查、离线音频回归、构建及 callback
预算测试，不包含音频设备播放或人工试听。原六套／100 次实曲交接报告保留其
原始版本和源哈希，本轮没有重跑全套实曲。该渲染工具不执行桌面的接歌前自动
演奏，原有数字也不能证明本次修正的命令路径；新增回归专门覆盖这一缺口。
没有提交、安装或发布。
