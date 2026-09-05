# 00 — 总需求

| 字段 | 值 |
|------|-----|
| 产品 | Mixless |
| 日期 | 2026-08-15 |
| 状态 | Draft 0.3.0 |

## Persona

- **卧室 / 酒吧 DJ**：习惯 djay Pro / Serato 双碟，要搓碟、hot cue、loop、SYNC、彩色波形、独立变速变调。
- **做歌单的人**：用 Spotify 管歌单，希望登录后直接看到同一套列表，点导入就能在碟上混，而不是手抄曲名、自己找文件。
- **懒得每对歌手混的人**：打开 Automix，列表按顺序（或 shuffle）无缝接下去，必要时自己抢 fader。

不是：要 DVS 的俱乐部老炮（v1 不做 timecode）；不是要 4 碟战的 battle DJ。

## 用户故事与验收

### 双唱盘与工作台

- 作为 DJ，我能在 Deck A/B 加载本地曲目，看到封面、曲名、BPM、Camelot。
- 我能用拟物 jog 搓碟（含负向与惯性），vinyl / slip 可切换。切入/切出 vinyl 无咔哒。
- 我能一键 **SYNC**：本碟 sounding BPM 贴对面，并量化到对面下一 downbeat；可选 keylock。
- 我能独立调 pitch（半音）和 BPM（time-stretch），互不影响。
- 我能用完整 mixer：trim、3-band isolator + **kill**、channel filter、fader、crossfader（含曲线与 hamster）、master + limiter、quantize、beat jump。
- 效果齐备：每碟 3 insert（Gate / Flanger / Phaser / Crush / Dist / Chorus）+ 共享 Echo（含 echo-out）/ Reverb + 传输类 Roll / Reverse / Brake。规格见 [10-mixer-fx.md](./10-mixer-fx.md)。
- 我能设 1/2/4/8/16 bar loop（源播放头域），开关与倍减吸附网格。
- 每轨最多 8 个 **Hot Cue**；单击跳转，seek 无咔哒、缺口 < 8 ms。**没有耳机设备时热键照常能跳**，只是不能 PFL 预听。
- 右键可把某垫标成 **mix-in / mix-out**。一键自动 cue 在结构分析就绪后可用，不覆盖我手打的点。
- Mixer 上的 **PFL** 才是耳机预听。没有第二输出时只禁用 PFL，**绝不禁用热键**。

**验收**：

- 图跑设备原生 SR；jog SLA 配置 128 frames。
- **Jog 控制 → 声音 p99 < 30 ms**（scratch 重采样路径，指针 4–8 ms 合并）。算术见 [03-audio-pipeline.md](./03-audio-pipeline.md)。
- `< 10 ms` 往返 **不是** v1 验收（P1：stretch-bypass + 非 WebView 指针）。
- cue gap < 8 ms（6 ms 多块 xf）。
- xrun < 0.01%。
- 两碟 + 全 FX 全开：callback p99 < 50% of 256-frame block。
- 两碟同时出声、xf 中间 equal-power 不掉能量。
- 无第二输出时：Hot Cue 跳转验收仍过；PFL 键为 disabled。

### 资源管理器与 metadata

- 我能 **显式导入** 音频（flac/mp3/aac/wav/ogg/aiff），看到 title/artist/album/ISRC/时长/封面。
- Library 有文件夹树 + playlist + 曲目表（含分析状态）。
- 损坏标签不导致导入失败，回退文件名。
- Watch-folder **不是** v1（P1）。

### 预分析

- 导入后自动排队：单 BPM 或分段动态 BPM、主调、彩色波形、按小节频谱、结构段、逐 bar 特征。
- 4 分钟 44.1 k stereo 在普通笔记本上，不含深度学习结构模型，< 8 s 出 BPM/key/waveform。
- 结构模型可后台补齐；失败时规则切段，状态 `partial` 仍可混。
- 分析独立 decode，不复用播放 PCM cache。

### Spotify 歌单 + 获取

