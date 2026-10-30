package dev.replayer.comic

import android.graphics.Bitmap
import android.util.LruCache
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import dev.replayer.AppNav
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.debounce
import kotlinx.coroutines.launch
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.ensureActive
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.withContext
import org.json.JSONObject
import java.util.concurrent.ConcurrentHashMap

/**
 * Webtoon reader: one vertical LazyColumn, pages decoded at the shared cache
 * width. Bitmaps live in a byte-sized LruCache; the core's page cache makes
 * refetches cheap, so eviction only costs a decode. [ImagePolicy.concurrency]
 * bounds parallel fetches and [ImagePolicy.ahead] drives prefetching from the
 * first visible index.
 */
internal class PageFetcher(cacheBytes: Int, concurrency: Int) {
    // JNI loads block synchronously, so one dispatcher view is sufficient to
    // cap concurrent fetches. A second resizable semaphore is unnecessary.
    private val scope = CoroutineScope(
        SupervisorJob() + Dispatchers.IO.limitedParallelism(concurrency.coerceAtLeast(1)),
    )
    val bitmaps = MutableStateFlow<Map<String, Bitmap>>(emptyMap())
    val failures = MutableStateFlow<Set<String>>(emptySet())
    private val cache = object : LruCache<String, Bitmap>(cacheBytes) {
        override fun sizeOf(key: String, value: Bitmap): Int = value.byteCount
        override fun entryRemoved(
            evicted: Boolean,
            key: String,
            oldValue: Bitmap,
            newValue: Bitmap?,
        ) {
            if (newValue == null) publish()
        }
    }
    private val inFlight = ConcurrentHashMap.newKeySet<String>()

    private fun publish() {
        bitmaps.value = cache.snapshot()
    }

