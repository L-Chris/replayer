package dev.pordo.comic

import androidx.compose.ui.res.painterResource
import dev.pordo.R
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.GridItemSpan
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items as gridItems
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Favorite
import androidx.compose.material.icons.filled.FavoriteBorder
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material.icons.filled.Search
import androidx.compose.material3.Badge
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Tab
import androidx.compose.material3.TabRow
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import dev.pordo.AppNav
import dev.pordo.sync.CloudSync
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONObject

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ComicHome(onOpenDetail: () -> Unit, onOpenReader: () -> Unit, onOpenSync: () -> Unit, onOpenSourceSettings: () -> Unit) {
    var tab by rememberSaveable { mutableIntStateOf(0) }
    Scaffold(topBar = { TopAppBar(title = { Text("漫画") }, actions = {
        TextButton(onClick = onOpenSourceSettings, modifier = Modifier.testTag("nav-comic-source")) { Text("源设置") }
        TextButton(onClick = onOpenSync, modifier = Modifier.testTag("nav-cloud")) { Text("云同步") }
    }) }) { padding ->
        Column(Modifier.padding(padding).fillMaxSize()) {
            TabRow(selectedTabIndex = tab) {
                Tab(
                    selected = tab == 0,
                    onClick = { tab = 0 },
                    text = { Text("书架") },
                    modifier = Modifier.testTag("comic-tab-shelf"),
                )
                Tab(
                    selected = tab == 1,
                    onClick = { tab = 1 },
                    text = { Text("搜索") },
                    modifier = Modifier.testTag("comic-tab-search"),
                )
            }
            when (tab) {
                0 -> BookshelfTab(onOpenDetail, onOpenReader)
                1 -> ComicSearchTab(onOpenDetail)
            }
        }
    }
}

@Composable
private fun BookshelfTab(onOpenDetail: () -> Unit, onOpenReader: () -> Unit) {
    var entries by remember { mutableStateOf<List<LibraryEntry>>(emptyList()) }
    var loading by remember { mutableStateOf(true) }
    var reload by remember { mutableIntStateOf(0) }
    var menuEntry by remember { mutableStateOf<LibraryEntry?>(null) }
    val scope = rememberCoroutineScope()
    val syncRevision by CloudSync.revision.collectAsStateWithLifecycle()
    LaunchedEffect(reload, syncRevision) {
        loading = true
        entries = withContext(Dispatchers.IO) {
            runCatching { ComicRepository.entries() }.getOrElse { emptyList() }
        }
        loading = false
    }
    val favorites = entries.filter { it.favorite }.sortedByDescending { it.favoritedAt }
    val history = entries.filterNot { it.favorite }.sortedByDescending { it.lastRead }
    if (loading) {
        Box(Modifier.fillMaxSize(), Alignment.Center) { CircularProgressIndicator() }
        return
    }
    if (entries.isEmpty()) {
        Box(Modifier.fillMaxSize(), Alignment.Center) {
            Text(
                "书架是空的。在“搜索”页签找一本书，进入详情后收藏。",
                style = MaterialTheme.typography.bodyMedium,
                modifier = Modifier.padding(32.dp),
            )
        }
        return
    }
    LazyVerticalGrid(
        columns = GridCells.Fixed(3),
        modifier = Modifier.fillMaxSize(),
        contentPadding = androidx.compose.foundation.layout.PaddingValues(10.dp),
        horizontalArrangement = Arrangement.spacedBy(10.dp),
        verticalArrangement = Arrangement.spacedBy(4.dp),
    ) {
        if (favorites.isNotEmpty()) {
            item(span = { GridItemSpan(maxLineSpan) }) { SectionHeader("收藏") }
            gridItems(favorites, key = { "favorite:${it.book.url}" }) { entry ->
                BookCell(
                    entry = entry,
                    onOpen = { openEntry(entry, onOpenDetail, onOpenReader) },
                    onLongPress = { menuEntry = entry },
                )
            }
        }
        if (history.isNotEmpty()) {
            item(span = { GridItemSpan(maxLineSpan) }) { SectionHeader("历史") }
            gridItems(history, key = { "history:${it.book.url}" }) { entry ->
                BookCell(
                    entry = entry,
                    onOpen = { openEntry(entry, onOpenDetail, onOpenReader) },
                    onLongPress = { menuEntry = entry },
                )
            }
        }
    }
    menuEntry?.let { entry ->
        EntryMenu(
            entry = entry,
            onDismiss = { menuEntry = null },
            onChanged = { reload++ },
            scope = scope,
        )
    }
}

