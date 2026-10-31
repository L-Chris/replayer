# Android 客户端

Android 端复用桌面版的 `pordo-core`：Rust 核心逻辑不变，通过
`crates/pordo-android` 的 JNI 桥接暴露给 Kotlin/Compose 应用。remdia 引擎
静态链接进唯一的业务库 `libpordo_android.so`；AAR 里的独立
`libremdia_sdk.so` 被 Gradle packaging 排除（`android-dev.py audit` 持续校验）。
漫画通过 pordo-service 发现和搜索启用的内容源；桌面和 Android 使用同一个 Rust API 适配器。
同时支持音乐播放、在线歌曲收藏和与桌面兼容的云同步。

## 内容服务连接

Android：漫画页 → 源设置；桌面：设置 → 漫画 → 内容服务。
填写 pordo-service 的根地址与设备 Token，点击“保存并测试连接”。
源账号、密码、登录与代理由服务管理页面维护，客户端不再包含 Komiic/MangaYun/DM5 的抓取实现。

- Android 只将服务地址保存到 preferences；设备 Token 通过 SecretStore 使用系统 Keystore 加密，按服务地址隔离。
- Token 不回填到输入框，留空可沿用同一地址的已存 Token；更换地址必须提供新 Token。“断开并清除 Token”清除当前连接。
- 设置保存与凭据写入失败会回滚；临时服务故障不会丢弃已保存的连接。
- 旧源密码不自动上传、不参与同步；在服务页面重新配置，旧本机凭据保留以便回退版本。
- 旧收藏按原 provider/site/detailUrl 调用 resolve；章节保留原 ID/URL，已读、阅读进度和 WebDAV 协议不变。
- 服务不可达或 Token 撤销时显示明确错误，不退回源站直连。服务配置异常不阻断音频/媒体 JNI 初始化。

## 正式签名与发布

正式下载包为 `pordo-VERSION-android-arm64-v8a.apk`，支持 Android 9+
arm64 设备。首版提供音乐、漫画和 WebDAV 云同步，尚无桌面端的视频、AI
字幕和漫画翻译界面。签名证书的 SHA-256 固定在
`android/release-certificate.sha256`，后续 APK 使用同一密钥覆盖更新。

此前开发/测量包使用测试签名，不能直接升级为正式签名。已安装测试包的用户
请先同步或备份数据，再卸载测试版、安装正式版；同一正式签名的后续版本
可以直接更新。

- 本机密钥和配置位于被 Git 忽略的 `.deps/android-signing/`，目录权限 0700，
  JKS 和 `keystore.properties` 权限 0600；含密钥及口令的恢复副本保存在
  macOS Keychain（`dev.replayer.credentials` 服务下的
  `replayer/android/signing/v1` 项目），不参与 iCloud 同步。
- CI 使用 `ANDROID_KEYSTORE_BASE64`、`ANDROID_KEYSTORE_PASSWORD`、
  `ANDROID_KEY_ALIAS`、`ANDROID_KEY_PASSWORD` 四个 GitHub Secrets，
  临时 JKS 仅用于签名，结束即删除，不写日志、缓存或发布附件。
- Gradle `release` 不再默认使用 debug 密钥。环境变量
  `ANDROID_KEYSTORE_FILE` 及上述三个口令/别名变量优先于本机配置；没有
  正式密钥时拒绝 release 构建。仅本地测量可显式设置
  `PORDO_ANDROID_TEST_SIGNING=1`，该签名无法通过正式打包证书校验。
- `scripts/package-android.py` 检查包名、版本、最低系统、ABI 和非调试属性，
  先 zipalign 再签名，分别验证 v2/v3 签名及固定证书，确认所有应用负载未变。
  APK、校验和与公开签名报告是唯一输出，私钥及口令不进入输出目录。
- 统一发布 workflow 同时验证 Windows、macOS 和 Android，全部通过后才
  创建 Release。`workflow_dispatch` 的 `release_tag` 可补发已有标签：
  应用仍从不可变标签构建，打包工具和安装测试来自 workflow 提交。
  v0.5.1 不移动标签；仅安装测试脚本补建旧组件的模拟目录，未修改应用代码。

