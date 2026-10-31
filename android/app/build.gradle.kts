import java.util.Properties

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
}

// The JVM companion of rustls-platform-verifier is staged from Cargo.lock by
// scripts/build-android.sh, so the coordinate cannot drift from the Rust lock
// and a missing stage fails with an actionable message instead of a confusing
// dependency-resolution error.
val companionProperties = file("libs/rustls-companion.properties")
check(companionProperties.exists()) {
    "missing ${companionProperties.path}: run scripts/build-android.sh before Gradle"
}
val companionCoordinate =
    Properties()
        .apply { companionProperties.inputStream().use { load(it) } }
        .getProperty("coordinate")
        ?: error("rustls-companion.properties has no coordinate")

// Local credentials stay in an ignored, permission-restricted directory. CI
// supplies the same fields through encrypted repository secrets.
val releaseProperties = Properties().apply {
    val path = rootProject.file("../.deps/android-signing/keystore.properties")
    if (path.isFile) path.inputStream().use { load(it) }
}
fun signingValue(environment: String, property: String): String? =
    System.getenv(environment)?.takeIf { it.isNotBlank() }
        ?: releaseProperties.getProperty(property)?.takeIf { it.isNotBlank() }
val releaseStore = signingValue("ANDROID_KEYSTORE_FILE", "storeFile")
val releaseStorePassword = signingValue("ANDROID_KEYSTORE_PASSWORD", "storePassword")
val releaseKeyAlias = signingValue("ANDROID_KEY_ALIAS", "keyAlias")
val releaseKeyPassword = signingValue("ANDROID_KEY_PASSWORD", "keyPassword")
val haveReleaseKey = listOf(releaseStore, releaseStorePassword, releaseKeyAlias, releaseKeyPassword).all { it != null }
val testSigning = (System.getenv("PORDO_ANDROID_TEST_SIGNING")
    ?: System.getenv("REPLAYER_ANDROID_TEST_SIGNING")) == "1"
if (gradle.startParameter.taskNames.any { it.contains("release", ignoreCase = true) }) {
    check(haveReleaseKey || testSigning) {
        "Release signing is required; see docs/android.md. For local measurement only, set PORDO_ANDROID_TEST_SIGNING=1."
    }
}

android {
    namespace = "dev.pordo"
    compileSdk = 35

    defaultConfig {
        // Published identity: retain installed data, permissions and signing continuity.
        applicationId = "dev.replayer"
        minSdk = 28
        targetSdk = 35
        versionCode = 2
        versionName = "0.5.1"
        ndk { abiFilters += listOf("arm64-v8a") }
        testInstrumentationRunner = "dev.pordo.ProxyTestRunner"
    }

    signingConfigs {
        if (haveReleaseKey) create("production") {
            storeFile = file(releaseStore!!)
            storePassword = releaseStorePassword
            keyAlias = releaseKeyAlias
            keyPassword = releaseKeyPassword
        }
    }

    buildTypes {
        release {
            // R8 shrinking is part of the measured release baseline (size and
            // build time via `scripts/android-dev.py measure`). The audit's DEX
            // keep checks (dev.remdia, dev.pordo.Native, the rustls
            // companion) must pass on the minified APK.
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
            signingConfig = when {
                haveReleaseKey -> signingConfigs.getByName("production")
                testSigning -> signingConfigs.getByName("debug")
                else -> null
            }
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions { jvmTarget = "17" }
    buildFeatures { compose = true }

    testOptions {
        // Instrumented smoke tests assert on state, not on animation timing.
        animationsDisabled = true
    }

    packaging {
        jniLibs {
            // remdia's engine is statically embedded in libpordo_android.so, so
            // the AAR's standalone SDK library (jni/<abi>/libremdia_sdk.so, ~1.9 MB
            // per ABI) must never be packaged. A file-dependency AAR *does*
            // contribute its jni/ folder, so this exclusion is what actually keeps
            // the redundant libremdia_sdk.so out of the APK (verified by
            // `android-dev.py audit`). abiFilters already limits us to arm64-v8a.
            excludes += "**/libremdia_sdk.so"
            // AGP 8.5+ keeps uncompressed libs 16 KB page aligned.
            useLegacyPackaging = false
        }
    }
}

dependencies {
    implementation(files("libs/remdia-0.3.0.aar"))
    implementation(companionCoordinate)
    implementation(platform("androidx.compose:compose-bom:2024.12.01"))
    implementation("androidx.core:core-ktx:1.15.0")
    implementation("androidx.activity:activity-compose:1.9.3")
    implementation("androidx.lifecycle:lifecycle-runtime-compose:2.8.7")
    implementation("androidx.navigation:navigation-compose:2.8.5")
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.foundation:foundation")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.material:material-icons-core")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.9.0")

    androidTestImplementation(platform("androidx.compose:compose-bom:2024.12.01"))
    androidTestImplementation("androidx.compose.ui:ui-test-junit4")
    androidTestImplementation("androidx.test.ext:junit:1.2.1")
    androidTestImplementation("androidx.test:runner:1.6.2")
    debugImplementation("androidx.compose.ui:ui-test-manifest")
}
