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

**Manage → Import…** 预览设备要求、绑定数量和名称冲突。确认只保存独立副本，不改变设备分配。**Manage → Export…** 仅导出当前选中的原生 JSON 预设，不携带本机设备 ID。不支持的版本、无效规则和写入失败均不改变当前配置。不导入原始脚本或 XML 映射。社区 HID 预设可以包含具有明确报告权限的 Wasm 沙箱适配器。

旧 `midi.json` 首次运行时备份为 `midi.pre-controllers.backup.json`，再迁移到 `midi.controllers.json`，原文件保留。旧配置中显式设置的通配绑定继续生效。

## 映射与报告监视

控制器示意图常驻显示在映射列表上方；点击控制项或列表条目可查看输入消息。先选择输入设备，再点击示意图中的控制项并操作硬件。离线点击示意图只查看映射。MIDI 学习会保存首次匹配的 Note 或 CC；取消不会修改映射。简单编辑器支持通道、Note/CC，以及 1/127、65/63、1/65 三种相对编码。复杂映射显示真实输入，通过 Edit MIDI / HID 修改规则。

**Manage → Edit MIDI / HID…** 编辑预设的声明式 JSON，支持 Pitch Bend、14-bit CC、RPN/NRPN、SysEx、校准、软接管、设备独立的 Shift 层及按住/切换行为。Jog 的 `ticks_per_revolution` 可调整，应用默认值仍需实机校准。**Details → Ports…** 为每个逻辑端口明确指定本机输入和输出 ID。

HID 预设指定 VID/PID、接口和 Usage，以及 Report ID、位偏移、宽度、符号、字节顺序和计数器模式。在 **Details** 中，**Add HID field** 添加可编辑字段；**Read feature…** 按报告 ID 和长度读取 Feature Report。**Input monitor** 显示接收字节。字段偏移包含 Report ID 字节，无编号报告统一补零字节。字段含义必须来自厂商协议或用户自行采集。

反馈规则根据应用实际状态发送 MIDI 或 HID Output/Feature Report，相同内容合并，每条规则最高 30 Hz。重连时初始化并刷新反馈，断连释放按住状态。性能控制在显示循环之外处理；资料库浏览和加载按顺序唤醒应用事件循环。

## 社区预设

通过 **Manage → Import…** 导入社区预设。预览会显示作者、版本、许可证和设备要求；适配器还会列出允许读写的报告。导入只保存一个副本，需要另行应用到设备。新版本不会自动替换正在使用的预设，保留旧副本即可切换回去。

固定报告格式使用声明式映射。需要校验和、握手、序列跟踪或定时器的 HID 协议，可以使用独立的 Wasm 适配器接口。控制列表会将这些映射标为 **Adapter**，源代码由作者维护。适配器没有文件或网络访问权限；发生错误时，宿主停止适配器并释放按住状态，重新应用预设即可重启。

开发者可使用[控制器 SDK 和示例](https://github.com/backrunner/mixless/tree/main/crates/mixless-controllers)校验、打包、录制和回放报告。测试夹具可以在没有设备时检查预期动作和反馈字节。导入作者提供的测试记录不会自动获得实机验证状态。社区预设需要附带来源和许可证信息。

## 内置预设与验证状态

预设库提供 **12 个依据厂商官方 MIDI 消息表独立编写的预设**：

| 厂商 | 型号 |
| --- | --- |
| Pioneer DJ / AlphaTheta | DDJ-200、DDJ-400、DDJ-FLX4、DDJ-SB、DDJ-SB2、DDJ-SB3、DDJ-RB、DDJ-REV1、DDJ-WeGO3 |
| Hercules | DJControl Inpulse 200、DJControl Inpulse 300（初代） |
| Numark | DJ2GO（初代） |

按型号覆盖有协议依据的传输控制、混音、Jog、浏览加载、Hot Cue 和反馈。选中预设后展开 **Details** 可查看验证状态；**Preset notes** 提供使用说明和功能限制。SB2/SB3/REV1 使用 Deck 1/2；SB/SB2、WeGO3 和 Inpulse 200 每个 Deck 提供四个 Hot Cue。不默认覆盖其他 Pad 模式、效果和后续硬件代际。

全部内置预设均标为“**协议实现，实机验证待完成**”。Jog 灵敏度为待实机校准值；可以先使用预设，后续补充设备验证，但不能视为硬件已认证。声卡和通道对请在音频设置中选择，应用预设不会自动切换输出。

## 控制器声卡

在 **Audio I/O** 分别选择 Master、Headphones 设备和通道对。四通道控制器可使用 Master 1/2、Cue 3/4，同设备只创建一个音频流。通道重叠或超出范围会被拒绝，其余通道输出静音。**Details → Audio routing…** 只填写建议配置，需在 Audio I/O 中点击 **Apply** 才会切换输出。监听设备故障不会将 Cue 信号转入主输出。