本机构建与打包：

```sh
python3 scripts/android-dev.py build --profile release
JAVA_HOME=/path/to/jdk ANDROID_HOME=/path/to/sdk python3 scripts/package-android.py \
  --apk target/android/release/pordo-release.apk --tag v0.5.1
```

签名私钥和口令仅有本机/密钥库/CI 副本，请保留这些恢复途径，避免后续无法
为已安装应用签发兼容更新。证书指纹属于公开信息，私钥不能从指纹恢复。

正式签名验证（2026-10-30）：APK **11,492,740 bytes / 10.96 MiB**，
SHA-256 `3ad50bbec1844fe42c4b248d1ac0e6c17027a57ab52b912ea17a374fb1ec3cbe`。
打包、v2/v3 独立签名验证、证书固定、应用负载一致性和 13 项 APK 审计全部
通过；8 项开发/打包工具测试及 actionlint 通过。在独立的 Android 9/API 28
模拟器上先安装正式签名构建，再用最终发布 APK 覆盖，UID 和首次安装身份
不变，两次均原生初始化成功、无崩溃。原有预览模拟器和数据未改动。

## 云同步

入口：音乐或漫画页右上角“云同步”。选择与桌面相同的 WebDAV 提供商，填写
地址、用户名、应用密码并保存；可以先“测试连接”，再“立即同步”。设备 ID
由本机生成并持久保存，不从远端导入。同步 API Key 时，两端填写相同的独立
同步口令（至少 12 字符）。连接测试只创建、读取、删除随机探测对象，不触碰
已有设备文档。

- 协议完全复用 `pordo-core::sync`：`replayer/v1/devices/<UUID>.json`、
  向量时钟、删除标记、ETag 条件写入、AES-256-GCM/PBKDF2 的 v1 密钥封装。
  桌面 `src/sync/mod.rs` 只重新导出共享模块，漫画快照/导入也使用同一份代码。
- 与桌面一致，同步漫画收藏（含曾收藏后取消的已跟踪记录）、章节列表、已读/
  更新标记、阅读进度；同步在线歌曲收藏、播放进度、音源和音质选项。
  未收藏且从未跟踪的本地历史、音视频文件、下载路径和缓存不上传。
- Android 添加歌曲收藏入口和收藏列表；跨设备收藏先显示元数据，下载后播放。
  阅读进度中的 fraction 与桌面一样表示当前页内的位置；滚到最后一张已解码
  页的底部才标记章节读完，打开章节本身不会清除未读更新。
- 本地漫画修改通过字段补丁写入最新 SQLite 记录，不用旧页面对象覆盖云端
  刚合并的收藏/已读状态。同步请求返回后重新捕获期间发生的本地编辑，再合并；
  需要继续上传时显示“等待同步”，未完成的操作写入按账户隔离的本地日志。
- 启动、回到前台、手动触发时同步；前台每 5 分钟检查，普通变更合并 5 秒，
  进度变更合并 30 秒；退出前台时尝试提交待处理变化。错误自动退避 5 分钟，
  手动同步可立即重试。进程被系统终止后从下次启动继续，云同步没有单独的常驻后台服务。
- WebDAV 密码、独立口令和 API Key 使用 Android KeyStore 保护的 AES-GCM
  凭据存储；旧音乐明文偏好在首次启动时迁移并移除。草稿口令不写 SavedState，
  `allowBackup` 关闭，避免系统备份复制设备身份和本机凭据。
- 手机暂未实现的 AI/字幕选项保留在合并日志中，不产生删除操作。开启 API Key
  同步后，未实现提供商的密钥也保留在本机加密分区，防止下一轮上传空值覆盖。
  本机登录密码和同步口令从不写入远端文档。
- 同步完成后，书架/详情和歌曲收藏列表通过 revision 自动刷新；当前正在播放
  或阅读的位置不会被远端进度强制跳转，下次打开时恢复合并后的进度。

