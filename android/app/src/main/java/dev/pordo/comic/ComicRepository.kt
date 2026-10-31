package dev.pordo.comic

import android.graphics.Bitmap
import dev.pordo.Container
import dev.pordo.Native
import org.json.JSONArray
import org.json.JSONObject

/**
 * Bookshelf, discovery and page access. Records stay as the core's JSON
 * (serde shapes) so saves round-trip losslessly; typed views are read-only
 * conveniences over the raw objects.
 */

class Book(val raw: JSONObject) {
    val provider: String get() = raw.optString("provider")
    val siteId: String get() = raw.optString("siteId")
    val siteName: String get() = raw.optString("siteName")
    val title: String get() = raw.optString("title")
    val author: String get() = raw.optString("author")
    val description: String get() = raw.optString("description")
    val status: String get() = raw.optString("status")
    val coverUrl: String get() = raw.optString("coverUrl")
    val url: String get() = raw.optString("detailUrl")
    override fun toString(): String = raw.toString()
}

class Chapter(val raw: JSONObject) {
    val id: String get() = raw.optString("id")
    val name: String get() = raw.optString("name")
    val url: String get() = raw.optString("url")
    val order: Long get() = raw.optLong("order")
    val isVolume: Boolean get() = raw.optString("kind") == "Volume"

    /** Matches the core cache key: id when present, else url. */
    val key: String get() = id.ifEmpty { url }
    override fun toString(): String = raw.toString()
}

class Progress(raw: JSONObject?) {
    val raw: JSONObject = raw ?: JSONObject()
    val chapter: String get() = this.raw.optString("chapter")
    val page: Int get() = this.raw.optInt("page")
    val fraction: Float get() = this.raw.optDouble("fraction", 0.0).toFloat()
    val pageCount: Int get() = this.raw.optInt("page_count")
}

class LibraryEntry(val raw: JSONObject) {
    val book: Book get() = Book(raw.getJSONObject("book"))
    val chapters: List<Chapter>
        get() = raw.getJSONArray("chapters").let { array ->
            (0 until array.length()).map { Chapter(array.getJSONObject(it)) }
        }
    val favorite: Boolean get() = raw.optBoolean("favorite")
    val favoritedAt: Long get() = raw.optLong("favorited_at")
    val lastRead: Long get() = raw.optLong("last_read")
    val progress: Progress?
        get() = raw.optJSONObject("progress")?.let { Progress(it) }

    val knownKeys: Set<String> get() = stringSet("known")
    val readKeys: Set<String> get() = stringSet("read")
    val newUnreadCount: Int get() = stringSet("new_unread").size

    private fun stringSet(name: String): Set<String> {
        val array = raw.optJSONArray(name) ?: return emptySet()
        return (0 until array.length()).map { array.getString(it) }.toSet()
    }


}

data class SearchGroup(val siteId: String, val siteName: String, val books: List<Book>, val error: String?)

data class ImagePolicy(
    val concurrency: Int,
    val ahead: Int,
    val behind: Int,
    val retainCacheOnRefresh: Boolean,
)

object ComicRepository {
    /** Page decode width; identical to the desktop reader so caches match. */
    const val PAGE_WIDTH = 1800
    const val COVER_WIDTH = 240

    fun entries(): List<LibraryEntry> {
        val array = Native.parse(Native.nativeLibraryEntries()) as? JSONArray ?: return emptyList()
        return (0 until array.length()).map { LibraryEntry(array.getJSONObject(it)) }
    }

    fun entryFor(book: Book): LibraryEntry? = entries().firstOrNull { it.book.url == book.url }

    fun search(keyword: String): List<SearchGroup> {
        val array = Native.parse(Native.nativeComicSearch(keyword)) as? JSONArray ?: return emptyList()
        return (0 until array.length()).map { i ->
            val group = array.getJSONObject(i)
            val site = group.getJSONObject("site")
            val books = group.getJSONArray("books")
            SearchGroup(
                siteId = site.getString("siteId"),
                siteName = site.getString("siteName"),
                books = (0 until books.length()).map { Book(books.getJSONObject(it)) },
                error = if (group.isNull("error")) null else group.optString("error"),
            )
        }
    }

    data class Detail(val book: Book, val chapters: List<Chapter>)

    fun detail(book: Book): Detail {
        val json = JSONObject(Native.nativeComicDetail(book.raw.toString()))
        val chapters = json.getJSONArray("chapters")
        return Detail(
            book = Book(json.getJSONObject("book")),
            chapters = (0 until chapters.length()).map { Chapter(chapters.getJSONObject(it)) },
        )
    }

    fun pages(book: Book, chapter: Chapter, refresh: Boolean = false): List<String> {
        val array = JSONArray(
            Native.nativeComicPages(book.raw.toString(), chapter.raw.toString(), refresh),
        )
        return (0 until array.length()).map { array.getString(it) }
    }

    fun imagePolicy(book: Book): ImagePolicy {
        val json = JSONObject(Native.nativeComicImagePolicy(book.raw.toString()))
        return ImagePolicy(
            concurrency = json.optInt("concurrency", 4),
            ahead = json.optInt("ahead", 3),
            behind = json.optInt("behind", 1),
            retainCacheOnRefresh = json.optBoolean("retainCacheOnRefresh"),
        )
    }

    fun fetchPage(book: Book, chapter: Chapter, index: Int, url: String): Bitmap =
        Container(
            Native.nativeComicFetchPage(
                book.raw.toString(),
                chapter.raw.toString(),
                index,
                url,
                PAGE_WIDTH,
            ),
        ).bitmap()

    fun fetchCover(book: Book): Bitmap =
        Container(Native.nativeComicFetchCover(book.raw.toString(), COVER_WIDTH)).bitmap()

    /** Add or merge a book's fresh details into the shelf. */
    fun refreshIntoLibrary(detail: Detail) {
        val chapters = JSONArray()
        detail.chapters.forEach { chapters.put(it.raw) }
        Native.nativeLibraryRefresh(detail.book.raw.toString(), chapters.toString())
        dev.pordo.sync.CloudSync.changed()
    }

    fun setFavorite(book: Book, chapters: List<Chapter>, favorite: Boolean): LibraryEntry {
        // Ensure an entry exists, then toggle through the typed view so the
        // JSON round-trips with every core field preserved.
        val existing = entryFor(book)
        if (existing == null) {
            refreshIntoLibrary(Detail(book, chapters))
        }
        val entry = entryFor(book) ?: error("无法加入书架 / Could not shelve the book")
        Native.nativeLibraryUpdate(book.raw.toString(), JSONObject().put("favorite", favorite).toString())
        dev.pordo.sync.CloudSync.changed()
        return entryFor(book) ?: entry
    }

    /** Update the latest row; only a completed chapter joins the read set. */
    fun saveProgress(entry: LibraryEntry, chapter: Chapter, page: Int, fraction: Float, pageCount: Int, completed: Boolean = false) {
        val progress = JSONObject().put("chapter", chapter.key).put("page", page)
            .put("fraction", fraction.toDouble()).put("page_count", pageCount)
        Native.nativeLibraryUpdate(entry.book.raw.toString(),
            JSONObject().put("progress", progress).put("completed", completed).toString())
        dev.pordo.sync.CloudSync.changed(progressOnly = true)
    }
}
