# Android 开发恢复方案与 QwenDev 交接

调研日期：2026-10-29（US/Pacific）。本文记录交接时的实测状态；实现与验收结果由接手 agent 补充。原始问题记录在上级目录 `app-dev.md`，不覆盖该历史记录。

## 目标与边界

在现有 Kotlin Compose + Rust `pordo-core` + remdia Android 实现上继续，先交付可靠的音乐、漫画使用路径及开发工具。音视频和图片继续通过 remdia 使用系统能力；不引入 FFmpeg、额外图片解码库，不改变可组合插件图，不扩展到全量视频、AI、同步功能。

保留其他 agent 的全部未提交修改；目前 `android/`、`crates/pordo-android/`、构建脚本及部分 core 修改尚未提交。已提交的 core 提取为 `cd302aa`，SQLite 裁剪已在 `9c5e8b5`，均不是待丢弃的本地修改。先读仓库 `AGENTS.md`，按阶段提交自己的完整改动；此任务不包含推送、合并、发布或跟踪旧 Actions。

## 已核实的当前状态

| 项目 | 证据与判断 |
| --- | --- |
| Android Rust 当前不可编译 | 正确设置 NDK 后运行 `cargo check --locked -p pordo-android --target aarch64-linux-android`，稳定得到 4 个错误。日志：`target/android-audit/current-check.log`。历史 APK 不能证明当前源码可编译。 |
| JNI 版本边界未完成 | 桥接使用 jni 0.21.1，verifier 0.7.0 使用 jni 0.22.4。错误涉及 VM 原始指针类型、attach 回调错误类型、class loader API 和 get_java_vm 的 Result。不要依据旧版示例猜 API。 |
| TLS 不仅缺 Rust 初始化 | 锁定的 companion 为 `rustls-platform-verifier-android 0.1.1`。Gradle 未引入其 Maven/AAR，也没有对应 keep 规则；旧 APK 的全部 DEX 未发现 `org/rustls/platformverifier`，而 `dev/remdia` 已存在。 |
| 初始化失败被吞掉 | `init_tls_verifier()` 仅记录错误，返回 `()`；`nativeInit()` 继续成功，Kotlin `ready=true`，后续 HTTPS 仍可能触发 verifier 未初始化 panic。 |
| remdia 单库路径基本正确 | `PordoApplication` → `Native.initialize` → `RemdiaBootstrap`；桥接复用 remdia Java 类，APK 排除了独立 `libremdia_sdk.so`。应保持这条路径。 |
| 旧 debug APK 很大 | 旧包 `android/app/build/outputs/apk/debug/app-debug.apk` 为 295,247,594 bytes，业务库 `libpordo_android.so` 为 119,874,896 bytes。另有正常的 Compose `libandroidx.graphics.path.so`。这不是 release 包体结论。 |

编译错误位于当前桥接 `lib.rs` 约 171–181 行：E0308、E0277、E0061、E0308。修复后需继续实际链接、组装、安装和运行，不能把 `cargo check` 作为最终验收。

## P0：先恢复可用链路

1. **修复 TLS 完整接入。** 以 Cargo.lock 和本地下载的 0.7.0 源码为准，优先使用 `android::init_with_env`，避免手工重建错误的 class loader。0.7.0 实际使用 jni 0.22 的 `Env`，源码文档中的某些旧签名示例已经过时。保留小范围、Android 专用的 0.21/0.22 适配；不要为了这一步迁移整个 JNI 桥接。
2. **正确处理所有权及线程。** 仅在有明确有效性依据处转换 JNI 原始指针；不 transmute 两个版本的 Rust wrapper。遵守 local/global reference 生命周期与 attach 回调约束，必要时克隆 local ref。使用 applicationContext，避免长期持有 Activity。Context 的 class loader 与 `context.getClass().getClassLoader()` 不等价，应使用支持的初始化入口。
3. **补齐 Android companion。** 用 `cargo metadata --locked --filter-platform aarch64-linux-android` 找到锁定 companion 包，再定位其 `maven/`，加入 Gradle 仓库并显式依赖匹配版本（当前 `rustls:rustls-platform-verifier:0.1.1`）。不硬编码个人 Cargo registry 哈希目录、不使用 `latest.release`，不照抄新版 0.2 分发说明。加入 verifier JNI 所需保留规则，检查最终 DEX。无法解析依赖时给出明确错误。
4. **初始化失败必须上报。** helper 返回 Result，nativeInit 用 `?`；失败抛出 Java 异常且 Kotlin 不设 ready。定义重复调用、失败后重试的行为。启动入口呈现可诊断的失败状态，不能继续发网络请求，也不能无提示崩溃。保持所有 JNI 导出 panic 防护，但不能把 catch_unwind 当作初始化修复；不能捕获 panic=abort。
5. **验证实际 HTTPS。** 漫画搜索 → 详情 → 页面列表 → 图片显示；网络失败与服务端失败进入 UI 错误态、允许重试，无 Rust abort/未处理 JNI 异常。外部站点失败需与应用实现失败区分。保留证书校验，不以跳过 TLS 验证解决问题。

