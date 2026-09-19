# AI 字幕

打开视频后，将鼠标移到顶部，点击 **字幕 → 生成 AI 字幕**。生成在后台进行，已经完成的片段会随播放时间显示；Seek、暂停和工具栏隐藏不会改变字幕时间线。

菜单支持 **显示字幕**、**取消生成** 和 **导出 SRT**。切换视频或关闭播放器会取消旧任务；失败或取消后，已完成的字幕仍可以导出。没有点击生成时不会上传音频。字幕没有自动保存，关闭前请导出需要保留的结果。

**Esc** 退出全屏；**F** 切换全屏。

## 配置

项目 `.env` 已被加入 `.gitignore`。现有 `LLM_API_KEY` 可以直接使用，无需改名。

```dotenv
LLM_API_KEY=你的密钥
LLM_BASE_URL=https://chat.rethinkos.com/v1
LLM_MODEL=qwen/omni-flash
LLM_AUDIO_ENCODING=auto
LLM_SUBTITLE_CHUNK_SECONDS=20
```

- 也支持 `OPENAI_API_KEY`、`OPENAI_BASE_URL`、`OPENAI_MODEL` 别名；优先选择对应 `LLM_*` 名称。同名的进程环境变量优先于 `.env`。
- 可通过 `REPLAYER_ENV_FILE` 指定配置文件。否则从工作目录及可执行文件目录向上寻找 `.env`。
- `LLM_BASE_URL` 是包含 `/v1` 的接口根地址，程序追加 `/chat/completions`。
- `LLM_AUDIO_ENCODING=base64` 使用 OpenAI 音频输入格式；`data_url` 添加 `data:audio/wav;base64,` 前缀。
- `auto` 对名称包含 `qwen` 的模型使用 `data_url`，其他模型使用标准 Base64。本次指定的网关实测要求 data URL；直接传 Base64 会返回 HTTP 400。
- 模型必须支持音频输入。仅支持文本的 OpenAI 兼容模型不能直接识别视频语音。

接口格式依据 [OpenAI Audio in Chat Completions](https://developers.openai.com/api/docs/guides/audio-chat-completions)：Bearer 鉴权、`input_audio` 消息、WAV、SSE 文本响应。也兼容返回普通 JSON 的网关。

## 处理方式与限制

音轨由独立的 FFmpeg 输入和解码器转为 16kHz 单声道 PCM。每次请求上传约 20 秒 WAV（配置范围 5–30 秒），优先在段末的短静音处分割，不把整部电影一次性载入内存。只有音频发送到模型，不上传视频画面或本地文件路径。

在顶部 **设置 → 语言** 选择简体中文或 English；界面文案立即切换，字幕也会生成该目标语言（需要时翻译）。切换语言会取消旧任务并清空旧语言字幕，避免混合显示；需要点击重新生成。设置保存在用户配置目录 `replayer/settings.json`，也可用 `REPLAYER_SETTINGS_FILE` 指定路径。

默认 **3 路并发**，可在设置中调整为 1–6，下一次生成生效。独立线程解码音频，解码队列和请求窗口均有容量上限；网络响应可以乱序结束，但字幕和进度按音频顺序提交。遇到 HTTP 429/5xx 最多重试两次并等待退避，数字型 Retry-After 等待上限 30 秒；网络超时与无效字幕 JSON 不自动重试。

模型返回原文、目标语言文字和片段内起止秒数，程序校验后加上音轨实际偏移，并应用[显示规范](SUBTITLE_STYLE.md)。无音轨会明确报错；数字静音不发起 API 请求。菜单中显示耗时、峰值并发和偏快/过短字幕数量，供质量检查。

**时间戳由模型估计，不是强制对齐结果**；字幕文字和同步精度需要人工检查。无静音的长句可能在分段边界被截断。模型响应不完整、时间戳越界或 HTTP 错误会终止任务并报告原因，不会悄悄换模型或无限重试。

每次 HTTP 尝试超时为 120 秒，连接超时为 15 秒。取消会中断等待中的请求和重试退避；已经由服务端接收的请求可能仍会计费。错误消息会过滤密钥，不记录 Authorization 或音频 Base64。

## 命令行与验证

```powershell
./target/debug/replayer-subtitles.exe --subtitles input.mp4 output.srt
./target/debug/replayer-language.exe --subtitles input.mkv sample.zh-CN.srt --language zh-CN --concurrency 3 --start 600 --duration 60
cargo test --locked subtitles
```

CLI 不覆盖已有 SRT 文件；省略 start/duration 时处理整段媒体。范围测试仍输出原视频的绝对时间戳。CLI 的 language/concurrency 不修改已保存的界面设置。

所有普通测试均使用合成音频或本地 HTTP 服务，不消耗 API 额度。并发测试会等待三路请求都到达后再故意乱序返回，检查真正的并发、上限与时间顺序。