    /** Idempotent: repeats for cached or in-flight pages are no-ops. */
    fun fetch(key: String, load: () -> Bitmap) {
        if (cache.get(key) != null || !inFlight.add(key)) return
        scope.launch {
            try {
                val bitmap = load()
                ensureActive()
                cache.put(key, bitmap)
                publish()
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (_: Exception) {
                failures.update { it + key }
            }
        }.invokeOnCompletion { inFlight.remove(key) }
    }

    fun retry(key: String, load: () -> Bitmap) {
        failures.update { it - key }
        cache.remove(key)
        fetch(key, load)
    }

    fun close() {
        scope.cancel()
        cache.evictAll()
    }
}

@OptIn(ExperimentalMaterial3Api::class, kotlinx.coroutines.FlowPreview::class)
@Composable
fun ReaderScreen(onBack: () -> Unit) {
    val bookJson = AppNav.bookJson
    val chapterJson = AppNav.chapterJson
    val entryJson = AppNav.entryJson
    val book = remember(bookJson) { bookJson?.let { Book(JSONObject(it)) } }
    val chapter = remember(chapterJson) { chapterJson?.let { Chapter(JSONObject(it)) } }
    val entry = remember(entryJson) { entryJson?.let { LibraryEntry(JSONObject(it)) } }

    var pages by remember { mutableStateOf<List<String>?>(null) }
    var loadError by remember { mutableStateOf<String?>(null) }
    var policy by remember { mutableStateOf(ImagePolicy(4, 3, 1, false)) }

    // Fetcher lifetime spans recompositions; the cache budget follows the
    // process memory class like a full-screen image viewer would.
    val fetcher = remember(bookJson, chapterJson, policy.concurrency) {
        val maxBytes = (Runtime.getRuntime().maxMemory() / 6).toInt()
        PageFetcher(
            cacheBytes = maxBytes,
            concurrency = policy.concurrency,
        )
    }
    DisposableEffect(fetcher) {
        onDispose { fetcher.close() }
    }

    val listState = rememberLazyListState(initialFirstVisibleItemIndex = AppNav.initialPage)
    val bitmaps by fetcher.bitmaps.collectAsStateWithLifecycle()
    val failures by fetcher.failures.collectAsStateWithLifecycle()

    // Loading a chapter does not mark it read; completion requires its last
    // decoded page to reach the bottom of the viewport, as on desktop.
    LaunchedEffect(bookJson, chapterJson) {
        val loadedBook = book ?: return@LaunchedEffect
        val loadedChapter = chapter ?: return@LaunchedEffect
        val result = runCatching {
            withContext(Dispatchers.IO) {
                val list = ComicRepository.pages(loadedBook, loadedChapter)
                policy = ComicRepository.imagePolicy(loadedBook)
                list
            }
        }
        result.onSuccess { pages = it }.onFailure { loadError = it.message }
    }

    // The wire fraction is within the current page, matching the desktop reader.
    val current = pages
    val loadedLast = current?.lastOrNull()?.let { bitmaps.containsKey(pageKey(current.lastIndex, it)) } == true
    fun position(): Triple<Int, Float, Boolean> {
        val layout = listState.layoutInfo
        val index = listState.firstVisibleItemIndex
        val first = layout.visibleItemsInfo.firstOrNull { it.index == index }
        val fraction = if (first != null && first.size > 0) listState.firstVisibleItemScrollOffset.toFloat() / first.size else 0f
        val tail = layout.visibleItemsInfo.lastOrNull()
        val complete = loadedLast && tail != null && tail.index == current?.lastIndex && tail.offset + tail.size <= layout.viewportEndOffset + 2
        return Triple(index, fraction.coerceIn(0f, 1f), complete)
    }
    DisposableEffect(current, entry, chapter, loadedLast) {
        onDispose {
            val loaded = current?.takeIf { it.isNotEmpty() } ?: return@onDispose
            val shelf = entry ?: return@onDispose
            val loadedChapter = chapter ?: return@onDispose
            val (index, fraction, complete) = position()
            runCatching { ComicRepository.saveProgress(shelf, loadedChapter, index.coerceIn(loaded.indices), fraction, loaded.size, complete) }
        }
    }
    LaunchedEffect(listState, current, loadedLast) {
        if (current.isNullOrEmpty()) return@LaunchedEffect
        snapshotFlow { position() }.distinctUntilChanged().debounce(500).collect { (index, fraction, complete) ->
            val shelf = entry ?: return@collect
            val loadedChapter = chapter ?: return@collect
            withContext(Dispatchers.IO) {
                runCatching { ComicRepository.saveProgress(shelf, loadedChapter, index.coerceIn(current.indices), fraction, current.size, complete) }
            }
        }
    }
    var restored by remember(bookJson, chapterJson) { mutableStateOf(false) }
    val initial = current?.getOrNull(AppNav.initialPage)?.let { bitmaps[pageKey(AppNav.initialPage, it)] }
    LaunchedEffect(initial) {
        if (!restored && initial != null) {
            restored = true
            val height = initial.height.toLong() * listState.layoutInfo.viewportSize.width / initial.width
            listState.scrollToItem(AppNav.initialPage, (height * AppNav.initialFraction.coerceIn(0f, 1f)).toInt())
        }
    }

    // Prefetch the window ahead of the reader.
    LaunchedEffect(listState, current, policy) {
        val urls = current ?: return@LaunchedEffect
        snapshotFlow { listState.firstVisibleItemIndex }
            .distinctUntilChanged()
            .collect { index ->
                for (offset in 0..policy.ahead) {
                    val target = index + offset
                    if (target >= urls.size) break
                    val url = urls[target]
                    fetcher.fetch(pageKey(target, url)) {
                        ComicRepository.fetchPage(book!!, chapter!!, target, url)
                    }
                }
            }
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = {
                    Column {
                        Text(
                            chapter?.name ?: "",
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                            style = MaterialTheme.typography.titleMedium,
                        )
                        val total = current?.size ?: 0
                        if (total > 0) {
                            Text(
                                "第 ${listState.firstVisibleItemIndex + 1} / $total 页",
                                style = MaterialTheme.typography.bodySmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                    }
                },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, "返回")
                    }
                },
            )
        },
    ) { padding ->
        when {
            book == null || chapter == null -> Box(
                Modifier.padding(padding).fillMaxSize(),
                Alignment.Center,
            ) {
                Text("记录丢失，请返回重进", color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
            loadError != null -> Box(
                Modifier.padding(padding).fillMaxSize(),
                Alignment.Center,
            ) {
                Column(horizontalAlignment = Alignment.CenterHorizontally) {
                    Text(
                        loadError ?: "",
                        color = MaterialTheme.colorScheme.error,
                        style = MaterialTheme.typography.bodySmall,
                        modifier = Modifier.padding(horizontal = 24.dp),
                    )
                    TextButton(onClick = onBack) { Text("返回") }
                }
            }
            current == null -> Box(
                Modifier.padding(padding).fillMaxSize(),
                Alignment.Center,
            ) { CircularProgressIndicator() }
            else -> LazyColumn(
                state = listState,
                modifier = Modifier.padding(padding).fillMaxSize(),
            ) {
                items(count = current.size) { index ->
                    val url = current[index]
                    val key = pageKey(index, url)
                    PageSlot(
                        bitmap = bitmaps[key],
                        failed = key in failures,
                        onRetry = {
                            fetcher.retry(key) {
                                ComicRepository.fetchPage(book, chapter, index, url)
                            }
                        },
                        onVisible = {
                            fetcher.fetch(key) {
                                ComicRepository.fetchPage(book, chapter, index, url)
                            }
                        },
                    )
                }
            }
        }
    }
}

private fun pageKey(index: Int, url: String) = "$index:$url"

@Composable
private fun PageSlot(
    bitmap: Bitmap?,
    failed: Boolean,
    onRetry: () -> Unit,
    onVisible: () -> Unit,
) {
    LaunchedEffect(Unit) { onVisible() }
    Box(
        Modifier
            .fillMaxWidth()
            .aspectRatio(if (bitmap != null) bitmap.width.toFloat() / bitmap.height else 1.4f)
            .background(MaterialTheme.colorScheme.surfaceVariant),
        contentAlignment = Alignment.Center,
    ) {
        when {
            bitmap != null -> Image(
                bitmap.asImageBitmap(),
                null,
                Modifier.fillMaxSize(),
                contentScale = ContentScale.FillWidth,
            )
            failed -> TextButton(onClick = onRetry) { Text("加载失败，点击重试") }
            else -> CircularProgressIndicator()
        }
    }
}
