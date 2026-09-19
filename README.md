# replayer

使用 Rust、FFmpeg、egui/WGPU 和 CPAL 开发的桌面视频播放器，目前主要在 Windows 上验证。

## 功能

- 播放、暂停、拖动定位、音量控制和全屏；`Space` 播放/暂停，左右方向键快进/快退，`F` 切换全屏，`Esc` 退出全屏，`M` 静音。
- 顶部和底部工具栏自动隐藏，鼠标靠近边缘时显示。
- Windows D3D11VA 硬件解码、软件回退，以及 GPU YUV/NV12 转色。
- 中英文界面；语言设置同时控制 AI 字幕的目标语言。
- 兼容 OpenAI Chat Completions 音频输入的字幕生成：分段处理、默认 3 路并发、取消、进度显示和 SRT 导出。
- 语言相关的字幕标点、两行排版、阅读速度检查和白字黑描边。

## 构建

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

设置 `REPLAYER_SOFTWARE=1` 可强制软件解码；`REPLAYER_STATS=1` 可输出播放诊断信息。

## AI 字幕

复制 `.env.example` 为 `.env`，填入自己的密钥、兼容接口地址和支持音频输入的模型。示例配置使用 `qwen/omni-flash`；它需要 data URL 形式的音频输入，程序会自动适配。

```powershell
Copy-Item .env.example .env
```

打开视频后，选择 **字幕 → 生成 AI 字幕**。只有主动生成时才会发送音频；密钥不会存入界面设置。**设置 → 语言** 控制界面和字幕语言，生成并发数可在 1–6 之间调整。

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
