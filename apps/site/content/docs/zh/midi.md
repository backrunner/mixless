---
title: "控制器"
description: "将硬件映射到唱盘、混音器和资料库。"
order: 6
image: /images/social.png
imageAlt: "mixless 双唱盘工作台"
imageWidth: 1200
imageHeight: 630
imageType: image/png
---

打开 **Preferences → Controllers**，选择设备和命名预设后点击 **Apply**。后台每 500 ms 检查设备变化，关闭设置窗口后仍继续运行。已保存的分配按设备 ID 恢复；新设备、身份变化或匹配不明确时需要手动选择。离线预设只保存在库中。

## 预设管理

在 **Manage** 菜单中使用 **New preset…**、**Duplicate…**、**Rename…** 和 **Delete…** 管理用户预设。内置预设只读，修改时建立用户副本。针对一台设备编辑共享预设会产生独立副本，再显式应用；已分配的预设须先解除分配才能删除。

**Manage → Import…** 支持同时选择多个预设 JSON 文件。列表显示逐个文件的结果，点击 **Details** 查看设备要求、名称冲突和适配器权限。无效文件会列出原因并跳过。确认后将有效预设一起保存为独立副本，不改变设备分配；写入失败时整批均不保存。**Manage → Export…** 仅导出当前选中的原生 JSON 预设，不携带本机设备 ID。不支持的版本、无效规则和写入失败均不改变当前配置。不导入原始脚本或 XML 映射。社区 HID 预设可以包含具有明确报告权限的 Wasm 沙箱适配器。

旧 `midi.json` 首次运行时备份为 `midi.pre-controllers.backup.json`，再迁移到 `midi.controllers.json`，原文件保留。旧配置中显式设置的通配绑定继续生效。

## 映射与报告监视

控制器示意图常驻显示在映射列表上方；点击控制项或列表条目可查看输入消息。先选择输入设备，再点击示意图中的控制项并操作硬件。离线点击示意图只查看映射。MIDI 学习会保存首次匹配的 Note、CC 或 Pitch Bend；取消不会修改映射。编辑器支持通道、Note、CC、Pitch Bend，以及显式指定 MSB/LSB 的 14 位 CC。14 位配对不会自动推测。CC 支持 1/127、65/63、1/65 三种相对编码，Calibration 中可以调整方向、灵敏度、软接管和 Jog 每圈计数。复杂映射显示真实输入，通过 Edit MIDI / HID 修改规则。

**Manage → Edit MIDI / HID…** 编辑预设的声明式 JSON，支持 Pitch Bend、14-bit CC、RPN/NRPN、SysEx、校准、软接管、设备独立的 Shift 层及按住/切换行为。Jog 的 `ticks_per_revolution` 可调整，应用默认值仍需实机校准。**Details → Ports…** 为每个逻辑端口明确指定本机输入和输出 ID。

HID 预设指定 VID/PID、接口和 Usage，以及 Report ID、位偏移、宽度、符号、字节顺序和计数器模式。在 **Details** 中，**Add HID field** 添加可编辑字段；**Read feature…** 按报告 ID 和长度读取 Feature Report。**Input monitor** 显示接收字节。字段偏移包含 Report ID 字节，无编号报告统一补零字节。字段含义必须来自厂商协议或用户自行采集。

反馈规则根据应用实际状态发送 MIDI 或 HID Output/Feature Report，相同内容合并，每条规则最高 30 Hz。重连时初始化并刷新反馈，断连释放按住状态。性能控制在显示循环之外处理；资料库浏览和加载按顺序唤醒应用事件循环。

## 社区预设

通过 **Manage → Import…** 导入社区预设。预览会显示作者、版本、许可证和设备要求；适配器还会列出允许读写的报告。导入只保存一个副本，需要另行应用到设备。新版本不会自动替换正在使用的预设，保留旧副本即可切换回去。

固定报告格式使用声明式映射。需要校验和、握手、序列跟踪或定时器的 HID 协议，可以使用独立的 Wasm 适配器接口。控制列表会将这些映射标为 **Adapter**，源代码由作者维护。适配器没有文件或网络访问权限；发生错误时，宿主停止适配器并释放按住状态，重新应用预设即可重启。

