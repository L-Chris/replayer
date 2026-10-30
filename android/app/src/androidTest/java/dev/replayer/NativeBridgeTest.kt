package dev.replayer

import androidx.test.ext.junit.runners.AndroidJUnit4
import org.json.JSONArray
import org.junit.Assert.assertNull
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

/**
 * Offline bridge contract: initialization state, sandboxed paths, JSON shapes
 * and the error taxonomy. Nothing here touches the network, so a failure means
 * the JNI surface itself regressed rather than an external site changed.
 */
@RunWith(AndroidJUnit4::class)
class NativeBridgeTest {
    @Test
    fun initializeSucceedsAndIsIdempotent() {
        assertTrue(Native.initError ?: "bridge not ready", Native.initialize(testContext))
        assertNull(Native.initError)
        assertTrue("a second initialize must keep the bridge ready", Native.initialize(testContext))
    }

    @Test
    fun libraryRootStaysInsideTheSandbox() {
        assertTrue(Native.initialize(testContext))
        val root = Native.parse(Native.nativeLibraryRoot()) as String
        assertTrue("$root not under ${testContext.filesDir}", root.startsWith(testContext.filesDir.absolutePath))
    }

    @Test
    fun libraryEntriesIsAJsonArray() {
        assertTrue(Native.initialize(testContext))
        assertTrue(Native.parse(Native.nativeLibraryEntries()) is JSONArray)
    }

    @Test
    fun blankSearchIsRejectedWithASourceMessage() {
        assertTrue(Native.initialize(testContext))
        val error = assertThrows(IllegalStateException::class.java) { Native.nativeComicSearch("   ") }
        val message = error.message ?: ""
        assertTrue("unexpected message: $message", message.contains("漫画名称") || message.contains("comic title"))
        assertNotImplementationFailure(message)
    }

    @Test
    fun malformedLibraryEntryIsRejected() {
        assertTrue(Native.initialize(testContext))
        // `Entry` is `#[serde(default)]`, so `{}` and other partial objects are
        // valid; a wrong-typed field is what actually fails deserialization and
        // must surface as the core's bilingual "invalid entry" message.
        val error =
            assertThrows(IllegalStateException::class.java) {
                Native.nativeLibrarySave("""{"favorite":"not-a-bool"}""")
            }
        val message = error.message ?: ""
        assertTrue("unexpected message: $message", message.contains("书架记录") || message.contains("library entry"))
        assertNotImplementationFailure(message)
    }
}