/** Continue from the stored progress, or open the detail view first. */
private fun openEntry(entry: LibraryEntry, onOpenDetail: () -> Unit, onOpenReader: () -> Unit) {
    val progress = entry.progress
    val chapter = progress?.chapter?.let { key -> entry.chapters.firstOrNull { it.key == key } }
    if (chapter != null) {
        AppNav.bookJson = entry.book.raw.toString()
        AppNav.chapterJson = chapter.raw.toString()
        AppNav.entryJson = entry.raw.toString()
        AppNav.initialPage = progress?.page ?: 0
        AppNav.initialFraction = progress?.fraction ?: 0f
        onOpenReader()
    } else {
        AppNav.bookJson = entry.book.raw.toString()
        onOpenDetail()
    }
}

@Composable
private fun SectionHeader(title: String) {
    Text(
        title,
        style = MaterialTheme.typography.titleSmall,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp),
    )
}

@OptIn(androidx.compose.foundation.ExperimentalFoundationApi::class)
@Composable
private fun BookCell(
    entry: LibraryEntry,
    modifier: Modifier = Modifier,
    onOpen: () -> Unit,
    onLongPress: () -> Unit,
) {
    Column(modifier) {
        Box(
            Modifier
                .fillMaxWidth()
                .aspectRatio(0.72f)
                .clip(RoundedCornerShape(8.dp))
                .background(MaterialTheme.colorScheme.surfaceVariant)
                .combinedClickable(onClick = onOpen, onLongClick = onLongPress),
        ) {
            CoverImage(book = entry.book)
            if (entry.newUnreadCount > 0) {
                Badge(
                    modifier = Modifier.align(Alignment.TopEnd).padding(6.dp),
                    containerColor = MaterialTheme.colorScheme.primary,
                ) { Text("${entry.newUnreadCount}") }
            }
        }
        Text(
            entry.book.title,
            maxLines = 2,
            overflow = TextOverflow.Ellipsis,
            style = MaterialTheme.typography.bodySmall,
            modifier = Modifier.padding(top = 4.dp),
        )
    }
}

