# replayer

> 本项目由 Qwen3.8 Max 开发。

使用 Rust、FFmpeg、egui/WGPU 和 CPAL 开发的桌面音视频播放器与漫画阅读器，支持 Windows 和 macOS（Apple Silicon / Intel）构建。

## 功能

- 漫画模式：MangaYun 分源搜索、详情目录、收藏、历史与断点续读、收藏更新红点；条漫/左右分页、右侧章节栏，以及通过图像编辑模型逐页翻译和原译对照，详见 [漫画阅读](COMICS.md)。

- 音乐与视频自动切换界面；音乐显示封面、标题、歌手和专辑，控件常驻。播放页与队列优先读取内嵌标签，缺失时读取在线音乐的本地元数据缓存。
- 多文件打开、混合播放队列、上一首/下一首、播完自动继续及队列排序；详见 [音乐与通用播放](MUSIC_PLAYBACK.md)。
- 在线音乐搜索、选择音质、下载并播放；接入 ChKSz 的网易云、QQ 音乐和酷狗接口，支持扩展其他源，详见 [音乐源配置](MUSIC_SOURCES.md)。
- WebDAV 云同步：同步 AI 提供商、模型配置、漫画与在线音乐收藏及进度；API Key 可选加密同步，纯本地媒体和文件路径不上传，详见 [云同步](CLOUD_SYNC.md)。

- 播放、暂停、拖动定位、音量控制和全屏；`Space` 播放/暂停，左右方向键快进/快退，`F` 切换全屏，`Esc` 退出全屏，`M` 静音。
- 顶部和底部工具栏自动隐藏，鼠标靠近边缘时显示。
- Windows D3D11VA 硬件解码、软件回退，以及 GPU YUV/NV12/P010 转色，支持 8/10/12/16 位 YUV420 源。
- 中英文界面；语言设置同时控制 AI 字幕的目标语言。
- 兼容 OpenAI Chat Completions 音频输入的字幕生成：默认 MP3、120 秒分段、2 路并发，支持播放位置优先调度、取消、进度显示和 SRT 导出。
- 语言相关的字幕标点、两行排版、阅读速度检查和白字黑描边。
- Windows 安装包与便携版；设置中的“关于”支持版本检查、下载更新、校验并安装重启。
- macOS 应用包、DMG 与 ZIP，内置播放依赖，可拖入“应用程序”安装。
- 磁链文件列表解析、选片边下边播、暂停/恢复下载和按播放位置取块；AI 字幕仅读已下载缓存，说明见 [MAGNET_PLAYBACK.md](MAGNET_PLAYBACK.md)。

## 构建

GitHub Actions 同时构建 Windows x64，以及 macOS Apple Silicon / Intel 的独立安装产物。推送与 Cargo 版本一致的 `v*` 标签，所有平台通过测试后才会发布正式版本；普通分支构建仅上传构建产物。流程和自动更新说明见 [RELEASING.md](RELEASING.md)。

### Windows

需要 Rust 工具链、Windows C++ Build Tools / SDK、FFmpeg 9 共享库开发包，以及 libclang。

`.cargo/config.toml` 默认从以下位置读取本地依赖：

```text
.deps/
  ffmpeg-n9.0.1-84-g946fcce07b-win64-lgpl-shared-9.0/
    bin/
    include/
    lib/
  libclang/
```

可以调整配置路径，或者设置 `FFMPEG_DIR` 和 `LIBCLANG_PATH` 环境变量指向自己的安装目录。FFmpeg 包需要包含开发头文件和链接库，仅有命令行程序不足以编译。构建脚本会将 FFmpeg 的 DLL 复制到输出目录。

```powershell
cargo build --locked
cargo run --locked -- "path/to/video.mkv"
```

本地构建预览统一使用固定名称 `target/debug/replayer.exe`，参考 torto 的
Cargo 开发流程，不再按功能创建新的可执行文件名：

```powershell
./scripts/preview.ps1
./scripts/preview.ps1 -Media "path/to/song.flac"
./scripts/preview.ps1 -NoLaunch
./scripts/preview.ps1 -Stats -Media "path/to/video.mkv"
```

