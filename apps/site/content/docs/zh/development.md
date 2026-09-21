---
title: "从源代码构建"
description: "运行原生应用，或参与官方网站开发。"
order: 9
image: /images/social.png
imageAlt: "mixless 双唱盘工作台"
imageWidth: 1200
imageHeight: 630
imageType: image/png
---

## 桌面应用

安装当前稳定版 Rust 与包含 Metal 编译器的完整 Xcode。单独的 Command Line Tools 不够。

```bash
git clone https://github.com/backrunner/mixless.git
cd mixless
./dev.sh --check
./dev.sh
```

脚本会查找支持 Metal 的 Xcode；显式设置的 `DEVELOPER_DIR` 优先。使用 `./dev.sh --build` 只构建、不启动。开发版本使用正常的本地应用资料库，不自动监视文件变化。

首次构建会优化音频引擎与 UI 依赖，可能需要更长时间。修改后重新运行脚本。**mixless → About mixless** 中可查看版本、Git 修订和构建时间。

## 工作区

| 目录或模块 | 职责 |
| --- | --- |
| `apps/desktop` | 原生 GPUI 界面与应用状态 |
| `mixless-engine` / `mixless-protocol` | 音频图、播放、命令与状态快照 |
| `mixless-library` | 本地资料库、歌单与持久化 |
| `mixless-analyze` / `mixless-stems` | 离线音乐分析与本地推理 |
| `mixless-mixplan` | 音乐过渡规划 |
| `mixless-midi` | 控制器映射与输入 |
| `mixless-spotify` / `mixless-acquire*` | 歌单元数据与可选音频获取 |
| `mixless-tools` | 打包与发行辅助工具 |
| `apps/site` | 本 svedocs 网站 |

桌面 UI 不实现 DSP，实时处理留在音频引擎中。

## 网站

安装 Node.js 22.12+ 和 pnpm，然后从仓库根目录执行：

```bash
pnpm -C apps/site install --frozen-lockfile
pnpm -C apps/site dev
```

网站使用 svedocs 与 svedocs-cli 0.2.1，首页、导航和主题均经过定制。英文内容直接放在 `content/docs` 与 `content/pages`，中文在各自的 `zh` 子目录中按相同路径组织。

```bash
pnpm -C apps/site check
pnpm -C apps/site check:content
pnpm -C apps/site build
```

构建产物位于 `apps/site/build`，可以静态托管。更换域名时，在构建期间设置 `SITE_URL`。修改原始截图或品牌资源后，运行 `pnpm -C apps/site assets`。

## 贡献

检查项与提交约定见 [CONTRIBUTING.md](https://github.com/backrunner/mixless/blob/main/CONTRIBUTING.md)，签名 macOS 发行流程见 [RELEASING.md](https://github.com/backrunner/mixless/blob/main/RELEASING.md)。

mixless 采用 [MPL 2.0](https://github.com/backrunner/mixless/blob/main/LICENSE) 许可证。归属与组件许可见 [THIRD_PARTY_NOTICES.md](https://github.com/backrunner/mixless/blob/main/THIRD_PARTY_NOTICES.md)。
