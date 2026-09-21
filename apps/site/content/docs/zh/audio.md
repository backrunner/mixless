---
title: "音频与预听"
description: "设置主输出和耳机输出，理清增益与电平。"
order: 5
image: /images/social.png
imageAlt: "mixless 双唱盘工作台"
imageWidth: 1200
imageHeight: 630
imageType: image/png
---

按 **Cmd+,** 打开 **mixless → Preferences…**，选择 **Audio I/O**。

## 输出设备

选择主输出、采样率（最高 96 kHz）和缓冲区大小。**Apply audio** 会重新打开音频流，短暂打断播放。设备不可用或格式不受支持时，会报告错误并保留先前配置。

更小的缓冲区可降低延迟，但也减少音频处理的时间余量。如果播放出现断续，可以增大缓冲区，并检查设备支持的设置。

## 耳机预听

为 PFL 选择独立耳机输出。耳机总线位于推子之前，独立于通道推子和主音量；两个设备使用不同采样率时也能工作。耳机音量单独调整。

没有耳机输出时，只会禁用 PFL；Hot Cue 仍然可用。Hot Cue 用于跳转播放位置，PFL 用于监听通道。

## 增益与电平

| 控制项 | 作用 |
| --- | --- |
| 混音器 TRIM | 校准通道电平 |
| 唱盘 GAIN | 调整进入唱盘限制器的增益，范围 −12 至 +12 dB |
| 通道推子 | 设置唱盘限制器之后的音量 |
| Master GAIN | 调整进入主限制器的增益 |
| Master LEVEL | 设置主限制器之后的最终输出音量 |
| 耳机音量 | 独立设置耳机输出音量 |

PFL 监听经过唱盘限制器、但尚未经过通道推子的信号。Master GAIN 和 LEVEL 不改变耳机音量。

较长叠混出现能量损失时，AutoMix 可临时补偿最高 6 dB，**AUTO +…** 显示补偿量。手动调整增益会接管控制，临时补偿在混音结束后归零。

## 常规设置

General 保存波形布局、效果器显示、量化、Key Lock、vinyl/slip、交叉推子曲线与反向，以及滤波/EQ 共振。修改也会立即应用于当前会话。

![mixless 的 General 设置界面](/images/preferences.webp)

目前尚未实现麦克风或 Line In 输入采集。控制器设置见 [MIDI 映射](/docs/midi)。
