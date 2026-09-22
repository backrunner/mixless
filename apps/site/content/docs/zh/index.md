---
title: "第一次混音"
description: "安装 mixless，导入音乐，开始一场连贯的混音。"
order: 1
image: /images/social.png
imageAlt: "mixless 双唱盘工作台"
imageWidth: 1200
imageHeight: 630
imageType: image/png
---

mixless 是原生 macOS DJ 工作台。你可以亲手混音，让 AutoMix 完成过渡，也可以随时在两者之间切换。

## 安装

前往 [直接下载](https://mixless.alkinum.com/download) 获取安装包。打开 DMG，将 **mixless** 拖入 **Applications（应用程序）**。应用面向 **macOS 12 及以上版本**，支持 Apple Silicon 和 Intel。下载优先提供最新正式版；尚无正式版时提供最新 beta。[其他下载选项](https://mixless.alkinum.com/zh#downloads)包含内置模型版、beta、历史版本、校验文件与源码。

如果需要从源代码构建，请阅读[开发指南](/docs/development)。

## 导入几首音乐

在资料库中选择 **+ Files** 或 **+ Folder**。文件夹导入会包含子文件夹；重复导入同一路径不会重复添加曲目。每个文件夹都有独立的歌单。

导入过程中，曲目会先出现在列表里，随后在后台准备波形、曲速、调性和音乐结构。在 Apple Silicon 上，原生分轨与音符分析使用经过校验的模型：内置模型版已附带，标准版在首次使用时下载。音频处理在你的 Mac 上完成。

## 检查输出设备

按 **Cmd+,** 打开 **mixless → Preferences…（设置）**。在 **Audio I/O** 中选择主输出设备并应用，从舒适的音量开始。你也可以配置独立耳机输出，详见[音频设置](/docs/audio)。

## 开始播放

1. 选中要播放的歌单。
2. 拖动曲目，安排播放顺序。
3. 按下 **AUTO**。如果唱盘为空，mixless 会载入并播放列表中的第一首可播放曲目。
4. 后续曲目可以顺序或随机播放。AutoMix 会循环整个歌单。

也可以把曲目拖到 A 或 B 唱盘，按 Play 手动混音。音频完成解码和波形准备后即可播放，节拍和调性分析会随后补齐。

## 加入自己的想法

设置 Hot Cue，为 Cue 指定 IN 或 OUT 角色，在 AutoMix 运行时调整 EQ 或推子。手动修改会接管对应的自动化控制项；手动载入曲目则会接管 AutoMix。

接下来可以阅读 [AutoMix](/docs/automix)、[唱盘与效果器](/docs/decks)，或查阅[快捷键](/docs/shortcuts)。

## 更新

发行版启动时会检查所属的 stable 或 beta 通道，也可以选择 **mixless → Check for Updates…** 手动检查。更新安装完成后，按照提示重启应用。开发版本不自动检查更新。
