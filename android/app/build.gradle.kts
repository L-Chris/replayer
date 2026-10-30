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

android {
    namespace = "dev.replayer"
    compileSdk = 35

    defaultConfig {
        applicationId = "dev.replayer"
        minSdk = 28
        targetSdk = 35
        versionCode = 1
        versionName = "0.5.0"
        ndk { abiFilters += listOf("arm64-v8a") }
        testInstrumentationRunner = "dev.replayer.ProxyTestRunner"
    }

    buildTypes {
        release {
            // R8 shrinking is part of the measured release baseline (size and
            // build time via `scripts/android-dev.py measure`). The audit's DEX
            // keep checks (dev.remdia, dev.replayer.Native, the rustls
            // companion) must pass on the minified APK.
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
            // Measurement builds only: no release keystore exists yet, and an
            // unsigned APK cannot be installed for the memory/boot checks.
            signingConfig = signingConfigs.getByName("debug")
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
            // remdia's engine is statically embedded in libreplayer_android.so, so
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
    implementation("androidx.lifecycle:lifecycle-viewmodel-compose:2.8.7")
    implementation("androidx.navigation:navigation-compose:2.8.5")
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.foundation:foundation")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.material:material-icons-extended")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.9.0")

    androidTestImplementation(platform("androidx.compose:compose-bom:2024.12.01"))
    androidTestImplementation("androidx.compose.ui:ui-test-junit4")
    androidTestImplementation("androidx.test.ext:junit:1.2.1")
    androidTestImplementation("androidx.test:runner:1.6.2")
    androidTestImplementation("androidx.test.uiautomator:uiautomator:2.3.0")
    debugImplementation("androidx.compose.ui:ui-test-manifest")
}
