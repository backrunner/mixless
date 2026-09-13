# AutoMix：编曲、分轨和音符证据

核查日期：2026-09-13。原 MLX/Python 评估入口已经被 [Rust 正式链路](native-inference.md) 替代；本页保留模型选型和教学依据，不把推理成功当作识别准确率。

## DJ 教学与本轮应用

Native Instruments 的教学把对拍、乐句、编曲和调性同时列为过渡基础，并将 fade、cut、phrase beatmatch、bass swap 和 filter fade 分别讲解。Bass swap 是重叠混音中的一次低频交接，其长度不等于整段混音长度。没有依据要求每首歌都在 drop 后固定一小节启动，或把所有过渡做成 1–2 小节。[官方教程](https://blog.native-instruments.com/dj-transitions/)

Digital DJ Tips 强调计数、乐句与基本混音，提醒不要以社交媒体上的短技巧演示代替整场音乐安排。我们据此保留合法的瞬间切歌，同时让可靠且相容的长窗口拥有充分的音量/EQ 交接空间。[Phil Morse 的教学正文](https://www.digitaldjtips.com/5-steps-mixing-podcast/)

此前已阅读的教学来源在这里集中保留，替代分散的逐轮报告：

| 来源 | 对本项目有用的主题 |
| --- | --- |
| [Digital DJ Tips：在哪里切歌](https://www.digitaldjtips.com/how-pro-djs-know-where-to-transition-plus-a-big-cheat/) | 用乐句和编曲安排接点 |
| [DJ.Studio：DnB 混音](https://dj.studio/blog/mix-drum-and-bass) | DnB 的对拍、能量与过渡 |
| [Pioneer DJ：DDJ-400 技巧教程](https://www.pioneerdj.com/en/news/2019/ddj-400-dj-controller-mixing-technique-tutorials/) | Drop swap、loop 与 EQ |
| [Crossfader：入门指南](https://wearecrossfader.co.uk/blog/learn-to-dj-the-complete-beginners-guide/) | 计拍、乐句与 EQ |
| [Crossfader：Techno 混音](https://wearecrossfader.co.uk/blog/mix-like-a-techno-dj-3-ways-to-mix-techno/) | 持续叠混和逐步替换声部 |
| [Native Instruments：Cue 与编曲](https://blog.native-instruments.com/cue-points-djing/) | Cue 标记变化点，不等于必须从此处结束混音 |
| [Native Instruments：现场重混](https://blog.native-instruments.com/live-remixing/) | 用素材和 loop 构造重叠 |
| [Native Instruments：DJ Tips](https://blog.native-instruments.com/dj-tips/) | 选曲、音乐流动与演出安排 |
| [Digital DJ Tips：House 混音](https://www.digitaldjtips.com/the-ultimate-guide-to-mixing-house-music/) | 长乐句内渐进交接 |
| [Digital DJ Tips：Loop 的使用](https://www.digitaldjtips.com/three-ways-to-use-loops-without-annoying-everyone/) | 有目的地延展素材，避免机械重复 |
| [Digital DJ Tips：Sync 混音](https://www.digitaldjtips.com/5-tips-for-better-sync-mixing/) | 网格和乐句仍需正确 |
| [Crossfader：高能量过渡](https://wearecrossfader.co.uk/blog/12-high-energy-dj-transitions/) | 能量、BPM、人声和 EQ 的配合 |
| [Crossfader：DnB](https://wearecrossfader.co.uk/blog/howtomixdnb/) | 高速鼓、人声与合成器的协调 |

24/32/48/64 小节只是候选长度，不是硬性目标；实际乐句长度也进入搜索。瞬切由结构变化和声音连续性决定，普通叠混由整段节拍、调性/音高分布、能量及双人声证据决定。具体置信阈值属于本项目工程选择。

## SoundAnalysis 与模型选择

[Apple SoundAnalysis](https://developer.apple.com/documentation/SoundAnalysis) 是声音分类框架。当前系统分类器可补充人声活动证据，不能直接输出 stems、乐谱或 DJ 乐段语义。因此它不能独自完成这里的混音诉求。

| 路线 | 三轨要求 | 当前判断 |
| --- | --- | --- |
| HTDemucs / HTDemucs FT + [demucs-mlx](https://github.com/ssmall256/demucs-mlx/) | 原生 vocals/drums/bass/other，bass+other 合并为 instruments | 历史原型跑通；按 Rust 技术栈要求，当前正式路径改用 ONNX |
| HTDemucs + [StemSplit demucs-onnx](https://github.com/StemSplit/demucs-onnx) | 相同四类，可合并成三轨 | 固定 FP16 权重模型已通过 Rust/ONNX CPU 跑通完整曲目并接入默认后台分析；未验证 CoreML EP 或与 MLX 逐样本等价 |
| RoFormer + [mlx-audio-separator](https://github.com/ssmall256/mlx-audio-separator) | 取决于具体 checkpoint；两轨人声模型不能单独分出鼓 | 作为后续听感对照，不能仅凭“RoFormer”名称认定任意权重满足三轨要求 |
| [sherpa-onnx 的 Spleeter 模型](https://k2-fsa.github.io/sherpa/onnx/source-separation/models.html) | 文档中的两轨示例是人声/伴奏 | 不足以单独满足人声/鼓/其他三轨需求 |

[Meta Demucs 文档](https://github.com/facebookresearch/demucs) 将 FT 描述为约四倍耗时、可能更好的精调版，并指出独立归一化各 stem 会破坏它们的相对音量。评估脚本保存 float WAV，不逐轨归一化、不裁剪；stem 峰值允许超过 1。真正接入播放图时必须处理统一 headroom、混合一致性残差、同步 seek/loop、缓存和取消，不能用推理线程阻塞载入或音频 callback。

## 音符与进行分析

[Celemony 对 DNA 的说明](https://helpcenter.celemony.com/M5/doc/melodyneStudio5/en/M5tour_AudioAlgorithms?env=standAlone) 明确：它面向单独录制的复音乐器，按音高区分音符，不能把两个乐器同时演奏的同音自动分成两个乐器。没有公开的 Melodyne 算法可直接照搬到本项目。

[Spotify Basic Pitch](https://github.com/spotify/basic-pitch) 提供音符事件和 pitch bend，包含 ONNX 模型。当前 Rust 链路在分离的人声与 instruments 上运行 ONNX，输出带来源轨、起止秒数、MIDI 音高和模型置信度的 JSON；旧原型的 MIDI 文件导出不属于当前运行链路。乐器轨仍是多乐器混合，失真贝斯、和声及泄漏可能产生错音；这些输出是估计，不是 ground truth。

当前 Rust 规划器优先使用分轨后的音符分布匹配，考虑实际移调和持续不相容区间；缺少音符时保留混合频谱的保守检查。人声活动、音符跨越切点和分离鼓的频段证据已经接入默认桌面分析及 AutoMix。现已支持 [独立声部实时混音](stem-playback.md)：后台准备 PCM，播放阶段使用同一时钟和变速器混合三声部；AutoMix 在有效窗口内按证据选择声部包络。分离质量尚未与商业产品做盲听对照。Python 原型和依赖清单已移除，原生入口如下。

```sh
cargo run --locked -p mixless-stems -- song.wav target/models target/stems
```

原生程序与权重都在本地运行，首次仅下载模型文件。用于下一阶段的可靠性指标应包含标注片段上的分轨泄漏/伪影、note onset/offset F1、切点错误率、完整混音盲听，以及导入和演出并行时的速度与内存；单条 WAV 的重构残差不等同于分轨音质分数。