验证：共享 Rust 测试 **111 + 39 = 150 passed**，含桌面→手机→桌面模拟
WebDAV 双向往返、离线取消、并发编辑、未实现字段及密钥保护。Android API 35
在线 smoke **20/20 passed，无 skip**（`smoke-20261030T110406Z`），含真实
漫画流程、云设置导航、KeyStore 随机 nonce、DAV 连接探测、JNI 完整往返与
应用协调器上传收藏/加密 API Key。API 28 离线 smoke **18 passed / 2 项在线用例未启用**（`smoke-20261030T110252Z`）。
未使用个人 WebDAV 凭据测试真实网盘服务。

## 工具链

统一入口是 `scripts/android-dev.py`（Python 3.9 兼容），它复用
`scripts/build-android.sh` 完成 NDK 交叉编译与产物发布，不允许出现第二条不
一致的构建路径：

```sh
./scripts/android-dev.py doctor    # 工具链、暂存依赖、设备与 AVD 体检
./scripts/android-dev.py check     # 锁定的 aarch64-linux-android cargo check（--clippy 可选）
./scripts/android-dev.py build     # cargo + gradle，发布 target/android/<profile>/pordo-<profile>.apk
./scripts/android-dev.py run --serial emulator-5554     # 安装、启动、验证 native 就绪与初始化门
./scripts/android-dev.py smoke --serial emulator-5554   # 离线 instrumentation 套件
./scripts/android-dev.py audit     # APK 内容审计（DEX/JNI/对齐/keep 规则）
```

`measure` 可记录构建、包体和内存基线；不支持的命令会显式报错。构建失败不发布
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
  检查能经 CRL 完成。问题定位时使用过进程内 PKIX 复现与 `replayer-rust` 日志桥。
  修复后删除了反射 companion 私有字段及复刻 PKIX 的临时测试，保留通用
  `NetworkDiagnosticsTest` 分层诊断和真实漫画流程。
- reqwest 的环境代理匹配器（`HTTPS_PROXY`/`HTTP_PROXY`/`NO_PROXY`）默认编译
  启用，不依赖 `system-proxy` feature。core 不为 Android 排障额外启用该
  feature；它只影响 macOS/Windows 的系统代理配置读取。

## JNI 约定

- 每个导出函数体包裹 `catch_unwind`；恢复的 panic 以 `Native panic` 前缀转为
  Java 异常，测试据此区分实现缺陷与外部失败。
- 数据交换使用 JSON 字符串（`JSONTokener` 解析）；页图/封面使用图像容器协议：
  metadata JSON（含 `tiles` 布局）+ RGBA 负载字节数组。
- Kotlin `external` 声明与 `.so` 导出由 `audit` 逐一对账（当前 32/32）。

## 测试

instrumentation 套件位于 `android/app/src/androidTest/java/dev/pordo/`：

- `ContentServiceTest`：Token 凭据隔离、重启恢复、配置验证，以及显式本地服务联调。
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
`HTTPS_PROXY` 等环境变量（使用 hyper-util 默认的环境代理匹配器）；`ProxyTestRunner` 在不传参时是空操作。
另一个候选是启动模拟器时使用官方 `emulator -http-proxy http://127.0.0.1:<端口>`
（宿主 emulator 进程级代理，对 guest 透明，原生 TCP 也生效）；两者不要同时
使用，避免双重代理。应用进程已通过下述 P0 在线验收；历史 shell/run-as
探测结果不能用作应用进程的结论。

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

## 历史验证状态（品牌更名前）

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

P1 于同日完成：播放修复（AudioTrack 帧单位、封面 tiles 协议、长图内存
range view）在 API 35 离线 smoke 验证（11 passed / 2 gated skip，
smoke-20261030T085747Z），并在 API 28 与 16 KB 页 AVD 复验通过。

release/R8 基线（`measure` 子命令，报告 measure-release-20261030T100057Z，
APK `8ad3065cc1bcd34fb4df662aeefe890d8bd916a24a0ad66c274c45a74336c5f1`）：

