package dev.replayer

import android.os.SystemClock
import android.system.Os
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import dev.replayer.comic.ComicRepository
import java.io.BufferedReader
import java.io.InputStreamReader
import java.net.InetAddress
import java.net.InetSocketAddress
import java.net.Socket
import java.net.URL
import javax.net.ssl.HttpsURLConnection
import org.junit.Assert.assertTrue
import org.junit.Assume.assumeNoException
import org.junit.Assume.assumeTrue
import org.junit.Test
import org.junit.runner.RunWith

/**
 * Layer-by-layer network diagnostics inside the real app process.
 *
 * `adb shell`/`run-as` probes are NOT equivalent to the ART app process:
 * SELinux domain, socket marking and per-uid routing can all differ, so every
 * conclusion about app networking must come from instrumentation running in
 * the target process itself. Each step records its layer verdict with timing:
 *
 * 1. env          — proxy variables visible to native reqwest
 * 2. dns          — InetAddress resolution of the comic host
 * 3. tcp          — Java Socket connect to the proxy endpoint (when given)
 * 4. connect      — HTTP CONNECT tunnel handshake, first response line
 * 5. tls+http     — HttpsURLConnection with the DEFAULT system trust store
 * 6. native       — the same request through JNI → reqwest, whose exception
 *                   now carries the full error chain (`{error:#}` at the JNI
 *                   boundary), distinguishing timeout/connect/TLS/proxy causes
 *
 * Run only this class: `smoke --online --test dev.replayer.NetworkDiagnosticsTest`.
 * A layer failure caused by the environment skips with the full report kept as
 * evidence; an implementation failure (native panic, TLS verifier, bridge init)
 * fails the test.
 */
@RunWith(AndroidJUnit4::class)
class NetworkDiagnosticsTest {
    private val report = StringBuilder()

    @Test
    fun diagnoseNetworkLayers() {
        assumeTrue("diagnostics need --online", onlineRequested())
        val proxy =
            InstrumentationRegistry.getArguments().getString("proxy")?.takeIf { it.isNotBlank() }

        record("env HTTPS_PROXY", Os.getenv("HTTPS_PROXY") ?: "(unset)")
        record("env HTTP_PROXY", Os.getenv("HTTP_PROXY") ?: "(unset)")
        record("proxy argument", proxy ?: "(none)")

        // Layer 2: DNS, inside the app process.
        time("dns komiic.com") {
            InetAddress.getAllByName("komiic.com").joinToString { it.hostAddress ?: "?" }
        }

        // Layers 3-4: TCP + CONNECT tunnel to the proxy, when one was given.
        if (proxy != null) {
            val url = URL(proxy)
            val host = url.host
            val port = if (url.port > 0) url.port else 80
            val tcp = time("tcp $host:$port") {
                Socket().use { socket ->
                    socket.connect(InetSocketAddress(host, port), CONNECT_TIMEOUT_MS)
                    "connected to ${socket.remoteSocketAddress}"
                }
            }
            if (!tcp.startsWith(FAILED)) {
                time("connect komiic.com:443 via $host:$port") { connectFirstLine(host, port) }
            }
        }

        // Layer 5: TLS + HTTP through the Java stack with the system trust
        // store. Any HTTP status (even 4xx) proves DNS+TCP+TLS+HTTP work.
        time("tls+http https://komiic.com/ (Java verifier)") {
            val connection = URL("https://komiic.com/").openConnection() as HttpsURLConnection
            try {
                connection.connectTimeout = CONNECT_TIMEOUT_MS
                connection.readTimeout = READ_TIMEOUT_MS
                connection.instanceFollowRedirects = false
                "HTTP ${connection.responseCode}"
            } finally {
                connection.disconnect()
            }
        }

        // Layer 5b: reproduce the companion verifier's second PKIX validation
        // (revocation checker with SOFT_FAIL + ONLY_END_ENTITY). Any
        // CertPathValidatorException there is what surfaces to Rust as
        // `invalid peer certificate: Revoked` — this step recovers the real
        // message/reason the companion drops, plus anchor-store sizes.
        record("companion anchors", companionAnchors())
        time("pkix validate komiic chain (companion-style)") { pkixValidateChain() }

        // Layer 6: the production path — JNI bridge → reqwest → komiic. The
        // exception message carries the core context plus reqwest's source
        // chain, which is the evidence for timeout vs connect vs TLS vs proxy.
        val ready = Native.initialize(testContext)
        record("native initialize", ready.toString())
        val search = time("native reqwest comic search") {
            val groups = ComicRepository.search("one piece")
            val books = groups.sumOf { it.books.size }
            val errors = groups.mapNotNull { it.error }
            if (books == 0 && errors.isNotEmpty()) throw IllegalStateException(errors.joinToString())
            "books=$books"
        }

        val text = report.toString()
        android.util.Log.i(TAG, "\n$text")
        println(text)

        // Native layer verdicts that indicate OUR defects must fail, not skip.
        if (search.startsWith(FAILED)) {
            val detail = search.removePrefix(FAILED).trim()
            assertNotImplementationFailure(detail)
        }
        assertTrue("bridge must initialize for diagnostics", ready)

        val broken = report.lineSequence().firstOrNull { it.contains(FAILED) }
        if (broken != null) {
            assumeNoException(
                "environment network failure (report above): $broken",
                IllegalStateException(broken),
            )
        }
    }

