# Android 客户端

Android 端复用桌面版的 `replayer-core`：Rust 核心逻辑不变，通过
`crates/replayer-android` 的 JNI 桥接暴露给 Kotlin/Compose 应用。remdia 引擎
静态链接进唯一的业务库 `libreplayer_android.so`；AAR 里的独立
`libremdia_sdk.so` 被 Gradle packaging 排除（`android-dev.py audit` 持续校验）。
首期范围为漫画（Komiic 单源）与音乐播放。

## 工具链

统一入口是 `scripts/android-dev.py`（Python 3.9 兼容），它复用
`scripts/build-android.sh` 完成 NDK 交叉编译与产物发布，不允许出现第二条不
一致的构建路径：

```sh
./scripts/android-dev.py doctor    # 工具链、暂存依赖、设备与 AVD 体检
./scripts/android-dev.py check     # 锁定的 aarch64-linux-android cargo check（--clippy 可选）
./scripts/android-dev.py build     # cargo + gradle，发布 target/android/<profile>/replayer-<profile>.apk
./scripts/android-dev.py run --serial emulator-5554     # 安装、启动、验证 native 就绪与初始化门
./scripts/android-dev.py smoke --serial emulator-5554   # 离线 instrumentation 套件
./scripts/android-dev.py audit     # APK 内容审计（DEX/JNI/对齐/keep 规则）
```

未实现的子命令（如 `measure`）会显式报错，不会静默通过。构建失败不发布
APK；`run`/`smoke` 拒绝安装比源码旧的 APK（可用 `--allow-stale` 覆盖）。
所有产物与报告位于本仓库 `target/` 下：

| 目录 | 内容 |
| --- | --- |
| `target/android/<profile>/` | 发布的 APK 与 `artifact.json`（sha256、大小、构建时间） |
| `target/android-audit/` | cargo/gradle 构建日志、APK 审计报告 |
| `target/android-review/` | 设备运行报告、instrumentation 原始输出与 JSON、截图 |

依赖默认取自 `../remdia/.deps`（Android SDK、NDK 28.2.13676358、JDK 21、
platform-tools 36.0.2）。SDK 自带的 adb 37.0.1 在本机出现过 libunwind 崩溃，
脚本默认优先使用 36.0.2，`--adb` 可覆盖。

## TLS 与网络

- reqwest 使用 rustls，通过 `rustls-platform-verifier` 在 Android 上接入系统
  信任库。初始化走 0.7.x 的 `android::init_with_env`（jni 0.22 `Env`），在
  `nativeInit` 中以 applicationContext 完成；失败沿 `Result` 上抛，Kotlin 侧
  `ready=false` 并显示初始化门（`init-error-title`/`init-error-detail`），绝不
  降级为跳过证书校验。
- 伴生 Java 库（companion AAR）版本由 `build-android.sh` 通过
  `cargo metadata --locked --filter-platform aarch64-linux-android` 从
  Cargo.lock 的 `rustls-platform-verifier-android` 包解析（当前 0.2.0），AAR
  从上游 `maven-archive` 分支下载、按发布的 sha1 校验后暂存到
  `android/app/libs/maven/`，并生成 `rustls-companion.properties`
  （`coordinate=org.rustls:rustls-platform-verifier:<版本>`）；Gradle 从该文件
  读取坐标，保证与 Cargo.lock 不漂移。R8 keep 规则覆盖
  `org.rustls.platformverifier.**`。
