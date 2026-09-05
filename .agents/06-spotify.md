# 06 — Spotify 浏览与元数据

Spotify 在 Mixless 里是 **歌单与曲目目录**，不是音频引擎。音频如何落到磁盘见 [09-acquire.md](./09-acquire.md)。

## 产品角色

| 层 | 做什么 | 不做什么 |
|----|--------|----------|
| `mixless-spotify` | OAuth、拉 playlist / saved / search、ISRC 与封面 | 不读、不写任何音频字节 |
| `mixless-acquire` | 用元数据去 YTM 等渠道物化本地文件 | 不自己实现播放 |
| `mixless-engine` | 只播已经在磁盘上的文件 | 不知道 Spotify |

用户原话落地：**歌单用 Spotify 整理很方便，整理本地库不方便**。所以登录后看到的就是用户的 Spotify 库；点导入后由获取层补文件。

## 官方通道（仍成立）

| 通道 | 对 Mixless 的用处 |
|------|-------------------|
| Web API + OAuth Premium | **P0 浏览**。playlist、saved、search、ISRC、封面、时长 |
| Audio Features | 2024-11-27 起对新应用关闭。不依赖 |
| Web Playback SDK | 无 PCM，不能做双碟 DSP。否决 |
| DJ Integration（rekordbox / Serato / djay） | 用户判定申请不到。不排期 |

https://www.spotify.com/us/dj-integration/

## OAuth（PR18 不得猜）

- Dashboard 注册 `http://127.0.0.1/callback` **不写端口**。运行时 ephemeral 端口。
- 2025-11-27 HTTPS 迁移后，**loopback HTTP 仍是文档化例外**。
- Scope：`playlist-read-private playlist-read-collaborative user-library-read user-read-private`。
- `GET /me`.`product` 检查 Premium。不要 `user-read-email`。
- 429：指数退避 1→32 s，尊重 `Retry-After`。
- Token → macOS Keychain，禁止 SQLite / 日志。

## 浏览模型

```
SpotifyPlaylist { id, name, owner, snapshot_id, tracks: [SpotifyTrack] }
SpotifyTrack    { id, isrc?, title, artist, album, duration_ms, artwork, explicit }
```

本地状态：

```
link_status = unmatched | local | acquired | suspect | missing
```

- `local`：对上了用户本来就有的文件。
- `acquired`：获取层新写入 `AppData/acquired/`。
- 导入工作台：`local` / `acquired` 可 load；`suspect` 需确认；`missing` 禁止 load。

## 与本地库匹配（获取之前）

与 09 的 `LocalMatch` 同一套：

```
normalize = NFKC + lower + 去 feat./括号 + 去标点 + 压缩空白
title_sim / artist_sim = 精确相等 ? 1 : 0     # v1；P1 再 Jaro-Winkler
dur_hit = |Δt|≤2s ? 1 : 0
score = 1.0*isrc + 0.5*title + 0.3*artist + 0.2*dur
```

无 ISRC 时自动链接必须三项全中。ISRC 命中锁定。

匹配失败 → 交给 [09-acquire.md](./09-acquire.md) 的渠道链，而不是停在 missing。

## UI 文案

连接对话框：

> 连接 Spotify 用于读取你的歌单。音频会按曲目信息从你选择的获取渠道保存到本机，再进行分析和混音。

设置里获取渠道默认 **YouTube Music**。闭源 CDN 插件单独开关，默认关。

## 当前接入状态（2026-09）

- `mixless-spotify` 已支持公开 playlist URL、`spotify:playlist:` URI、playlist ID 和带地区前缀的 URL。公开嵌入页只保证它返回的曲目；若页面报错或预览不完整，会明确标记 warning，不伪装成完整歌单。
- crate 同时提供带 OAuth bearer token 的 Web API 分页读取实现，保留 unavailable item 并校验 `next` 只允许 `api.spotify.com/v1/`。桌面端当前只接公开 URL，私人歌单 OAuth/Keychain 流程仍需接入后才能开放。
- Spotify 曲目导入不直接播放流：先按 ISRC 或标题+艺人+时长匹配本地文件，再由获取层做 YouTube Music/YouTube 候选筛选。未通过录音版本或时长门槛的条目保留在导入记录中，但不会进入可播放 playlist。

## 许可隔离

- `mixless-spotify`、`mixless-acquire`、`mixless-acquire-yt`：Apache-2.0。
- Spotify CDN / librespot 类实现：`private/mixless-acquire-cdn`，专有，不进开源发行物。
- 开源树 CI 禁止 `librespot` / `zotify` / `votify` 依赖。浏览 crate 禁止出现音频 URL 抓取。
