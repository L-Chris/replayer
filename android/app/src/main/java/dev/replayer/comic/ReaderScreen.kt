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
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Semaphore
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
private class PageFetcher(
    cacheBytes: Int,
    private val scope: CoroutineScope,
) {
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
    private val permits = java.util.concurrent.atomic.AtomicInteger(1)
    private val gate = Semaphore(1)

    /** Apply the book's ImagePolicy; only concurrency affects the gate. */
    fun setConcurrency(value: Int) {
        val target = value.coerceAtLeast(1)
        val delta = target - permits.getAndSet(target)
        repeat(delta.coerceAtLeast(0)) { gate.release() }
    }

    private fun publish() {
        bitmaps.value = HashMap(cache.snapshot())
    }

    /** Idempotent: repeats for cached or in-flight pages are no-ops. */
    fun fetch(key: String, load: () -> Bitmap) {
        if (cache.get(key) != null || !inFlight.add(key)) return
        scope.launch {
            gate.acquire()
            try {
                val bitmap = load()
                cache.put(key, bitmap)
                publish()
            } catch (_: Exception) {
                failures.value = failures.value + key
            } finally {
                gate.release()
            }
        }.invokeOnCompletion { inFlight.remove(key) }
    }

    fun retry(key: String, load: () -> Bitmap) {
        failures.value = failures.value - key
        cache.remove(key)
        fetch(key, load)
    }

    fun close() {
        scope.cancel()
        cache.evictAll()
    }
}

@OptIn(ExperimentalMaterial3Api::class)
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
    val fetcher = remember(bookJson, chapterJson) {
        val maxBytes = (Runtime.getRuntime().maxMemory() / 6).toInt()
        PageFetcher(
            cacheBytes = maxBytes,
            scope = CoroutineScope(SupervisorJob() + Dispatchers.IO.limitedParallelism(4)),
        )
    }
    DisposableEffect(fetcher) {
        onDispose { fetcher.close() }
    }

    val listState = rememberLazyListState(initialFirstVisibleItemIndex = AppNav.initialPage)
    val bitmaps by fetcher.bitmaps.collectAsStateWithLifecycle()
    val failures by fetcher.failures.collectAsStateWithLifecycle()

    // Load the page list once; the entry is marked opened so shelf refreshes
    // stop counting the chapter as new.
    LaunchedEffect(bookJson, chapterJson) {
        val loadedBook = book ?: return@LaunchedEffect
        val loadedChapter = chapter ?: return@LaunchedEffect
        val result = runCatching {
            withContext(Dispatchers.IO) {
                val list = ComicRepository.pages(loadedBook, loadedChapter)
                policy = ComicRepository.imagePolicy(loadedBook)
                fetcher.setConcurrency(policy.concurrency)
                if (entry != null) {
                    entry.markOpened(loadedChapter.key)
                    entry.save()
                }
                list
            }
        }
        result.onSuccess { pages = it }.onFailure { loadError = it.message }
    }

    // Save progress whenever the visible page settles, and once more on exit.
    val current = pages
    DisposableEffect(current, entry, chapter) {
        onDispose {
            val loaded = current ?: return@onDispose
            val shelf = entry ?: return@onDispose
            val loadedChapter = chapter ?: return@onDispose
            val index = listState.firstVisibleItemIndex.coerceIn(0, loaded.size - 1)
            runCatching {
                ComicRepository.saveProgress(
                    shelf,
                    loadedChapter,
                    index,
                    (index + 1f) / loaded.size,
                    loaded.size,
                )
            }
        }
    }
    LaunchedEffect(listState, current) {
        if (current == null) return@LaunchedEffect
        snapshotFlow { listState.firstVisibleItemIndex }
            .distinctUntilChanged()
            .collect { index ->
                val shelf = entry ?: return@collect
                val loadedChapter = chapter ?: return@collect
                withContext(Dispatchers.IO) {
                    runCatching {
                        ComicRepository.saveProgress(
                            shelf,
                            loadedChapter,
                            index,
                            (index + 1f) / current.size,
                            current.size,
                        )
                    }
                }
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
            .aspectRatio(if (bitmap != null) bitmap.height.toFloat() / bitmap.width else 1.4f)
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