脚本会关闭本项目原有的 debug 预览、重新构建，再启动新版本；构建失败不会启动旧版本。
QQ 音乐文件支持范围和密钥导入方式见 [音乐播放说明](MUSIC_PLAYBACK.md#qq-music-local-files)。

设置 `REPLAYER_SOFTWARE=1` 可强制软件解码；`REPLAYER_STATS=1` 可输出播放诊断信息。
预览脚本的 `-Stats` 将播放诊断（显示帧率、像素格式及上传耗时）保存到 `target/preview-stderr.log`。

### macOS

需要 macOS 15 或更新系统、Rust、Xcode Command Line Tools 和 `pkg-config`；Intel 构建还需要 `nasm`。

```bash
brew install pkg-config nasm
./scripts/build-macos.sh
./target/debug/replayer "path/to/video.mkv"
```

首次构建会从校验过 SHA-256 的源码编译 FFmpeg 9.0.2 和 LAME 4.0，并缓存到 `.deps/macos-aarch64` 或 `.deps/macos-x86_64`。
无须安装 Homebrew FFmpeg。继续执行 Cargo 命令前，先载入脚本打印的 `env.sh` 路径，覆盖 Windows 的默认依赖配置：

```bash
source .deps/macos-aarch64/env.sh  # Intel 改为 macos-x86_64
cargo clippy --locked --all-targets -- -D warnings
REPLAYER_SETTINGS_FILE="$PWD/target/macos-test-settings.json" cargo test --locked
./scripts/build-macos.sh --release
python3 scripts/package-macos.py
python3 scripts/test-macos-package.py
```

完整自检会只读挂载 DMG，系统可能申请 `diskutil` 权限；不希望在本机挂载时，可加 `--skip-dmg-mount`，仍会验证 DMG 校验和并完整测试 ZIP 中的应用。CI 始终执行完整验证。

产物在 `dist/`：`replayer-VERSION-macos-aarch64.{dmg,zip}` 或 `replayer-VERSION-macos-x86_64.{dmg,zip}`，以及各架构的 SHA-256 清单。
ZIP 解压后得到 `replayer.app`；DMG 内可将应用拖入“应用程序”。播放依赖全部随应用携带，安装后不需要 Homebrew。

默认构建使用 ad-hoc 签名，尚无 Apple Developer ID 公证，下载后的首次打开可能需要在“系统设置 → 隐私与安全性”中确认。
持有签名身份与公证配置时可使用发布文档中的可选参数。macOS 当前通过“关于”中的链接手动下载更新；系统密钥持久化和 Windows 安装器式自动升级尚未接入。

## AI 字幕

在 **设置 → AI 提供商** 添加 OpenAI 兼容服务，填写名称、接口地址和 API Key；获取模型列表后勾选要使用的模型，也支持手动添加模型 ID。在 **AI 字幕** 中选择支持音频输入的模型，在 **漫画相关** 中独立选择支持图片编辑的模型。

打开视频后，选择 **字幕 → 生成 AI 字幕**。只有主动生成时才会发送音频。Windows 密钥由系统凭据管理器保存，普通参数保存在用户配置目录；重启后仍可使用。界面语言同时控制字幕目标语言，生成并发数可在 1–6 之间调整。

客户端、开发预览和字幕 CLI 均不读取 `.env` 或服务环境变量。仓库的 `.env.example` 仅用于显式运行的真实接口测试，默认测试使用本地模拟服务。

也可通过命令行生成全部或指定时段的字幕：

```powershell
cargo run --locked -- --subtitles input.mkv output.zh-CN.srt --language zh-CN --concurrency 3
cargo run --locked -- --subtitles input.mkv sample.en-US.srt --language en-US --start 600 --duration 60
```

CLI 不覆盖已有 SRT。模型输出的文字和时间戳仍需校对；当前硬解管线也尚未实现跨图形 API 的零拷贝。

## 验证与文档

```powershell
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo test --locked gpu_yuv_readback -- --ignored --nocapture
./verify.ps1
```

`verify.ps1` 使用本地 FFmpeg 生成测试素材；`-Software` 强制软解，`-Stress` 增加大文件测试。普通单元测试中的 API 调用使用本地模拟服务，不消耗模型额度。

- [播放架构](ARCHITECTURE.md)
- [字幕配置与处理流程](SUBTITLES.md)
- [字幕显示规范与参考来源](SUBTITLE_STYLE.md)
- [实际媒体测试记录](DARK_MATTER_TEST.md)

密钥、媒体素材、本地依赖和构建产物不包含在仓库中。测试记录中的 `target/` 样本链接指向本地生成结果。