## 开发工具：减少反复手工试错

新增统一入口 `scripts/android-dev.py`，复用并修正现有 `scripts/build-android.sh`，不要建立第二套互不一致的构建流程。优先实现 doctor/check/build/run/logs/smoke/audit；measure 可在 P1 完成。未实现的子命令应明确报错，不能空跑报成功。

| 子命令 | 最小职责 |
| --- | --- |
| `doctor` | 打印 SDK、NDK、JDK、Rust target、Gradle wrapper、companion Maven、remdia AAR 来源/哈希；发现设备后打印 serial/API/ABI/page size。缺失依赖给出具体修复建议。 |
| `check` | 在子进程设置 Android NDK 环境，执行 locked Android cargo check；可选 clippy。失败不得继续复制旧 so 或安装旧 APK。不能仅检查宿主机 cfg 后报告 Android 通过。 |
| `build` | debug/release 参数；调用 Rust + Gradle 完整构建。验证 AAR 来源与 remdia Git pin 的一致性，替换“目标存在就不复制”的陈旧逻辑。产物和缓存尽量收敛到 checkout 的 `target/android/`，保留旧包作基线。 |
| `run` | 显式 `--serial`、可选 `--adb`；在有限期限内等待启动，校验设备 ABI/API，`install --no-incremental -r` 后启动本次成功构建的包，记录 hash 和 PID。不清空应用数据或终止无关模拟器。 |
| `logs` | 按包/PID及 crash buffer 收集日志，保留 panic/Java 异常完整根因，提供 llvm-addr2line/ndk-stack 符号化路径。构建符号和发布符号裁剪分开管理。 |
| `smoke` | 调用真正的 Compose/Android instrumentation 测试，输出用例、结果、日志及截图。用 testTag/语义选择器和状态等待，避免固定坐标及盲目长 sleep。系统权限/文件选择器用 UI Automator。外网集成检查单列。 |
| `audit` | 审计最终 APK 中 verifier 与 remdia Java 类、JNI 声明和导出匹配、业务 so 唯一性、16 KB ELF/ZIP 对齐、哈希和体积分项。允许 Compose 等合理系统桥接 so；不能仅以 so 数量判错。 |
| `measure` | 分开记录 debug/release，冷/热构建耗时，APK/AAB及 native/DEX/resources 大小，典型场景的应用 PSS/RSS/Java/native/graphics 内存。记录设备、page size、命令、采样点与原始数据。 |

测试输出统一到 `target/android-review/`、`target/android-audit/`；在 `docs/android.md` 写好一条从 check 到运行和复测的命令链，以及工具覆盖范围。不在仓库提交 SDK、AVD、APK、大日志或私有配置。

### 复用本机已存在的工具

- SDK：`../remdia/.deps/android-sdk`，NDK `28.2.13676358`，platform/build-tools 35。
- JDK 21：`../remdia/.deps/android-jdk/Contents/Home`；使用仓库 Gradle wrapper，不依赖系统 Gradle。
- 若 SDK 自带 ADB 37 在当前 macOS 发生 libunwind 崩溃，可显式使用已下载的官方 36.0.2：`../remdia/.deps/android-platform-tools-36/platform-tools/adb`。不要全局替换 ADB 或杀其他 agent 的 server；需要隔离时单独配置 server socket。
- 已有 API 28、API 35、16 KB AVD 在 `../remdia/target/android/avd/`，优先复用；已有 `emulator-5554` 可能属于另一会话，先枚举再选择，不重置、不全局 kill。
- `/Users/lchris/.android/bin/android-cli` 已存在。新建 AVD 先查看 CLI 实际 help 或使用 Android Studio Device Manager，不再手写不完整 AVD 配置。
- remdia 的 `scripts/build-android.py`、`audit-android.py`、`test-android.py` 和 `docs/android.md` 可供参考；SDK 测试通过不等于 pordo APK 测试通过。

## P1：功能正确性和资源占用

