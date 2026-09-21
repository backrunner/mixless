# Take You Home / Day Cycle 的停顿与出点复核

本文记录 v17。其固定小节数判据已由 [v18 泛化复核](2026-09-21-analysis-generalization.md) 中的测量证据组合取代；下文结果按历史版本理解。

从正式库 SQLite backup 取得分析与 cue，在副本上重新解码 27 首音频、保留原有 stem 测量。没有改写正式资料库、重启播放器或覆盖用户 cue。

## 实测问题

- Day Cycle：60.581–67.517 秒的短回落后低频重新进入；旧自动 Out 和实际规划的 BassSwap 都在 60.581 秒结束。旧保护逻辑将“上一段高潮的结束”无条件豁免，导致它漏过即将恢复的 drop。
- Take You Home：80.551–89.841、162.611–176.545 秒为两次 build，最后约一小节留白/蓄势后恢复。持续高频上升、低频退让可发生在 RMS 下降且起音密度近乎不变时，不能仅靠越来越响来确认 build。89.841–142.483 秒的连续高潮，以及 176.545–202.866 秒带内部音色切换的高潮，需要作为完整段落对待。
- Take You Home 正式 v16 库的 Out 已为 142.483 秒；116.16 秒是早先审计副本中的旧 cue，不能作为当前正式库错误。正式库自动 In 仍为 0.039 秒，落在结构已经标记的片头近静音里。

## 修复

- 协议层共享 `peak_ranges` / `drop_suspension`：合并连续 Drop/Chorus 后判断长度；两次持续高潮之间至多 6 个局部小节的短回落不作为自动出点，完整 8 小节 breakdown 保留。
- 自动 Out、mix region、恢复窗口和过渡候选遵循相同停顿判断。停顿开始处不能退出；在恢复 downbeat 退出需要入碟 drop 同时接管。用户显式 cue 保留原有优先级。
- Build-up 可包含末尾 1–2 小节静音；不再用最后一小节的单次回归证明蓄势。加入持续高频上升证据，并收紧软 build 标签。
- 自动 cue 使用完整高潮范围，In 排除 Silence；版本升级为 17 以触发缓存重算。
- `reanalyze-copy` 现在刷新自动 cue，并支持 `--structure-only`。该模式保留已有 stem 修正，不重复加权同一份 stem 测量。

## 验证

- 两首的数值 bar 特征加入回归，另覆盖静音前停、连续高潮内部变奏、1–6 小节停顿与正常 8 小节 breakdown。
- 工作区测试（排除桌面）：334 passed，12 ignored；开发 app 构建成功。
- 原始混合音频重新解码 27 首（30.5 秒）；附回每首原有 stem 测量一次。普通和分轨播放模式均为 24/24 对成功。Day Cycle → Idle World 在 A 67.517 秒接入 B 38.445 秒的 drop；自动 Out 为 89.713 秒，两者是不同合法用途。Take You Home 自动 In 为 1.588 秒、Out 为 142.483 秒。
- 最终 3 组实音频离线渲染（Take You Home → Day Cycle、Day Cycle → Idle World、Idle World → Take You Home）全部完成，无非有限值或削波；输出见 `target/phrase-audit/renders-final/`。Take You Home 作为歌单末曲的额外循环压力检查使用曲尾安全交接，不代表其正式歌单规划。
- 本地审计产物在 `target/phrase-audit/`，含库副本、重分析日志、普通/分轨规划日志和实音频渲染。
- 离线特征与渲染证明规则生效及音频有效，不等于主观试听或所有曲风的乐段边界均已准确。未进行 UI 听感验收。
