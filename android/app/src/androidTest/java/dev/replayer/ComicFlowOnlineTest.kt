package dev.replayer

import androidx.test.ext.junit.runners.AndroidJUnit4
import dev.replayer.comic.ComicRepository
import org.junit.Assume.assumeNoException
import org.junit.Assume.assumeTrue
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

/**
 * The real HTTPS path through the core: search, detail, page list and one
 * decoded page image. Gated behind the `online` instrumentation argument
 * because it depends on an external site; an outage or a source-side rejection
 * is reported as a skipped assumption, while anything the bridge itself caused
 * (a recovered panic, an uninitialized TLS verifier) fails the test.
 *
 * Run with `scripts/android-dev.py smoke --online`.
 */
@RunWith(AndroidJUnit4::class)
class ComicFlowOnlineTest {
    @Test
    fun searchDetailPagesAndImageOverHttps() {
        assumeTrue("online checks need --online", onlineRequested())
        assertTrue(Native.initError ?: "bridge not ready", Native.initialize(testContext))

        val groups = external("search") { ComicRepository.search("one piece") }
        val books = groups.flatMap { it.books }
        assumeTrue("source returned no books", books.isNotEmpty())
        val book = books.first()

        val detail = external("detail") { ComicRepository.detail(book) }
        val chapters = detail.chapters.filterNot { it.isVolume }.ifEmpty { detail.chapters }
        assumeTrue("source returned no chapters", chapters.isNotEmpty())
        val chapter = chapters.first()

        val pages = external("pages") { ComicRepository.pages(book, chapter) }
        assertTrue("chapter has no pages", pages.isNotEmpty())

        val bitmap = external("page image") { ComicRepository.fetchPage(book, chapter, 0, pages[0]) }
        assertTrue("decoded page is empty: ${bitmap.width}x${bitmap.height}", bitmap.width > 0 && bitmap.height > 0)
    }

    /**
     * Run [block], classifying a failure: implementation defects fail the test,
     * external ones skip it with the original cause preserved.
     */
    private fun <T> external(step: String, block: () -> T): T =
        try {
            block()
        } catch (error: Throwable) {
            val message = error.message ?: error.toString()
            assertNotImplementationFailure(message)
            if (!isExternalNetworkFailure(message)) {
                // JSON/protocol/NoSuchMethod and other defects are ours, not
                // the network's; they must fail loudly instead of skipping.
                throw error
            }
            assumeNoException("external failure during $step", error)
            throw error
        }

    /**
     * Only failures whose core-side context text identifies a network, HTTP
     * status or quota problem may be skipped as external. The core preserves
     * reqwest's source chain (`{error:#}` at the JNI boundary), so the message
     * shows the concrete layer after these markers.
     */
    private fun isExternalNetworkFailure(message: String): Boolean =
        EXTERNAL_MARKERS.any { message.contains(it) }

    private companion object {
        val EXTERNAL_MARKERS =
            listOf(
                "连接失败", // network layer (komiic/comic source/image/DM5)
                "request failed",
                "timed out",
                "超时",
                "HTTP 4",
                "HTTP 5",
                "Source unavailable",
                "Source rejected",
                "额度已用尽",
                "QUOTA_EXCEEDED",
            )
    }
}
