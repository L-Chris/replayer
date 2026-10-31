package dev.pordo

import dev.pordo.comic.ContentService
import android.content.Context
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

class ContentServiceTest {
    @Test fun deviceTokenStaysInVaultAndSurvivesRestartAndRetargeting() {
        assertTrue(Native.initialize(testContext))
        val previous = ContentService.saved(testContext)
        val prefs = testContext.getSharedPreferences("content-service", Context.MODE_PRIVATE)
        try {
            ContentService.save(testContext, "https://content.example.test/", "fixture-private-token")
            assertEquals("https://content.example.test", ContentService.saved(testContext).getString("url"))
            assertEquals("fixture-private-token", ContentService.saved(testContext).getString("token"))
            assertFalse(JSONObject(prefs.all).toString().contains("fixture-private-token"))
            ContentService.initialize(testContext)
            assertEquals("fixture-private-token", ContentService.saved(testContext).getString("token"))
            ContentService.save(testContext, "https://content.example.test", "")
            assertEquals("fixture-private-token", ContentService.saved(testContext).getString("token"))
            assertThrows(IllegalStateException::class.java) { ContentService.save(testContext, "https://different.example.test", "") }
            assertEquals("https://content.example.test", ContentService.saved(testContext).getString("url"))
            ContentService.save(testContext, "", "")
            assertEquals("", ContentService.saved(testContext).getString("token"))
        } finally {
            ContentService.save(testContext, previous.optString("url"), previous.optString("token"))
        }
    }
    @Test fun realServiceReadsLegacyFavoritesDirectoriesAndImages() {
        val file = java.io.File(testContext.filesDir, "content-service-fixture.json")
        org.junit.Assume.assumeTrue("opt-in local service fixture required", file.exists())
        assertTrue(Native.initialize(testContext))
        val fixture = JSONObject(file.readText())
        val previous = ContentService.saved(testContext)
        val book = dev.pordo.comic.Book(fixture.getJSONObject("book"))
        try {
            ContentService.save(testContext, fixture.getString("url"), fixture.getString("token"))
            val sites = JSONObject(Native.nativeComicServiceTest()).getJSONArray("sites")
            assertTrue(sites.length() > 0)
            // Open the existing book identity before doing any search.
            val detail = dev.pordo.comic.ComicRepository.detail(book)
            assertEquals(book.provider, detail.book.provider)
            assertEquals(book.siteId, detail.book.siteId)
            assertEquals(book.url, detail.book.url)
            assertTrue(dev.pordo.comic.ComicRepository.search("fixture").flatMap { it.books }.isNotEmpty())
            val chapter = detail.chapters.first()
            assertEquals("https://site-a.example/ch/1", chapter.url)
            val pages = dev.pordo.comic.ComicRepository.pages(book, chapter)
            val image = dev.pordo.comic.ComicRepository.fetchPage(book, chapter, 0, pages.first())
            assertTrue(image.width > 0 && image.height > 0)
            image.recycle()
            val entry = dev.pordo.comic.ComicRepository.setFavorite(book, detail.chapters, true)
            dev.pordo.comic.ComicRepository.saveProgress(entry, chapter, 0, 0.5f, pages.size, completed = true)
            val restored = dev.pordo.comic.ComicRepository.entryFor(book)!!
            assertTrue(restored.favorite)
            assertTrue(restored.readKeys.contains(chapter.key))
            assertEquals(0.5f, restored.progress!!.fraction)
            dev.pordo.comic.ComicRepository.setFavorite(book, detail.chapters, false)
        } finally {
            ContentService.save(testContext, previous.optString("url"), previous.optString("token"))
        }
    }

    @Test fun invalidServiceConfigurationNeverChangesAnExistingConnection() {
        assertTrue(Native.initialize(testContext))
        val previous = ContentService.saved(testContext)
        try {
            ContentService.save(testContext, "https://content.example.test", "fixture-token")
            for (url in listOf("file:///secret", "https://user:password@example.test", "https://example.test?token=secret")) {
                assertThrows(IllegalStateException::class.java) { ContentService.save(testContext, url, "fixture-token") }
            }
            assertEquals("https://content.example.test", ContentService.saved(testContext).getString("url"))
        } finally {
            ContentService.save(testContext, previous.optString("url"), previous.optString("token"))
        }
    }
}
