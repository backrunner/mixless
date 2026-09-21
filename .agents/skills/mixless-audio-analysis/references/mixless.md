# Mixless 代码与审计入口

以下路径相对于当前仓库根目录，不绑定某台机器或固定 checkout。先确认所在仓库；代码可能继续演进，使用 `rg` 核实入口及 CLI 参数。

| 层 | 入口 |
| --- | --- |
| tempo / moments / bars | `crates/mixless-analyze/src/features.rs`、`features/tempo.rs`、`features/moments.rs` |
| section / phrase | `crates/mixless-analyze/src/structure.rs`、`structure/dynamics.rs` |
| 共用音乐证据 | `crates/mixless-protocol/src/buildup.rs`、`phrases.rs` |
| 自动 cue / 候选窗口 | `crates/mixless-analyze/src/cues.rs`、`regions.rs` |
| 候选与安全约束 | `crates/mixless-mixplan/src/phrasing.rs`、`drops.rs`、`recovery.rs`、`arrangement.rs`、`smooth/candidate.rs` |
| 加载与重分析 | `apps/desktop/src/analysis.rs`、`analysis/reanalysis.rs`、`analysis/deep.rs` |
| 自动 cue 持久化 | `crates/mixless-library/src/cues.rs` |

工作说明见 `.agents/04-analysis.md`、`.agents/05-ai-mixing.md`；提交前检查见 `CONTRIBUTING.md`。代码与当前验证证据优先于历史报告。

## 可重现审计

先按库当前路径建立 SQLite backup；不能只复制 WAL 模式中的主数据库文件。将临时文件放在忽略的 `target/` 或独立临时目录。保留改前分析和 cue 以比较。审计工具会打开可写数据库，因此传入副本。

- `mixless-library --example reanalyze-copy`：从音频重新提取并附回旧 stem 证据，或使用 `--structure-only` 仅重分类已有测量；当前示例同时刷新自动 cue。
- `mixless-analyze --example structure-audit`：从导出的 TrackAnalysis 检查结构与规划；若传音频路径会重新提取，确认该模式的人声/stem 条件与生产路径是否一致。
- `mixless-library --example automix-audit`：按持久化分析和 cue 检查真实相邻曲对；分轨模式用 `MIXLESS_AUDIT_STEM_PLAYBACK=1`。有 stem 特征不等于实际启用分轨播放。
- `mixless-analyze --example render-audit`：实音频离线渲染；每次使用新的输出目录，避免已有 WAV 的 `create_new` 冲突。读取生成计划核实实际选用的交接和回退。

库可以同时包含多个分析版本。按源 payload 的实际版本读取，确认每首重分析是否完成、旧 stems 是否保留；不要因版本不匹配而把“无法读到缓存”误认成“没有 stem 测量”。结构刷新使用已调整的 bars 时不得再次执行 weighted stem blend。

对于结构/阈值改动，通常先运行 analyzer、protocol、mixplan 的相关测试，随后按 `CONTRIBUTING.md` 完成工作区检查及开发 app 构建。仅当修改实时 DSP/callback 时才需要对应实时预算测试。构建成功不证明已重启、加载最新分析或完成听感验收。