- companion 0.1.1 存在上游缺陷（rustls-platform-verifier issue #221）：证书
  只有 CRL 分发点、无 OCSP URL 时（Let's Encrypt 2025-08 起、Google Trust
  Services 等），第二次 PKIX 校验抛
  `CertPathValidatorException: Certificate does not specify OCSP responder`，
  被无条件映射为 `Revoked`，导致原生 reqwest 全部 HTTPS 失败（`invalid peer
  certificate: Revoked`），而同一证书 Java 平台校验器接受。0.2.0（PR #253）
  在 AAR 内携带 network security config：基础配置仍禁止 cleartext，仅对 CCADB
  生成的 CA CRL 主机（如 `c.pki.goog`、`*.c.lencr.org`）放行 HTTP，使吊销
  检查能经 CRL 完成。诊断证据见 `NetworkDiagnosticsTest`（进程内 PKIX 复现 +
  `replayer-rust` 日志桥输出平台校验器原始消息）。
- reqwest 的环境代理匹配器（`HTTPS_PROXY`/`HTTP_PROXY`/`NO_PROXY`）默认编译
  启用（hyper-util `Matcher::from_env`，不受 feature 门控）；replayer-core 另
  启用 `system-proxy` feature 与上游默认特性集对齐，在 macOS/Windows 桌面端
  额外读取系统代理配置（Android 上无效果）。

## JNI 约定

- 每个导出函数体包裹 `catch_unwind`；恢复的 panic 以 `Native panic` 前缀转为
  Java 异常，测试据此区分实现缺陷与外部失败。
- 数据交换使用 JSON 字符串（`JSONTokener` 解析）；页图/封面使用图像容器协议：
  metadata JSON（含 `tiles` 布局）+ RGBA 负载字节数组。
- Kotlin `external` 声明与 `.so` 导出由 `audit` 逐一对账（当前 24/24）。

## 测试

instrumentation 套件位于 `android/app/src/androidTest/java/dev/replayer/`：

- `NativeBridgeTest`：桥接契约（初始化幂等、库沙箱路径、空白搜索报错、
  书架条目反序列化校验），离线可跑。
- `NavigationUiTest`：Compose 语义导航（testTag 选择器），离线可跑。
- `ComicFlowOnlineTest`：真实 HTTPS 全流程（搜索→详情→页列表→解码页图），
  由 `-e online true` 门控；外部故障（源超时、配额）记为 assumption skip，
  桥接自身缺陷记为 fail。

`smoke` 默认离线；`--online` 打开网络用例。在本宿主机上观察到：宿主解析
`komiic.com` 得到 198.18.x.x（疑似 Clash Verge TUN/fake-IP），模拟器外部网络
丢包、DNS 挂起；`adb shell`（shell uid）可连通 `10.0.2.2:7890`，但 `run-as`
域探测超时——**run-as 不等价于 ART 应用进程网络，不能据此下结论**，应用进程
内的真实分层证据由 `NetworkDiagnosticsTest` 给出（env→DNS→TCP→CONNECT→
TLS/HTTP→native reqwest 错误链）。

`--proxy http://10.0.2.2:<端口>` 会让 `ProxyTestRunner` 在测试进程启动时导出
`HTTPS_PROXY` 等环境变量（依赖 replayer-core 的 reqwest `system-proxy`
feature 与 hyper-util 环境代理匹配器）；`ProxyTestRunner` 在不传参时是空操作。
另一个候选是启动模拟器时使用官方 `emulator -http-proxy http://127.0.0.1:<端口>`
（宿主 emulator 进程级代理，对 guest 透明，原生 TCP 也生效）；两者不要同时
使用，避免双重代理。哪条路径真正可达以 `NetworkDiagnosticsTest` 的分层报告
为准，尚在验证中。

MainActivity 通过 `testTagsAsResourceId`（`androidx.compose.ui.semantics`，
需 `@OptIn(ExperimentalComposeUiApi::class)`）把 testTag 原样发布为
resource-id（无 `package:id/` 前缀），供 UI Automator/adb 驱动的检查和
`android-dev.py` 的初始化门读取使用。

## 设备与 AVD

- `emulator-5554` 为 `replayer` AVD（API 35，`-no-window -no-snapshot`，位于
  `~/.android/avd`）。