开发者可使用[控制器 SDK 和示例](https://github.com/backrunner/mixless/tree/main/crates/mixless-controllers)校验、打包、录制和回放报告。测试夹具可以在没有设备时检查预期动作和反馈字节。导入作者提供的测试记录不会自动获得实机验证状态。社区预设需要附带来源和许可证信息。

## 内置预设与验证状态

预设库提供 **39 个独立编写的预设**，其中 37 个依据厂商官方 MIDI 消息表，另两个使用指定的 NI Controller Editor 模板：

| 厂商 | 型号 |
| --- | --- |
| Pioneer DJ / AlphaTheta | DDJ-200、DDJ-400、DDJ-800、DDJ-1000、DDJ-1000SRT、DDJ-FLX2、DDJ-FLX4、DDJ-FLX6、DDJ-FLX6-GT、DDJ-FLX10、DDJ-GRV6、DDJ-RB、DDJ-REV1、DDJ-REV5、DDJ-REV7、DDJ-RR、DDJ-RX、DDJ-RZ、DDJ-SB、DDJ-SB2、DDJ-SB3、DDJ-SP1、DDJ-SR、DDJ-SR2、DDJ-SX、DDJ-SX2、DDJ-SX3、DDJ-SZ、DDJ-SZ2、DDJ-WeGO2、DDJ-WeGO3、DDJ-WeGO4、DDJ-XP1、DDJ-XP2 |
| Hercules | DJControl Inpulse 200、DJControl Inpulse 300（初代） |
| Numark | DJ2GO（初代） |
| Native Instruments | F1（FL Studio 模板）、X1 MK1（SSL 2.2 模板） |

按型号覆盖有协议依据的传输控制、混音、Jog、浏览加载、Hot Cue 和反馈。选中预设后展开 **Details** 可查看验证状态；**Preset notes** 提供使用说明和功能限制。有四个 Deck 的 DDJ 控制器使用 Deck 1/2；SB/SB2、WeGO3 和 Inpulse 200 每个 Deck 提供四个 Hot Cue。不默认覆盖其他 Pad 模式、效果和后续硬件代际。

全部内置预设均标为“**协议实现，实机验证待完成**”。Jog 灵敏度为待实机校准值；可以先使用预设，后续补充设备验证，但不能视为硬件已认证。声卡和通道对请在音频设置中选择，应用预设不会自动切换输出。

设备和预设按型号自然排序，名称只保留设备名；**None** 始终排在预设第一项。独立混音台和独立播放器不在本轮扩展范围内。

新增的 24 个 DDJ 预设按各自官方表实现。REV5/REV7 需关闭设置工具中的 LOOP options；REV5 的 Jog Touch 因官方表项冲突未绑定，REV7 不包含马达、显示屏或耳机 Cue 推子。XP2 选择 PAD MODE 1，实体 Pad 1–8 对应 mixless Cue 1–8。SP1、XP1/2 是 Pad 控制器预设，没有完整混音或音频接口映射。WeGO2/3/4 各提供四个 Hot Cue。详情见[官方来源与覆盖范围](https://github.com/backrunner/mixless/tree/main/crates/mixless-controllers/examples/alphatheta-ddj)。

## DDJ-FLX2

通过 USB 连接，选择 **DDJ-FLX2** 预设并应用。按住 Shift 按 BEAT SYNC，
再按 Pad 1 选择 HOT CUE 模式；Shift + Pad 清除对应 Cue。双唱盘支持
播放/Cue、Sync、触摸/Jog、速度、EQ、滤波、推子、耳机 PFL、八个 Hot Cue
和灯光反馈，主输出和耳机旋钮控制软件音量。在工作台浏览和加载曲目。

Smart Fader、Master CUE、其他 Pad 模式、Shift 传输控制和推子启动暂不映射。
请手动选择声卡和通道对，不套用 FLX4 的输出顺序。USB 协议回放已验证；
蓝牙、物理操作、Jog 校准、灯光和音频路由仍待设备测试。
详见[预设与验证说明](https://github.com/backrunner/mixless/tree/main/crates/mixless-controllers/examples/ddj-flx2)。

## 控制器声卡

在 **Audio I/O** 分别选择 Master、Headphones 设备和通道对。四通道控制器可使用 Master 1/2、Cue 3/4，同设备只创建一个音频流。通道重叠或超出范围会被拒绝，其余通道输出静音。**Details → Audio routing…** 只填写建议配置，需在 Audio I/O 中点击 **Apply** 才会切换输出。监听设备故障不会将 Cue 信号转入主输出。

**Headphone mix** 从 Cue 混合到 Master，0% Master 保持纯 Cue，100% Master 只监听主输出，中间位置线性混合。耳机音量作用于混合后的信号，不影响主输出。该控制也可通过示意图中的 **CUE MIX** 映射。

## NI DJ 设备

F1 和初代 X1 的预设要求加载 Controller Editor 2.8.2 中指定的原始模板，并选择 Basic 页和 MIDI 模式；不是出厂地址，也不覆盖后续代际或 Shift 页。F1 提供两组唱盘按钮、Stem 音量和 Pad 亮度反馈；X1 提供传输控制、FX/EQ、浏览和加载，没有灯光反馈。打开 Preset notes 查看具体布局。

MX2 可通过双侧 Shift 切换 MIDI 模式后使用 Learn；目前没有完整预设或 NI 原生协议适配。X1 MK3、Z1 MK2 和其他 NI 型号仍有消息定义缺口，不能将它们视为已验证兼容。

依据 Traktor 逐型号输出配置，MX2、Z1（初代和 MK2）、Z2、S2 和 S4（初代、MK2 和 MK3）、S3、S5、S8 提供音频建议。选择匹配设备后，**Details → Audio routing…** 可填写主输出 1/2、耳机 3/4，无需先选择控制预设。请在 Audio I/O 检查并显式应用；这不代表新增控制映射或已验证实际输出。详见[NI 兼容矩阵](https://github.com/backrunner/mixless/blob/main/crates/mixless-controllers/NI_COMPATIBILITY.md)。
