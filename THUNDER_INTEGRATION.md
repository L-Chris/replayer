# 迅雷边下边播接入调研

调研范围：公开技术资料、当前 replayer 源码、本机迅雷安装文件及实际播放调用链。已通过迅雷界面启动测试任务的边下边播，未修改迅雷配置、注册表或程序文件。

## 2026-09-19 实机与社区研究补充

- 用户确认设置中的播放器关联仅针对下载完成的视频，未找到边下边播的第三方播放器设置。
- 在本机 25.1.10.1612 点击边下边播后，原生播放组件连接 DownloadSDKServer 的本地 HTTP 服务。本次端口为 9080，不应硬编码。
- 通过只读诊断取得当前任务播放 URL，FFprobe 成功识别 H.264 1920×960、六声道 E-AC-3、字幕流及 3206.432 秒时长。这证明实际媒体地址可由外部 FFmpeg 读取，尚未证明持续缓冲、缺块跳转或按钮接管可用。
- URL 含运行时任务标识及会话参数，不能用 TaskDb 的持久任务 ID 替换。移除参数会导致请求失败；关闭原生播放会话后曾返回 404，因此需要进一步验证会话的维持方式。
- 当前临时文件是预分配文件，逻辑长度不是已下载字节数。不能用简单文件尾跟随实现可靠边下边播。
- 完整播放 URL 和诊断材料只保存在被忽略的 target/thunder-probe 下，不写入本文或提交到仓库。进程内存读取仅用于本次验证，尚未选作产品接入方式。

社区证据分为三类：

