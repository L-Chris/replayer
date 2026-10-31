package dev.pordo

import androidx.test.platform.app.InstrumentationRegistry
import android.content.Context
import org.junit.Assert.assertFalse

/** Shared helpers for the instrumented smoke tests. */

val testContext: Context get() = InstrumentationRegistry.getInstrumentation().targetContext

/**
 * True only for runs that explicitly asked for external-network checks, so the
 * default suite stays deterministic and offline:
 * `scripts/android-dev.py smoke --online`.
 */
fun onlineRequested(): Boolean =
    InstrumentationRegistry.getArguments().getString("online") == "true"

/**
 * Every JNI body is wrapped in `catch_unwind` and recovered panics are prefixed,
 * so an implementation defect is distinguishable from a source, validation or
 * network error. External failures may be tolerated by a test; these may not.
 */
fun assertNotImplementationFailure(message: String) {
    assertFalse("recovered native panic: $message", message.contains("Native panic"))
    assertFalse("TLS verifier failure: $message", message.contains("TLS verifier"))
    assertFalse("bridge initialization failure: $message", message.contains("initialization failed"))
    assertFalse("bridge initialization failure: $message", message.contains("初始化失败"))
}