- Spotify Premium 登录后能浏览 playlist / saved tracks / 搜索。
- 未匹配到本地文件时，点「导入 / 获取」走获取链：本地库 → YouTube Music → YouTube 回退；可选闭源 CDN 插件（默认关）。详见 [09-acquire.md](./09-acquire.md)。
- 按 ISRC / 标题+艺人+时长先匹配本地。无 ISRC 时自动链接必须三项全中。
- 获取成功：落盘、打 Spotify 标签、入资料库、排队分析，然后才能 load 到碟。
- `|Δt| > 5 s` 或匹配分 < 0.72 不得自动采用。
- Free 账号被拒绝。token 只进 macOS Keychain。

**验收**：一条真实 playlist 能批量变成 `acquired`/`local` 行；YTM 误匹配（1 小时混音、现场版）被时长窗拦住；Deck 从不直接播 Spotify 流。

### AI Mixing

- 按 playlist 顺序自动混；可 shuffle（只打乱顺序）。生产路径只规划 **下一 1–2 对**。
- 无显式 mix-in/out：自动选 in/out。
- 有 `kind=in|out`：分轨约束，选用窗口必须覆盖该点，只可向外扩展。
- **Hot cue 不是最小跨度**，只作 ±2 bar 软加分。
- 规划器从过渡目录选策略，按节点表编译 xfader / EQ / gain / filter / FX / rate / pitch / loop。
- 禁止人声 verse 中段硬切人声 chorus。
- 分数（归一化 0–1）< 0.35 走 `fallback_swap_filter`，禁止静音跳切。
- 我移动 fader 时对应 automation lane 取消。Automix 可 pause/resume/skip。
- `energy_hold` 后的 sounding 网格经 `PerformanceOffset` 进入下一对。

### 音频输出

- Master = 系统默认输出，跟随系统默认设备变化并热切换（短 fade，不爆音）。
- 耳机 = 用户选择的第二 CoreAudio 设备。PFL 亮的碟在 fader 前被抽到耳机总线。无该设备则不能预听，热键跳转不受影响。
- 图按原生 SR 跑，预分配按实际 SR，上限 96 kHz。v1 只做 macOS。

## 功能范围（v1 / 以后）

| 功能 | v1 | 以后 |
|------|----|------|
| 2 deck + 完整 mixer + 3 insert + send FX + 传输 FX + loop + 8 hot cue + SYNC | 是 | |
| 本地库 + 预分析（显式导入） | 是 | watch-folder、手动 beat grid（P1） |
| Automix 8 策略 + fallback 表 | 是 | smart shuffle（P1） |
| 右键 mix-in / mix-out | 是 | |
| Spotify 浏览 + YTM/YT 获取 | 是 | streamrip / Soulseek（P1/P2）、闭源 CDN 插件 |
| Master 默认输出 + headphone cue | 是 | 同设备多通道拆分 |
| 4 deck / DVS / stems / MIDI / 云库 / 移动端 | 否 | P1–P2 评估 |

## 非目标

- 4 decks、DVS / timecode vinyl、stem separation、MIDI 控制器（v1）、移动端。
- 开源树里链 librespot / zotify / 任何 Spotify CDN 客户端。
- Web Audio 主混音；每帧 JSON IPC 传 PCM。
- 默认把用户音频传到云端。
- 云同步资料库。
- v1 watch-folder。
- 把 Mixxx / fundsp 当 mixer 核心。
- v1 Windows。

## 约束

- 计算密集路径在 Rust；Tauri 前端只渲染。
- 分析默认全本地；ONNX 放 app resources。
- 仅 Spotify Premium 可登录浏览。
- **目标平台：macOS only（v1）**。
- 主代码 Apache-2.0。CDN 获取若做，必须是闭源插件。

## 获取策略（需求级）

官方 API 给不出 PCM，Partner 不申请。混音对象永远是本地文件。获取层用 Spotify 的 ISRC/曲名去 **YouTube Music 等渠道** 找同一首录音（spotDL 同款路线），用时长窗挡住错版本。闭源 CDN 插件可选、默认关、不进开源发行。