- `build.gradle.kts` release 开启 `isMinifyEnabled` + `isShrinkResources`；
  当时尚无正式 keystore，测量期 release 使用 debug 签名（仅可安装于开发设备）。
- APK 9.0 MB（debug 167.9 MB）：dex 2.1 MB / 2942 类（debug 52.7 MB /
  24180 类），native .so 6.7 MB（debug 114.7 MB），资源收缩后 97.9 KB。
  构建时长：首次 release 80.4s，`measure --cold`（清 Gradle 构建目录 +
  `cargo clean -p replayer-android`）33.2s，热缓存 4.6s。
- 常驻内存（启动静置，dumpsys meminfo 3 采样）：API 35/4K PSS 29–34 MB
  （native ~12.4 MB），API 35/16K PSS 38–44 MB。
- `audit --profile release` 14/14 PASS（apk-audit-release-20261030T100311Z，
  对应上述 8ad3065c APK）：
  R8 后 `dev/remdia/Bridge`、`dev/replayer/Native`、
  `org/rustls/platformverifier/CertificateVerifier` 均保留，JNI 导出 24/24。
- 资源收缩器把 companion 的 `res/xml/network_security_config.xml` 重命名为
  `res/8G.xml`，但清单引用（`@0x7f0b0000`）与内容（cleartext 基础配置 +
  CRL 主机放行表）完整，已用 aapt2 核实——release 的 CRL 抓取路径不受影响。
- release `run`：API 35（run-release-20261030T091907Z）、API 28
  （run-release-20261030T094642Z）、16 KB 页（run-release-20261030T095437Z）
  均启动、MainActivity resumed、无 init gate、无崩溃。

API 28 实测发现平台缺陷并已规避（remdia-api28 AVD）：

- **已证实**：`Bitmap.copyPixelsFromBuffer` 收到带非零 base offset 的堆
  ByteBuffer 时，AOSP `nio_releasePointer`（android-9.0.0_r35
  `core/jni/android_nio_utils.cpp`）以 `elements + offset` 释放
  `GetPrimitiveArrayCritical` 指针，CheckJNI 中止进程（SIGABRT，
  `invalid element pointer 0x12d0554a, array elements are 0x12d05504`，
  差值 0x46 恰为容器 payload 偏移）。API 35 无此路径，同一代码通过。
- 修复：`Container.bitmap()` 改用 `directSlice()`（direct ByteBuffer，
  走 `getBasePointer` 路径），代价是每 tile 一次有界拷贝（tile 高度受
  core 限制）。修复后 api28 离线 smoke 11 passed / 2 gated skip
  （smoke-20261030T094625Z）；16 KB 页 AVD（sdk_gphone16k_arm64，
  page_size 16384）同样 11/2（smoke-20261030T095421Z）。
- `android-dev.py` 同步修复的 API 28 兼容问题：`dumpsys activity` 的
  `mResumedActivity:` 冒号格式、无 `getconf` 时经 `/proc/self/smaps`
  读取页大小、crash 判定过滤 logcat `--------- beginning of` 横幅、
  新鲜度判定排除 `jniLibs`（release 暂存 .so 不再把 debug APK 标脏）。

另观察：`cargo clean -p replayer-android` 后重建，debug .so 从约 275 MB
降至 120 MB（增量链接的调试段膨胀），debug APK 相应从 281.7 MB 降至
167.9 MB；体积异常时可先清理该 crate 再判断。

尚未验证：release 构建上的在线 HTTPS 全流程（仪器化测试仅挂 debug
变体；release 的 companion 类与 network security config 已由 audit/aapt2
静态证实，`run` 证实 nativeInit 成功）。历史报告见 `target/android-review/`、
`target/android-audit/` 中的 JSON。

## 代码审核后的精简

- 用七个独立 Material Design 矢量资源替代 `material-icons-extended`，仅保留
  核心图标库；移除未使用的 viewmodel-compose 和 UI Automator 测试依赖。
