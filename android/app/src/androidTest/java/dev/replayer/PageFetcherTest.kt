package dev.replayer

import android.graphics.Bitmap
import androidx.test.ext.junit.runners.AndroidJUnit4
import dev.replayer.comic.PageFetcher
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class PageFetcherTest {
    @Test
    fun policyAllowsTwoParallelLoadsAndQueuesTheRest() {
        val fetcher = PageFetcher(cacheBytes = 1024 * 1024, concurrency = 2)
        val started = CountDownLatch(2)
        val release = CountDownLatch(1)
        val completed = CountDownLatch(5)
        val active = AtomicInteger()
        val peak = AtomicInteger()
        try {
            repeat(5) { index ->
                fetcher.fetch("page-$index") {
                    val count = active.incrementAndGet()
                    peak.updateAndGet { maxOf(it, count) }
                    started.countDown()
                    try {
                        check(release.await(5, TimeUnit.SECONDS))
                        Bitmap.createBitmap(2, 2, Bitmap.Config.ARGB_8888)
                    } finally {
                        active.decrementAndGet()
                        completed.countDown()
                    }
                }
            }
            assertTrue("two loads should start together", started.await(3, TimeUnit.SECONDS))
            assertEquals(2, active.get())
            release.countDown()
            assertTrue("queued pages should complete", completed.await(3, TimeUnit.SECONDS))
            assertEquals(2, peak.get())
        } finally {
            release.countDown()
            fetcher.close()
        }
    }
}