    /** Sizes of the trust anchors the companion AAR itself loads vs the platform store. */
    private fun companionAnchors(): String =
        try {
            val verifier = Class.forName("org.rustls.platformverifier.CertificateVerifier")
            val keystore =
                verifier.getDeclaredField("systemKeystore").apply { isAccessible = true }
                    .get(null) as java.security.KeyStore
            val directory =
                verifier.getDeclaredField("systemCertificateDirectory").apply { isAccessible = true }
                    .get(null)
            val platform = java.security.KeyStore.getInstance("AndroidCAStore").apply { load(null) }
            "companion=${keystore.size()} dir=$directory platform=${platform.size()}"
        } catch (error: Throwable) {
            "$FAILED ${error.javaClass.simpleName}: ${error.message ?: error}"
        }

    /**
     * Fetch komiic.com's live chain, clean it through the platform trust
     * manager (as conscrypt does for HttpsURLConnection), then run the exact
     * PKIX validation the companion performs, reporting the true cause.
     */
    private fun pkixValidateChain(): String {
        val connection = URL("https://komiic.com/").openConnection() as HttpsURLConnection
        return try {
            connection.connectTimeout = CONNECT_TIMEOUT_MS
            connection.readTimeout = READ_TIMEOUT_MS
            connection.connect()
            val chain =
                connection.serverCertificates
                    .filterIsInstance<java.security.cert.X509Certificate>()
                    .toTypedArray()
            val factory =
                javax.net.ssl.TrustManagerFactory.getInstance(
                    javax.net.ssl.TrustManagerFactory.getDefaultAlgorithm(),
                )
            factory.init(null as java.security.KeyStore?)
            val platformManager =
                factory.trustManagers.filterIsInstance<javax.net.ssl.X509TrustManager>().first()
            val cleaned =
                android.net.http.X509TrustManagerExtensions(platformManager)
                    .checkServerTrusted(chain, "ECDSA", "komiic.com")

            val anchors = java.security.KeyStore.getInstance("AndroidCAStore").apply { load(null) }
            val params = java.security.cert.PKIXParameters(anchors)
            val validator = java.security.cert.CertPathValidator.getInstance("PKIX")
            val revocation =
                validator.revocationChecker as java.security.cert.PKIXRevocationChecker
            revocation.options =
                setOf(
                    java.security.cert.PKIXRevocationChecker.Option.SOFT_FAIL,
                    java.security.cert.PKIXRevocationChecker.Option.ONLY_END_ENTITY,
                )
            params.certPathCheckers = listOf(revocation)
            params.isRevocationEnabled = true
            val path =
                java.security.cert.CertificateFactory.getInstance("X.509").generateCertPath(cleaned)
            validator.validate(path, params)
            "chain=${cleaned.size} leaf=${cleaned[0].subjectX500Principal} root=${cleaned.last().subjectX500Principal} OK"
        } catch (error: java.security.cert.CertPathValidatorException) {
            throw IllegalStateException(
                "PKIX reason=${error.reason} index=${error.index} msg=${error.message} cause=${error.cause}",
                error,
            )
        } finally {
            connection.disconnect()
        }
    }

    private fun connectFirstLine(host: String, port: Int): String =
        Socket().use { socket ->
            socket.connect(InetSocketAddress(host, port), CONNECT_TIMEOUT_MS)
            socket.soTimeout = READ_TIMEOUT_MS
            socket.getOutputStream().apply {
                write("CONNECT komiic.com:443 HTTP/1.1\r\nHost: komiic.com:443\r\n\r\n".toByteArray())
                flush()
            }
            val line = BufferedReader(InputStreamReader(socket.getInputStream())).readLine()
            line ?: "no response line before read timeout"
        }

    private fun time(label: String, block: () -> String): String {
        val started = SystemClock.elapsedRealtime()
        val outcome =
            try {
                block()
            } catch (error: Throwable) {
                "$FAILED ${error.javaClass.name}: ${error.message ?: error}"
            }
        record(label, "$outcome (${SystemClock.elapsedRealtime() - started}ms)")
        return outcome
    }

    private fun record(label: String, value: String) {
        report.append(label.padEnd(46)).append(value).append('\n')
    }

    private companion object {
        const val TAG = "replayer-netdiag"
        const val FAILED = "FAILED:"
        const val CONNECT_TIMEOUT_MS = 5_000
        const val READ_TIMEOUT_MS = 10_000
    }
}