- 删除证书问题定位期间的私有字段反射和 PKIX 复刻；默认平台验证及真实
  reqwest 请求仍覆盖 TLS，不增加证书绕过分支。
- JNI 图片分片直接写入 Java 数组，不再构造两份完整 native 负载；删除没有
  Kotlin 调用者的图片 path/hash 元数据。Kotlin 仍将长图拼成整幅 Bitmap，
  因此这次只降低传输临时内存，不能宣称已经实现视口分片渲染。
- 阅读器只使用与源策略一致的受限 IO dispatcher，删除重复且无法扩容的
  Semaphore；取消加载后不再回填缓存。修正页图宽高比。
- 开发脚本的进程超时覆盖输出读取，显式 NDK 参数传递给统一构建脚本。
  `audit` 使用 SDK zipalign 校验 ZIP 对齐，不再维护重复的 ZIP 本地头解析；
  DEX 类索引和包体分项统计由 audit/measure 共用。发布产物安装前校验哈希。
- 宿主工具回归：`python3 -m unittest discover -s scripts/tests -p test_android_dev.py -v`。
  `smoke` 只支持 debug instrumentation，release 使用 run/audit。

`measure --cold` 只清除桥接 crate 和 Gradle 输出，保留依赖编译缓存；历史
33.2s 是局部重建数据，不代表首次构建或完整 CI 冷构建。

审核后验证（2026-10-30）：

- Android target cargo check 无警告，debug/release 完整构建通过。
- API 35/4 KB 在线 smoke **14/14 passed，无 skip**（包括真实漫画全流程、
  图片像素、播放 EOF 和新增 PageFetcher 并发回归）；原始报告
  `target/android-review/smoke-20261030T103055Z.json`。
- 宿主工具回归 4/4 passed；桌面 122 + 26 passed，26 ignored；fmt/diff 检查通过。
- audit 使用 SDK ZIP 对齐校验，删除重复的手工校验后为 **13/13**，不与历史
  14 项统计混用。release API 35 安装启动通过；本轮未重跑 API 28/16 KB 设备。
- debug DEX 从约 52.7 MiB / 24180 类减至 **22.3 MiB / 13074 类**。
  本轮观察到增量 APK 为 282.7 MiB，但有效条目压缩大小合计仅约 137.6 MiB；
  仅删除 Gradle 的 debug APK 输出后重新打包即降为 **137.7 MiB**，相对上一轮
  167.9 MiB 基线减少约 30 MiB。逐条目 SHA-256 比较确认重新打包前后应用负载
  完全相同（排除签名元数据），因此这次多余空间来自增量 ZIP，不是 native
  代码增长。比较包体应使用干净打包产物，不必清空 Cargo 依赖缓存。
- release APK **9,499,205 bytes / 9.06 MiB**（上一轮 9,455,546 bytes / 9.02 MiB）；
  R8 本来就会删除未使用图标，裁剪扩展库的收益主要体现在 debug 和构建输入。
  少量增量包含独立矢量资源及其加载支持、许可证和功能修正。
  最终 release SHA-256：
  `30d3a6da66b1193a5b6dc5a579c2813c5b78fb6ab6233cb6a9be0f1a99c7c5ec`。
- 本轮未测长图峰值内存；删除 native 全量拷贝不等于实测出等量 PSS 降幅。

云同步版本发布构建验证：release APK 11,291,833 bytes / 10.77 MiB，
SHA-256 `224e293b47889c09c987af01dcd8fe489bd33507659ec864e0390cf431e5de84`。release audit 13/13（JNI 28/28），API 35 安装
启动无初始化门、无崩溃；API 28 的 KeyStore/协议/应用协调器使用 debug
instrumentation 验证。个人网盘与 release 变体的完整同步尚未实测；本轮未重跑
16 KB 设备，已通过 ELF/ZIP 16 KB 对齐审计。临时 API 28 AVD 已关闭，保留
原有 emulator-5554 运行最终 release。