- API 28 与 16 KB 页 AVD 复用 remdia 的产物：
  `ANDROID_AVD_HOME=../remdia/target/android/avd`，AVD 名 `remdia-api28`、
  `remdia-16k`、`remdia-api35`。
- 原生库按 16 KB LOAD 对齐链接（`-Wl,-z,max-page-size=16384`）；APK 内未压缩
  `.so` 的 ZIP 对齐以 `zipalign -c -P 16 -v 4` 为准（audit 中的本地头校验读取
  local file header，中央目录的 extra 字段不反映 zipalign 的填充）。

## 已知环境怪癖

- 宿主 Clash Verge TUN 运行中（系统代理关闭，宿主 `komiic.com` 解析为
  198.18.x.x）。**已观察**：emulator-5554 到 8.8.8.8 约 50% 丢包、DNS 挂起、
  出现时钟回退告警；模拟器存在双网卡 eth0(10.0.2.15)/wlan0(10.0.2.16)，
  `ip rule` 有按 uid 的分表路由（app uid 走 wlan0/netsimd 侧，shell 走
  eth0/slirp 侧），`adb reverse` 隧道实测不转发。
- **已证实**（`NetworkDiagnosticsTest` 于应用进程内的分层报告，
  smoke-20261030T082553Z）：app 进程 DNS 解析 fake-IP（10ms）、TCP 到
  `10.0.2.2:7890` 连通（1ms）、CONNECT 隧道 200、Java 平台校验器直连
  `https://komiic.com/` 得 HTTP 200。此前原生 HTTPS 失败与网络拓扑无关，
  根因是 companion 0.1.1 的吊销检查缺陷（见"TLS 与网络"），0.2.0 已修复。
- 长跑模拟器可能出现时钟回退与丢包；时钟偏差会影响 TLS 证书有效期判断，
  遇到证书错误先核对 `adb shell date`。
- 修改 `android/app/build.gradle.kts` 会使已发布 APK 判定为过期，需重新
  `build`；androidTest 源码改动不影响 app APK 的新鲜度（smoke 每次重建测试
  APK）。

## 验证状态

P0 验收于 2026-10-30 在 emulator-5554（API 35）全部通过，APK
`bf0663a80e363d06986f0328b37caccdb2ec1e1ebdb66972e24c1bdf86d03435`
（debug，281.7 MB，companion 0.2.0 + Context 错误链 + log 桥）：

- `audit` 14/14 PASS（apk-audit-debug-20261030T082804Z）：DEX 含
  `dev/remdia/Bridge`、`dev/replayer/Native`、
  `org/rustls/platformverifier/CertificateVerifier`；`libremdia_sdk.so` 已
  排除；ZIP 与 ELF 均 16 KB 对齐；JNI 导出 24/24 覆盖 Kotlin 声明。
- 在线 smoke 10/10 passed（smoke-20261030T082746Z，`--online --proxy
  http://10.0.2.2:7890`）：`ComicFlowOnlineTest.searchDetailPagesAndImageOverHttps`
  完成真实 HTTPS 搜索→详情→页列表→页图；`NetworkDiagnosticsTest` 全层通过
  （native reqwest 搜索 books=8）。
- 离线 smoke 8 passed / 2 gated skip（smoke-20261030T082922Z，无
  `--online`/`--proxy`，`ProxyTestRunner` 空操作）。
- `run`：启动、MainActivity resumed、native bridge ready、无崩溃
  （run-debug-20261030T083054Z）。
- 桌面回归：`cargo fmt --all -- --check` 通过；`cargo test --locked` exit=0
  （122 + 26 passed，0 failed；日志 `target/android-audit/desktop-test-*.log`）。

尚未验证（P1 范围）：API 28 与 16 KB 页 AVD 实测、release/R8 体积与构建
时长、播放 EOF/封面协议/长图内存。历史报告见 `target/android-review/`、
`target/android-audit/` 中的 JSON。