@Composable
private fun CoverImage(book: Book) {
    val cover = book.raw.toString()
    val bitmap by produceState<android.graphics.Bitmap?>(initialValue = null, cover) {
        value =
            withContext(Dispatchers.IO) {
                runCatching { ComicRepository.fetchCover(book) }.getOrNull()
            }
    }
    if (bitmap != null) {
        Image(
            bitmap!!.asImageBitmap(),
            book.title,
            Modifier.fillMaxSize(),
            contentScale = ContentScale.Crop,
        )
    } else {
        Box(Modifier.fillMaxSize(), Alignment.Center) {
            Icon(
                painterResource(R.drawable.ic_menu_book),
                null,
                Modifier.size(36.dp),
                tint = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

@Composable
private fun EntryMenu(
    entry: LibraryEntry,
    onDismiss: () -> Unit,
    onChanged: () -> Unit,
    scope: kotlinx.coroutines.CoroutineScope,
) {
    androidx.compose.material3.AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(entry.book.title, maxLines = 2, overflow = TextOverflow.Ellipsis) },
        text = {
            Column {
                TextButton(
                    onClick = {
                        scope.launch {
                            withContext(Dispatchers.IO) {
                                runCatching {
                                    val current = ComicRepository.entryFor(entry.book) ?: entry
                                    ComicRepository.setFavorite(current.book, current.chapters, !current.favorite)
                                }
                            }
                            onDismiss()
                            onChanged()
                        }
                    },
                ) { Text(if (entry.favorite) "取消收藏" else "加入书架") }
                TextButton(
                    onClick = {
                        scope.launch {
                            withContext(Dispatchers.IO) {
                                runCatching {
                                    val detail = ComicRepository.detail(entry.book)
                                    ComicRepository.refreshIntoLibrary(detail)
                                }
                            }
                            onDismiss()
                            onChanged()
                        }
                    },
                ) { Text("刷新章节") }
            }
        },
        confirmButton = {},
        dismissButton = { TextButton(onClick = onDismiss) { Text("关闭") } },
    )
}

@Composable
private fun ComicSearchTab(onOpenDetail: () -> Unit) {
    var keyword by rememberSaveable { mutableStateOf("") }
    var groups by remember { mutableStateOf<List<SearchGroup>?>(null) }
    var searching by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()
    fun search() {
        if (keyword.isBlank() || searching) return
        searching = true
        error = null
        scope.launch {
            val result = runCatching {
                withContext(Dispatchers.IO) { ComicRepository.search(keyword.trim()) }
            }
            result.onSuccess { groups = it }.onFailure { error = it.message }
            searching = false
        }
    }
    Column(Modifier.fillMaxSize()) {
        Row(Modifier.fillMaxWidth().padding(12.dp), verticalAlignment = Alignment.CenterVertically) {
            OutlinedTextField(
                value = keyword,
                onValueChange = { keyword = it },
                modifier = Modifier.weight(1f).testTag("comic-search-field"),
                placeholder = { Text("搜索漫画") },
                singleLine = true,
            )
            IconButton(
                onClick = { search() },
                enabled = !searching,
                modifier = Modifier.testTag("comic-search-button"),
            ) {
                if (searching) {
                    CircularProgressIndicator(Modifier.size(22.dp), strokeWidth = 2.dp)
                } else {
                    Icon(Icons.Filled.Search, "搜索")
                }
            }
        }
        error?.let {
            // Errors keep the core's bilingual text: a source/network failure
            // reads as a source message, while an implementation failure is
            // prefixed "原生错误 / Native panic" by the JNI guard.
            Text(
                it,
                color = MaterialTheme.colorScheme.error,
                style = MaterialTheme.typography.bodySmall,
                modifier = Modifier.padding(horizontal = 16.dp).testTag("comic-search-error"),
            )
        }
        val current = groups
        when {
            current == null -> Box(Modifier.fillMaxSize(), Alignment.Center) {
                Text(
                    "输入关键词开始搜索",
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.testTag("comic-search-hint"),
                )
            }
            else -> LazyColumn(Modifier.fillMaxSize().testTag("comic-search-results")) {
                current.forEach { group ->
                    item {
                        Row(
                            Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Text(
                                group.siteName,
                                style = MaterialTheme.typography.titleSmall,
                                modifier = Modifier.weight(1f),
                            )
                            Text(
                                "${group.books.size} 本",
                                style = MaterialTheme.typography.bodySmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                    }
                    group.error?.let { message ->
                        item {
                            Text(
                                message,
                                color = MaterialTheme.colorScheme.error,
                                style = MaterialTheme.typography.bodySmall,
                                modifier = Modifier.padding(horizontal = 16.dp),
                            )
                        }
                    }
                    items(group.books, key = { it.url }) { book ->
                        Row(
                            Modifier
                                .fillMaxWidth()
                                .testTag("comic-search-result")
                                .clickable {
                                    AppNav.bookJson = book.raw.toString()
                                    onOpenDetail()
                                }
                                .padding(horizontal = 16.dp, vertical = 10.dp),
                        ) {
                            Box(
                                Modifier
                                    .width(64.dp)
                                    .aspectRatio(0.72f)
                                    .clip(RoundedCornerShape(6.dp))
                                    .background(MaterialTheme.colorScheme.surfaceVariant),
                            ) { CoverImage(book = book) }
                            Spacer(Modifier.width(12.dp))
                            Column(Modifier.weight(1f)) {
                                Text(book.title, maxLines = 2, overflow = TextOverflow.Ellipsis)
                                Text(
                                    listOf(book.author, book.status)
                                        .filter { it.isNotEmpty() }
                                        .joinToString(" · "),
                                    style = MaterialTheme.typography.bodySmall,
                                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                                    maxLines = 1,
                                )
                            }
                        }
                    }
                }
            }
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun BookDetailScreen(onBack: () -> Unit, onOpenReader: () -> Unit) {
    val scope = rememberCoroutineScope()
    val bookJson = AppNav.bookJson
    val book = remember(bookJson) { bookJson?.let { Book(JSONObject(it)) } }
    var detail by remember { mutableStateOf<ComicRepository.Detail?>(null) }
    var entry by remember { mutableStateOf<LibraryEntry?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    var reload by remember { mutableIntStateOf(0) }
    val syncRevision by CloudSync.revision.collectAsStateWithLifecycle()
    LaunchedEffect(bookJson, syncRevision) {
        if (book != null) entry = withContext(Dispatchers.IO) { ComicRepository.entryFor(book) }
    }
    LaunchedEffect(bookJson, reload) {
        if (book == null) return@LaunchedEffect
        val result = runCatching {
            withContext(Dispatchers.IO) {
                val loaded = ComicRepository.detail(book)
                // Keep the shelf aware of this book so reading records history.
                ComicRepository.refreshIntoLibrary(loaded)
                Pair(loaded, ComicRepository.entryFor(loaded.book))
            }
        }
        result.onSuccess { (loaded, shelf) ->
            detail = loaded
            entry = shelf
        }.onFailure { error = it.message }
    }
    Scaffold(
        topBar = {
            TopAppBar(
                title = {
                    Text(
                        book?.title ?: "",
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, "返回")
                    }
                },
                actions = {
                    val current = entry
                    if (current != null) {
                        IconButton(onClick = {
                            scope.launch {
                                withContext(Dispatchers.IO) {
                                    runCatching {
                                        ComicRepository.setFavorite(current.book, current.chapters, !current.favorite)
                                    }
                                }
                                reload++
                            }
                        }) {
                            Icon(
                                if (current.favorite) Icons.Filled.Favorite else Icons.Filled.FavoriteBorder,
                                if (current.favorite) "取消收藏" else "收藏",
                                tint =
                                    if (current.favorite) MaterialTheme.colorScheme.primary
                                    else MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                    }
                },
            )
        },
    ) { padding ->
        val current = detail
        when {
            book == null -> {
                Box(Modifier.padding(padding).fillMaxSize(), Alignment.Center) {
                    Text("记录丢失，请返回重进", color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
            }
            current == null -> Box(Modifier.padding(padding).fillMaxSize(), Alignment.Center) {
                Column(horizontalAlignment = Alignment.CenterHorizontally) {
                    CircularProgressIndicator()
                    error?.let {
                        Spacer(Modifier.height(12.dp))
                        Text(
                            it,
                            color = MaterialTheme.colorScheme.error,
                            style = MaterialTheme.typography.bodySmall,
                        )
                        Button(onClick = { reload++ }) { Text("重试") }
                    }
                }
            }
            else -> LazyColumn(Modifier.padding(padding).fillMaxSize()) {
                item {
                    Row(Modifier.padding(16.dp)) {
                        Box(
                            Modifier
                                .width(110.dp)
                                .aspectRatio(0.72f)
                                .clip(RoundedCornerShape(8.dp))
                                .background(MaterialTheme.colorScheme.surfaceVariant),
                        ) { CoverImage(book = current.book) }
                        Spacer(Modifier.width(16.dp))
                        Column(Modifier.weight(1f)) {
                            Text(current.book.title, style = MaterialTheme.typography.titleMedium)
                            Spacer(Modifier.height(4.dp))
                            Text(
                                listOf(current.book.author, current.book.status, current.book.siteName)
                                    .filter { it.isNotEmpty() }
                                    .joinToString(" · "),
                                style = MaterialTheme.typography.bodySmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                            current.book.description.takeIf { it.isNotBlank() }?.let {
                                Spacer(Modifier.height(8.dp))
                                Text(
                                    it,
                                    style = MaterialTheme.typography.bodySmall,
                                    maxLines = 6,
                                    overflow = TextOverflow.Ellipsis,
                                )
                            }
                        }
                    }
                }
                val progressChapter = entry?.progress?.chapter
                if (progressChapter != null) {
                    item {
                        val chapter = current.chapters.firstOrNull { it.key == progressChapter }
                        if (chapter != null) {
                            Button(
                                onClick = {
                                    openChapter(current.book, chapter, entry, onOpenReader)
                                },
                                modifier = Modifier.padding(horizontal = 16.dp),
                            ) {
                                Text(
                                    "继续阅读：${chapter.name}（第 ${(entry?.progress?.page ?: 0) + 1} 页）",
                                )
                            }
                        }
                    }
                }
                item {
                    Text(
                        "章节（${current.chapters.size}）",
                        style = MaterialTheme.typography.titleSmall,
                        modifier = Modifier.padding(16.dp),
                    )
                }
                val sorted = current.chapters.sortedBy { it.order }
                val readKeys = entry?.readKeys ?: emptySet()
                items(sorted, key = { it.key }) { chapter ->
                    val isRead = readKeys.contains(chapter.key)
                    Row(
                        Modifier
                            .fillMaxWidth()
                            .clickable { openChapter(current.book, chapter, entry, onOpenReader) }
                            .padding(horizontal = 16.dp, vertical = 12.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Column(Modifier.weight(1f)) {
                            Text(
                                chapter.name.ifEmpty { chapter.key },
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis,
                                color =
                                    if (isRead) MaterialTheme.colorScheme.onSurfaceVariant
                                    else MaterialTheme.colorScheme.onSurface,
                            )
                            if (chapter.isVolume) {
                                Text(
                                    "卷",
                                    style = MaterialTheme.typography.labelSmall,
                                    color = MaterialTheme.colorScheme.primary,
                                )
                            }
                        }
                        if (progressChapter == chapter.key) {
                            FilterChip(selected = true, onClick = {}, label = { Text("上次") })
                        }
                    }
                }
            }
        }
    }
}

private fun openChapter(
    book: Book,
    chapter: Chapter,
    entry: LibraryEntry?,
    onOpenReader: () -> Unit,
) {
    AppNav.bookJson = book.raw.toString()
    AppNav.chapterJson = chapter.raw.toString()
    AppNav.entryJson = entry?.raw?.toString()
    AppNav.initialPage =
        if (entry?.progress?.chapter == chapter.key) entry.progress?.page ?: 0 else 0
    AppNav.initialFraction = if (entry?.progress?.chapter == chapter.key) entry.progress?.fraction ?: 0f else 0f
    onOpenReader()
}