1. [fondoger/xmp-2-potplayer](https://github.com/fondoger/xmp-2-potplayer)：已有旧版按钮转交实现，替换 XMP.exe 并转发命令行播放地址。当前安装结构不同，不能直接复用。
2. [tomjiu/smart-downloader 的迅雷引擎研究](https://github.com/tomjiu/smart-downloader/blob/main/docs/research/xunlei/xunlei_engine_research.md)：研究对象为 25.0.90.1592，与当前版本较接近；[导出清单](https://github.com/tomjiu/smart-downloader/blob/main/docs/research/xunlei/sdk_export_inventory.md)列出 XL_GetFilePlayInfo、XL_QueryPlayInfo 等接口。已检查研究文档、bindings.rs、loader.rs、handle.rs 和反编译索引，未找到可直接复用的播放信息调用实现或现有桌面任务按钮接管实现。导出清单不等于已验证 ABI，也不等于官方开放接口。
3. [Bleu404/ThunderPanLink](https://github.com/Bleu404/ThunderPanLink)：针对迅雷云盘网页提取链接并转交播放器，与桌面端正在下载的 BT 任务不是同一入口。

据此，推荐继续研究当前播放器插件到下载 SDK 的播放信息请求及会话生命周期，在独立适配层实现版本检查与转交。当前已验证“媒体流可读取”，尚未解决“点击按钮自动转交且原生播放器退出后仍持续播放”。本轮未发现适用于本机版本的现成替换工具；这不代表社区不存在其他实现。

## 结论

应把“迅雷按钮能否唤起 replayer”和“replayer 能否连续读取未下载完成的数据”作为两个独立问题验证。优先路线是复用迅雷提供的可播放 HTTP 地址，通过小型适配器转交给 replayer；只有确认数据是顺序增长文件时才采用文件跟随读取。

当前不能承诺本机版本通过一份 xmp.ini 就能完成接入。先验证入口，再实现渐进播放，最后处理版本适配与安装回滚。

## 已确认与未确认

本机 `D:/Thunder/program/thunder.exe` 的文件版本和应用 package.json 均为 `25.1.10.1612`。卸载注册项还存在指向同一目录的 `12.4.10.3940` 记录，因此判断以实际文件为准。

安装目录包含：

- `program/player/APlayer.dll`
- `program/player_helper.node`
- `program/resources/app/plugins/player-plugin.asar`
- `program/SDK/DownloadSDK.dll`、`DownloadSDKProxy.dll`

未发现旧方案使用的 `Program/XMP/XMP.exe` 和 xmp.ini。播放器插件包未能按普通明文 ASAR 索引读取。这些事实说明旧版替换方法不能直接当作本机适配依据，但不等于已经证明新版完全不支持外部播放器。

下载 SDK 二进制中能找到 `XL_GetFilePlayInfo`、`XL_GetUniversalPlayInfo`、`DownloadedRanges` 等字符串。这只是后续调查线索，未确认函数签名、调用约定、实际返回值或是否对第三方开放，不能直接作为已支持 API 使用。

### 公开证据

1. [xmp-2-potplayer 项目](https://github.com/fondoger/xmp-2-potplayer)提供了替换旧迅雷 X 的 XMP.exe 的适配器。[源码](https://github.com/fondoger/xmp-2-potplayer/blob/master/main.go)将第一个参数当作播放地址、第七个参数当作名称，再调用 PotPlayer；HTTP 分支还修改了地址。它证明旧版存在外部转发路线，不能证明当前版本采用同样参数或传输协议。不能照搬硬编码参数位置和 URL 拼接。
2. [迅雷影音官方帮助](https://video.xunlei.com/aq_page.html)说明了边下边播功能，但本次未找到针对上述本机版本的公开第三方播放器接入契约。
3. [迅雷开放平台](https://open.xunlei.com/)提供下载 SDK 接入能力；这与“接管已安装迅雷的现有下载任务”不是同一个已证实的接口。
4. [Windows 默认程序机制](https://learn.microsoft.com/en-us/windows/win32/shell/default-programs)适用于文件/协议关联。只有调用方使用这种启动机制时关联才有用；不能据此推断内嵌播放器按钮会走系统默认播放器。

## 路线比较

| 路线 | 必要条件 | 评价 |
| --- | --- | --- |
| 配置外部播放器，再由适配器转发 HTTP 地址 | 当前迅雷版本有可用外部入口和可复用播放地址 | 首选，复用下载和选块能力 |
| 旧版 XMP 启动适配器 | 实际版本仍会启动外部 XMP.exe | 历史上有实现；需要版本检查、备份和恢复 |
| 直接跟随下载文件 | 数据确实顺序追加，初始化信息已经可读 | 只适用于受限场景 |
| 下载引擎数据桥 + HTTP/自定义 AVIO | 可获得已完成区间、完成状态、按需取块接口 | 能处理不连续下载，但实现和版本维护成本更高 |
| 自己接入新的下载引擎 | 另行创建、维护下载任务 | 属于另一种产品方案，不能自动满足接管迅雷现有按钮的目标 |

如果当前版本只使用内部插件且没有可转交的入口，需要先验证版本专用适配方式。注册一个 replayer URI 协议本身不会让迅雷自动调用它。

## 推荐架构（待入口验证）

```text
迅雷的“边下边播”动作
    → 版本适配器：规范化地址、标题、来源和会话信息
    → replayer 单实例入口 / 命名管道
    → ProgressiveSource：HTTP 或已知可读区间的数据源
    → FFmpeg demux + 现有音视频解码/渲染
```

适配器不解析命令行为 shell 脚本，不把文件名拼接到 URL 中；标题与数据地址分开。日志应隐藏播放地址中的鉴权查询参数。下载优先级、任务生命周期仍由迅雷负责，适配器只在已确认的接口上交互。

### 数据源必须区分三种情况

- **可复用 HTTP 流**：验证是否支持 Range/206、数据未到时如何等待、链接或请求头是否有会话限制，以及原播放器关闭后服务是否仍然存在。
- **顺序追加文件**：FFmpeg 的 `file` 协议有 `follow` 选项，可在文件尾等待后续写入，并通过中断退出；它不提供缺块调度。[FFmpeg 协议文档](https://ffmpeg.org/ffmpeg-protocols.html#file)
- **预分配或稀疏文件**：文件长度可能已经是最终长度，缺块不一定表现为 EOF。NTFS 已分配范围也不等于已下载并校验完成的范围；需要下载引擎提供真实可用区间。[Microsoft 文件区间说明](https://learn.microsoft.com/en-us/windows/win32/api/winioctl/ni-winioctl-fsctl_query_allocated_ranges)

MP4 的初始化索引也可能位于文件后部。FFmpeg 的 faststart 会将 moov 索引移到头部，但正在下载时不应简单对缓存文件执行重新封装；需要下载端优先取得必要信息。[FFmpeg 格式文档](https://ffmpeg.org/ffmpeg-formats.html)

## 当前项目需要补齐的能力

| 位置 | 当前行为 | 接入后的要求 |
| --- | --- | --- |
| `src/main.rs` | 首个普通参数作为媒体路径 | 结构化打开请求、标题和来源；单实例转发 |
| `src/player/demux.rs` | 普通输入；10 秒读取/定位期限；EOF 后等待新 Seek | 区分缺数据、传输中断、真实结束；可以取消的等待/恢复 |
| `src/player/mod.rs` | 没有 Buffering 状态 | 增加缓冲状态和可播放范围信息 |
| `src/player/session.rs` | 输入 EOF 后排空解码器 | 仅在确认最终结束后排空；缺数据时冻结媒体时钟，恢复后重新同步 |
| 字幕模块 | 另开输入，从头并发处理音频 | 下载中避免争抢文件读取/取块优先级；只处理可用片段，或等待下载完成 |

HTTP 可以按实际服务语义配置 FFmpeg 的重连、请求头和超时选项，但不能把 `reconnect_at_eof` 无条件用于有限时长影片，否则真实结束也可能被当作重连条件。[FFmpeg HTTP 选项](https://ffmpeg.org/ffmpeg-protocols.html#http)

对于未下载位置的 Seek，第一版应显示“等待缓冲”或限制到可用范围；只有下载端确实支持按需优先下载，才能承诺任意位置跳转。

## 实施顺序与验收

**第一阶段：验证调用契约。** 对一次真实的“边下边播”点击，观察进程创建、命令行、配置读取和网络连接。可使用微软 [Process Monitor](https://learn.microsoft.com/en-us/sysinternals/downloads/procmon) 和 [TCPView](https://learn.microsoft.com/en-us/sysinternals/downloads/tcpview)。重点确认是否启动外部程序、是否读取外部播放器配置，以及是否存在属于本次任务的播放地址；不能把任意本地监听端口当作媒体端口。

验收：获得可复现的入口和经过验证的数据访问方式。若这一步不成立，就先停留在兼容性研究，不能宣称已经能自动接管按钮。

**第二阶段：播放器最小渐进播放。** 使用合成媒体和可控慢速 HTTP 服务验证缓冲、恢复、取消和 Range Seek，再连接实际迅雷任务。真实 EOF 与暂时无数据必须分别测试。

**第三阶段：客户端适配。** 只对已验证版本配置外部入口或安装适配器；提供状态检查和回滚。测试更新、重启、暂停下载、恢复下载、缺块 Seek、中文/空格路径、多次点击、下载完成后的正常结束，以及生成字幕时的资源竞争。

当前调研已确定播放器侧的改造点，并验证了本机实际 HTTP 播放流可读取；“一键接管入口”、播放会话维持和缺块读取语义仍需第一阶段的进一步验证。
