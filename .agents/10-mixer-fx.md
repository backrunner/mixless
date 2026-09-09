# 10 — Mixer、效果器与性能

两个必须分清的「cue」：

| 名字 | UI | 没第二输出时 |
|------|-----|----------------|
| **Hot Cue** | 碟上 8 个彩色垫 | **照常**：设置、跳转、mix-in/out。与监听设备无关 |
| **PFL / 耳机预听** | Mixer 上标 **PFL** 的键（不要再标成 CUE） | **只关预听**：键禁用。Master 仍出声，热键跳转不受影响 |

长按热键：有 PFL 设备 → 耳机里从该点预听、不进 Master；无设备 → 与单击相同（Master 上跳转）。不要为了「预听」把热键禁掉。

## Mixer（v1 齐备）

每通道：

- Trim / gain（±12 dB）
- **3-band isolator EQ**（LR4，150 Hz / 2 kHz，±12 dB）
- **Kill**：Low / Mid / High 各一键，立刻 −inf，松开关回到旋钮值
- **Channel filter**：单旋钮，中点全通，左 HP，右 LP（与 Automix `filter_sweep` 同一节点）
- Channel fader
- **PFL**：fader 前抽到耳机总线；无第二设备则禁用，**不影响热键**

总控：

- Crossfader `[-1, 1]`，曲线：`linear` / `equal_power` / `cut` / `scratch`（极陡）
- XF **hamster / reverse**
- Master gain + **brickwall limiter**（ceiling −0.3 dBFS，lookahead 仅用预分配 32–64 sample）
- **Quantize**：热键跳转、loop 开关、beat jump 吸到 grid（可关）
- **Beat jump**：±1 / 2 / 4 / 8 / 16 bar，源域，短 xf

Deck 传输（本来就有，列在这里算齐）：vinyl / slip、独立 rate / pitch、SYNC、keylock、loop 1–16、8 hot cue。

## 效果器（v1 目录）

拓扑不变：每碟 **4 个预分配 insert** + **共享 Echo / Reverb send** + **传输类 FX**（不进 insert，改播放头）。

```
Playhead+Loop → Stretch/Resample → EQ+Kill+Filter → Insert0..3 → Send tap
                                                              → Fader → XF → Master+Limiter
Send tap → Echo + Reverb → Master
Insert 后、Fader 前 → PFL（仅第二设备）
```

### Insert（每槽可选一种，两碟可不同）

| id | 算法要点 | 关键参数 | 典型用途 |
|----|----------|----------|----------|
| `gate` | 按拍打开，trance gate | `division` 1/4–1/16，`mix` | Break 切换 |
| `flanger` | 短 sweep delay + feedback | `rate_hz`，`depth`，`fb` | 过渡染色 |
| `phaser` | 6–8 级 AP | `rate_hz`，`depth`，`fb` | 同上，更薄 |
| `crush` | bit + 降采样（预计算步长） | `bits` 4–16，`rate_div` | 脏化 |
| `dist` | tanh waveshape | `drive`，`mix`，`hpf` | 加谐波 |
| `chorus` | 2–3 tap 调制 delay | `rate`，`depth`，`mix` | 铺垫 |

用户点名的 Gate / Flanger / Phaser 必须是一等公民，参数可自动化。

### Send（始终分配，mix=0 时 **整段 bypass**）

| id | 算法要点 | 关键参数 |
|----|----------|----------|
| `echo` | 拍同步 delay，可选 ping-pong；**echo-out**：冻输入、只放尾巴 | `beats` 1/8–2，`fb`，`hpf`，`pingpong` |
| `reverb` | 4×4 FDN，decay 0.4–2.5 s，pre-delay 预分配 | `decay`，`size`，`damp`，`mix` |

Automix `echo_out` 走 send echo 的 freeze，不另开图。

### 传输类（改播放头，0 额外 STFT）

| id | 行为 |
|----|------|
| `roll` | 源域 1/32–1 bar 循环，松开回到「本应到达」的 playhead（slip 语义） |
| `reverse` | 播放头负向，rate 绝对值不变 |
| `brake` | 指数减速到 0（停盘）；`spinback` 再反向短促 |
| `slip` | 已有：松开 jog/loop/roll 后跳回未操作轨迹 |

## 性能（验收，macOS Apple Silicon 与近 5 年 Intel）

目标：**全功能开着也要稳**，不能「关 FX 才不爆音」。

| 指标 | 验收 |
|------|------|
| 稳跑块 | 256 frames @ 48 k |
| jog 块 | 128 frames |
| Callback p99，**两碟 + 每碟 4 insert + 双 send + limiter + stretch 音乐档** | **< 50% block**（256 帧 → < 2.67 ms） |
| 同上，scratch 档（无 STFT） | **< 35% block** |
| 单 insert | < 0.12 ms / 256 帧 |
| Echo + Reverb 合计 | < 0.25 ms / 256 帧 |
| xrun | < 0.01%（30 min 压测） |
| 热键跳转缺口 | < 8 ms |
| Jog p99 | < 30 ms @ 128 / scratch |
| 关效果 | `mix==0` 且无 automation 写该槽 → **零代价 bypass**（不进 process） |

硬约束（违反即缺陷）：

- 所有 delay / FDN / limiter lookahead / 降采样缓冲在 `Engine::new(actual_sr)` **一次分配**，callback 禁 `resize` / `Box` / `Vec`。
- EQ / filter / delay mix 走 **SIMD**（macOS `Accelerate` vDSP 或显式 NEON）。
- denormal：callback 入口 `FTZ/DAZ`。
- insert 用函数指针表或 enum dispatch，不用 trait object 虚调用热路径。
- 自动化包络 lock-free 读；FX 参数块边界生效，禁止在 callback 里重建延迟线。
- 离线夹具：`render_offline` 必须覆盖「全 FX 全开 60 s 无 NaN / 无 inf / 峰值 ≤ 0 dBFS」。

## 命令（补齐，勿与热键混淆）

```
SetPfl { deck, on }              // 耳机预听，无设备则 Host 拒绝
SetCueGain { value }
SetCueDevice { name }
JumpCue / SetCue / ClearCue      // 热键，永远可用
SetEqKill { deck, band, on }
SetChannelFilter { deck, amount } // -1..1
SetXfReverse { on }
SetQuantize { on }
BeatJump { deck, bars }           // 有符号
SetFx { deck, slot: Insert0..3 | SendEcho | SendReverb, kind, params }
SetFxBypass { deck, slot, on }
SetRoll { deck, division, on }
SetReverse { deck, on }
SetBrake { deck, on }
```

`FxSlot`：`Insert0 | Insert1 | Insert2 | Insert3 | SendEcho | SendReverb`。
