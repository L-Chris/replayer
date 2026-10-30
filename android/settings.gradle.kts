pluginManagement {
    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
}
dependencyResolutionManagement {
    repositories {
        google()
        mavenCentral()
        // JVM companion of rustls-platform-verifier (org.rustls.platformverifier),
        // staged from Cargo.lock by scripts/build-android.sh. Without it the Rust
        // TLS verifier cannot reach the Android system trust store.
        maven {
            name = "rustlsCompanion"
            url = uri("${rootDir}/app/libs/maven")
        }
    }
}
rootProject.name = "replayer"
include(":app")
