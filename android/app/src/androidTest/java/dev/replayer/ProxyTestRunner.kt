package dev.replayer

import android.os.Bundle
import android.system.Os
import androidx.test.runner.AndroidJUnitRunner

/**
 * Runner that can route the app's native `reqwest` traffic through an HTTP proxy.
 *
 * The Rust core opens raw TLS sockets, which ignore Android's *system* proxy
 * setting; `reqwest` instead consults the `HTTPS_PROXY`/`HTTP_PROXY`/`NO_PROXY`
 * environment variables (hyper-util's env matcher, compiled in unconditionally;
 * reqwest's `system-proxy` feature additionally reads macOS/Windows system
 * proxy config, which is inert on Android). On hosts where the comic source
 * is only reachable through a local proxy, passing `-e proxy
 * http://10.0.2.2:<port>` exports those variables so the online smoke can
 * reach the real site through the host tunnel and exercise the platform
 * certificate verifier against the genuine certificate. Whether app-process
 * traffic can actually reach that endpoint is what `NetworkDiagnosticsTest`
 * verifies layer by layer.
 *
 * The variables are exported in [onCreate], before any test runs, so the
 * process-wide comic source (a `OnceLock`-cached `reqwest::Client`) is built with
 * the proxy already in effect. This is a no-op unless `-e proxy` is supplied, so
 * committed tests never depend on a specific host network.
 */
class ProxyTestRunner : AndroidJUnitRunner() {
    override fun onCreate(arguments: Bundle) {
        arguments.getString("proxy")?.takeIf { it.isNotBlank() }?.let { proxy ->
            for (name in listOf("HTTPS_PROXY", "https_proxy", "HTTP_PROXY", "http_proxy")) {
                runCatching { Os.setenv(name, proxy, true) }
            }
            // Diagnostics: confirm the env actually changed in this process, so a
            // silent setenv failure cannot masquerade as a network outage.
            android.util.Log.i(
                "replayer-proxy",
                "arg=$proxy env HTTPS_PROXY=${Os.getenv("HTTPS_PROXY")} https_proxy=${Os.getenv("https_proxy")}",
            )
        }
        super.onCreate(arguments)
    }
}