先完成 P0，再以独立改动处理以下已从源码定位的问题；不要把范围扩大成 UI 或底层库重写。

1. **播放结束计数。** `AudioEngine.writeFrame` 把 `AudioTrack.write(float[])` 返回的 float 数累加进 writtenFrames，却与以音频帧为单位的 playbackHeadPosition 比较；立体声 EOF 可能一直等待。正确计算交错样本/声道数，处理部分写入、负错误码与无符号 32 位播放头。覆盖短 mono/stereo、EOF、暂停、seek、关闭及重开。审查正常 EOF 的资源释放；读取阻塞时，排在同一 Handler 后面的 close/seek 无法抢占，需明确取消或超时策略，同时保持 native session 线程归属。
2. **封面协议不一致。** `nativeMediaArtwork` 目前发送 width/height + RGBA；`Container.bitmap()` 强制读取 tiles，而 AudioEngine 对 artwork 直接调用该方法。统一容器协议或显式区分 raw 与 tiles，增加小型协议/像素测试，不能把异常封面变成整首播放失败。
3. **长图内存。** Kotlin 又把分片拼回整幅 Bitmap，并同时保留 ByteArray、payload 拷贝和临时 tile Bitmap。改为视口/窗口分片显示、限制缓存与并发、及时释放资源，避免同一长图多份驻留。校验尺寸、stride、offset、长度和整数溢出；透明像素/premultiplied alpha 以实测确认，不能靠格式名称猜测。记录相同长图连续翻阅前后和峰值内存，不预设未经测量的绝对预算。
4. **发布体积与构建耗时。** 先做可运行的 release 基线，再启用/验证 R8 与资源裁剪，保留两套 JNI 的必要规则。当前 material-icons-extended、debug native symbols 和关闭 minify 值得分项测量。旧 ZIP 各 entry 压缩大小总和与文件总长相差较大，原因尚未确认；在隔离输出中做一次干净组装比较，不能直接归咎 remdia，也不能每轮 clean 重编。release 验证包括实际启动与 HTTPS，不只看文件变小。

## 验收与阶段交付

- P0：格式检查、Android Rust 检查和完整 APK 构建通过；最终 APK 包含两个系统桥接所需 Java 组件，排除独立 remdia SDK；当前源码在 API 35 设备完成首次启动和真实 HTTPS 漫画搜索/阅读。错误输入、离线/失败请求、初始化失败有可观察结果，无 abort，ready 状态正确。
- 音乐：至少一份本地短 PCM/支持的压缩音频，播放、自然结束、暂停恢复、seek、关掉重开；没有封面不能阻止播放。网络歌曲源需要凭据时不得把缺失凭据算作实现通过。
- 兼容：在 API 28 和 16 KB API 35 AVD 跑启动/核心路径；不能运行的项目明确写未验证和原因。不能以已有 SDK 兼容结果替代 App 验收。
- 桌面回归：按 AGENTS 加载 `.deps/macos-aarch64/env.sh` 后运行 `cargo fmt --all -- --check` 与 root 默认成员的 `cargo test --locked`，记录实际结果；另行执行 Android target 检查。Windows 无运行环境时明确标记未运行。
- P1：上述播放/图片协议问题的回归测试、长图内存数据、release 体积和构建耗时数据及 audit 结果。先测再比较，区分静态源码判断与设备观察。
- 分阶段提交，记录 commit、命令、测试结果、APK 路径/hash、尚未验证事项；不要一次重复粘贴整份开发过程。遇到相同错误不反复盲改：先读锁定版本源码/官方文档、保留完整日志，再做最小修复。

## 一手资料

- [rustls-platform-verifier 0.7.0 Android 集成说明](https://docs.rs/crate/rustls-platform-verifier/0.7.0)：Android companion 与 Gradle 集成。
- [0.7.0 Android 实现源码](https://docs.rs/rustls-platform-verifier/0.7.0/src/rustls_platform_verifier/android.rs.html)：初始化前置条件和实际 API；同时参照本地锁定版本 jni 0.22.4 源码的线程及 reference 约束。
- [Android JNI tips](https://developer.android.com/ndk/guides/jni-tips)：VM、线程、reference 和 class loader。
- [AudioTrack](https://developer.android.com/reference/android/media/AudioTrack)：write 与播放头单位及生命周期。
- [UI Automator](https://developer.android.com/training/testing/other-components/ui-automator)、[ADB](https://developer.android.com/tools/adb)、[APK Analyzer](https://developer.android.com/tools/apkanalyzer)：设备测试、诊断与产物审计。
