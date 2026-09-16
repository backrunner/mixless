# 02 — 模块职责与接口

## 依赖图

见 [01-architecture.md](./01-architecture.md)。**`mixplan` 只依赖 `protocol`**，禁止依赖 `analyze`（避免把 `ort`/FFT 拉进规划器）。

## `mixless-protocol`

共享 serde 类型，无重依赖。

- `DeckId`, `TrackId`, `PlaylistId`
- `Command` / `Event` / `EngineSnapshot` / `DeckSnapshot`（完整字段见主文档 §10）
- `TempoMap`, `BarFeature`, `SectionLabel`, **`TrackAnalysis`**
- `MixPlan`, `MixPlanSummary`, `AutomationLanes`, `EqLane`, `FilterLane`, `LoopOp`
- `PerformanceOffset`, `Polyline`
- `Cue`, `CueKind`, `LaneId`, `FxSlot`, `FxParams`
- `AnalysisStage`, `ErrorCode`, `XfCurve`

禁止：文件 I/O、FFI、Tokio。改协议必须同时改 `apps/desktop/src/proto`。

## `mixless-engine`

实时音频。依赖：`protocol`、`cpal`、`symphonia`、Signalsmith（cxx 或 `signalsmith-stretch` crate）、`rtrb`。

```rust
impl Engine {
    pub fn new(config: EngineConfig) -> Result<Self, EngineError>;
    pub fn dispatch(&self, cmd: Command) -> Result<(), EngineError>;
    pub fn subscribe(&self) -> EventRx;
    pub fn load_plan(&self, plan: MixPlan) -> Result<(), EngineError>;
    pub fn takeover_lane(&self, lane: LaneId);
    pub fn render_offline(&self, frames: usize) -> Vec<f32>; // PR03 起
}
```

当前实现：`engine.rs` 定义引擎状态；`engine/{host,commands,render,runtime,fx,state}.rs` 分别负责宿主生命周期、命令、块混音、逐采样 deck 渲染、FX 原子参数与快照。`effects/{mod,process,delay,capture,pitch,spectral,vocoder}.rs` 分离效果生命周期、调度与 DSP 核心。其他节点在 `dsp.rs`、`stretch.rs`、`resample.rs`、`automation.rs`、`engine_sync.rs` 和 `device.rs`。模块拆分与扩展规则见根目录 `CONTRIBUTING.md`。

禁止：分析算法、SQLite、HTTP、callback 里 `read_at` / mmap / `format!` / `Vec` 增长。

## `mixless-analyze`

离线。依赖：`protocol`、`symphonia`/`lofty`、FFT、可选 `ort`。

写出 `TrackAnalysis` blob 经 `Library::save_artifact`。独立 decode 到 22.05 k mono，**不**读播放 cache。

禁止：打开音频设备、依赖 engine。

## `mixless-mixplan`

纯计算。依赖：**仅** `protocol`。

```rust
impl Planner {
    pub fn plan_pair(
        &self,
        a: &TrackAnalysis,
        b: &TrackAnalysis,
        cues_a: &[Cue],
        cues_b: &[Cue],
        offset_a: PerformanceOffset,
        offset_b: PerformanceOffset, // 入碟未 hold → {1.0, 0.0}
    ) -> MixPlan;
    pub fn plan_next(&self, ctx: &PlanContext) -> MixPlan;
    // plan_playlist 仅 test-helpers / 单测
}
```

内部：`candidates.rs` `catalog.rs` `score.rs` `compile.rs` `constraints.rs`

禁止：违反 `covers_user_range` 仍出 plan；生产路径规划整张 playlist。

## `mixless-library`

SQLite + AppData。哈希 `blake3(len || mtime || first_64k || last_4k)`，无 inode。

`load_analysis(id) -> TrackAnalysis`。Token 不得入库。

Schema 变更只加不改（新表 / 可空或有默认值的列），同步 `schema.rs` 的 `SCHEMA_VERSION` 与 `REQUIRED_COLUMNS`；非加性变更会让旧版 build 在打开时拒绝该库（beta→stable 降级路径）。派生 payload 按行级 version 门控，读前必查版本。

## `mixless-spotify`

OAuth PKCE + Web API + 本地 matcher。macOS Keychain。429 退避。只产出 `ResolveJob` / `SpotifyTrack`。

禁止：写音频文件、拉 PCM、依赖 librespot / zotify。CI grep 这些 crate 名（闭源插件仓库除外）。

## `mixless-acquire`

Resolver 链、打分、落盘、lofty 打标签、磁盘配额。依赖：`protocol`、`library`。

批量入口是 `ImportService`（不是独立队列）：`spotify_playlist(&SpotifyPlaylistMeta, fetch, progress) -> ImportReport` 与 `local_files`。每行状态持久化在 `import_items`（queued/local/acquired/suspect/missing），重跑即续传。

## `mixless-acquire-yt`

yt-dlp sidecar。实现 `AudioAcquire` 的 YTM / YouTube。禁止在 UI 进程拼命令行。

## `private/mixless-acquire-cdn`

专有。可选。主树不引用其源码。

## `apps/desktop`

`src-tauri` 薄宿主。`src` Svelte 5 + SCSS 功能目录。PFL 键：有耳机设备才可点。Hot Cue 垫无条件可点。

禁止：TS 里 time-stretch；Web Audio 出主混音。
