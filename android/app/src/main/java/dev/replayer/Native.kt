package dev.replayer

import android.content.Context
import android.graphics.Bitmap
import android.graphics.Canvas
import org.json.JSONObject
import org.json.JSONTokener
import java.nio.ByteBuffer
import java.nio.ByteOrder

/**
 * JNI surface of libreplayer_android.so. Conventions:
 * - Text in/out are JSON strings; malformed records raise IllegalStateException
 *   with the bilingual messages the core produces.
 * - Binary results are containers: 4-byte big-endian JSON length, JSON
 *   metadata, raw payload (see [Container]).
 * - Media session methods are thread-pinned: open/read/seek/artwork/info/close
 *   for one handle must all run on the same worker thread (AudioEngine).
 */
object Native {
    /**
     * Loading the bridge can fail on a broken install (missing or incompatible
     * libreplayer_android.so); record it instead of crashing before the first
     * frame so [initialize] can report a diagnosable state.
     */
    private val loadError: Throwable? =
        runCatching { System.loadLibrary("replayer_android") }.exceptionOrNull()

    /** True once [initialize] completed; every JNI call requires it. */
    @Volatile
    var ready: Boolean = false
        private set

    /** Diagnostics for the last failed [initialize]; null while healthy. */
    @Volatile
    var initError: String? = null
        private set

    /**
     * Load remdia's Java side and initialize the native bridge with the
     * application context. Idempotent on success. A failure records [initError]
     * and leaves [ready] false so the caller can surface it and retry: the
     * native TLS verifier only stores state once it succeeds, and the data root
     * keeps the first value, so retrying is safe. Returns [ready].
     */
    @Synchronized
    fun initialize(context: Context): Boolean {
        if (ready) return true
        val load = loadError
        if (load != null) {
            initError = "原生库加载失败 / Native library load failed: $load"
            return false
        }
        val app = context.applicationContext
        initError = null
        try {
            // Package-private Bridge.initialize: remdia framework callbacks
            // (MediaCodec/MediaExtractor, content URIs) need the app context.
            dev.remdia.RemdiaBootstrap.initialize(app)
            nativeInit(app.filesDir.absolutePath, app)
            ready = true
        } catch (error: Throwable) {
            initError =
                buildString {
                    append(error.javaClass.name)
                    error.message?.let { append(": ").append(it) }
                }
            android.util.Log.e("replayer", "native initialization failed", error)
        }
        return ready
    }

    /** Parse any JSON document (objects, arrays, strings, null). */
    fun parse(text: String): Any? = JSONTokener(text).nextValue()

    // ---- comics ----
    external fun nativeComicSearch(keyword: String): String
    external fun nativeComicDetail(book: String): String
    external fun nativeComicPages(book: String, chapter: String, refresh: Boolean): String
    external fun nativeComicImagePolicy(book: String): String
    external fun nativeComicFetchPage(
        book: String,
        chapter: String,
        index: Int,
        url: String,
        width: Int,
    ): ByteArray

    external fun nativeComicFetchCover(book: String, width: Int): ByteArray

    // ---- bookshelf library ----
    external fun nativeLibraryEntries(): String
    external fun nativeLibrarySave(entry: String)
    external fun nativeLibraryRefresh(book: String, chapters: String)
    external fun nativeLibraryRoot(): String

    // ---- music ----
    external fun nativeMusicSources(apiKey: String): String
    external fun nativeMusicSearch(apiKey: String, sourceId: String, keyword: String, page: Int): String
    external fun nativeMusicDownloadStart(
        apiKey: String,
        sourceId: String,
        song: String,
        quality: String,
        directory: String,
    ): Long

    external fun nativeMusicDownloadPoll(handle: Long): String
    external fun nativeMusicDownloadCancel(handle: Long)
    external fun nativeMusicLyrics(path: String): String
    external fun nativeMusicCover(url: String, width: Int): ByteArray

    // ---- media playback (single worker thread per handle) ----
    external fun nativeMediaOpen(path: String): Long
    external fun nativeMediaInfo(handle: Long): String
    external fun nativeMediaArtwork(handle: Long): ByteArray?
    external fun nativeMediaRead(handle: Long): ByteArray?
    external fun nativeMediaSeek(handle: Long, targetUs: Long): Long
    external fun nativeMediaClose(handle: Long)

    private external fun nativeInit(root: String, context: Context)
}

/** Binary JNI container: metadata JSON plus a raw payload. */
class Container(bytes: ByteArray) {
    val meta: JSONObject
    val payload: ByteArray

    init {
        require(bytes.size >= 4) { "truncated container" }
        val length = ByteBuffer.wrap(bytes, 0, 4).order(ByteOrder.BIG_ENDIAN).int
        require(length in 0..bytes.size - 4) { "bad container metadata length" }
        meta = JSONObject(String(bytes, 4, length, Charsets.UTF_8))
        payload = bytes.copyOfRange(4 + length, bytes.size)
    }

    /** Assemble RGBA tiles into one bitmap; tiles are full-width strips. */
    fun bitmap(): Bitmap {
        val width = meta.getInt("width")
        val height = meta.getInt("height")
        val tiles = meta.getJSONArray("tiles")
        val full = Bitmap.createBitmap(width, height, Bitmap.Config.ARGB_8888)
        val canvas = Canvas(full)
        var y = 0
        for (i in 0 until tiles.length()) {
            val tile = tiles.getJSONObject(i)
            val tw = tile.getInt("width")
            val th = tile.getInt("height")
            val offset = tile.getInt("offset")
            val strip = Bitmap.createBitmap(tw, th, Bitmap.Config.ARGB_8888)
            val buffer = ByteBuffer.wrap(payload, offset, tw * th * 4)
            strip.copyPixelsFromBuffer(buffer)
            canvas.drawBitmap(strip, 0f, y.toFloat(), null)
            strip.recycle()
            y += th
        }
        return full
    }
}
