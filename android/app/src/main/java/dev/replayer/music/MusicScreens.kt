package dev.replayer.music

import android.content.Context
import android.graphics.BitmapFactory
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Download
import androidx.compose.material.icons.filled.MusicNote
import androidx.compose.material.icons.filled.Pause
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material.icons.filled.Search
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material.icons.filled.SkipNext
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
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
import androidx.compose.runtime.collectAsState
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
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import dev.replayer.Container
import dev.replayer.Native
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/** In-flight and finished download jobs, shared by the online and download tabs. */
object DownloadManager {
    sealed interface Status {
        data class Running(val bytes: Long) : Status
        data class Done(val path: String) : Status
        data class Failed(val message: String) : Status
    }

    data class Item(val handle: Long, val song: OnlineSong, val quality: String, val status: Status)

    private val scope = kotlinx.coroutines.CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val _items = MutableStateFlow<List<Item>>(emptyList())
    val items: StateFlow<List<Item>> = _items

    fun start(context: Context, song: OnlineSong, quality: String) {
        val app = context.applicationContext
        scope.launch {
            val handle = runCatching { MusicRepository.startDownload(app, song, quality) }
                .getOrElse { return@launch }
            _items.update { listOf(Item(handle, song, quality, Status.Running(0))) + it }
            var active = true
            while (active) {
                delay(400)
                val poll = runCatching { MusicRepository.pollDownload(handle) }
                    .getOrElse { error ->
                        setStatus(handle, Status.Failed(error.message ?: "download failed"))
                        null
                    }
                when (poll) {
                    null -> active = false
                    is DownloadPoll.Running -> setStatus(handle, Status.Running(poll.bytes))
                    is DownloadPoll.Done -> {
                        setStatus(handle, Status.Done(poll.path))
                        active = false
                    }
                    is DownloadPoll.Failed -> {
                        setStatus(handle, Status.Failed(poll.message))
                        active = false
                    }
                }
            }
        }
    }

    fun cancel(handle: Long) {
        runCatching { MusicRepository.cancelDownload(handle) }
        _items.update { items -> items.filterNot { it.handle == handle } }
    }

    fun clearFinished() {
        _items.update { items -> items.filter { it.status is Status.Running } }
    }

    private fun setStatus(handle: Long, status: Status) {
        _items.update { items ->
            items.map { if (it.handle == handle) it.copy(status = status) else it }
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun MusicHome(onOpenPlayer: () -> Unit) {
    var tab by rememberSaveable { mutableIntStateOf(0) }
    var showSettings by rememberSaveable { mutableStateOf(false) }
    val context = LocalContext.current
    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("音乐") },
                actions = {
                    IconButton(onClick = { showSettings = true }) {
                        Icon(Icons.Filled.Settings, "设置")
                    }
                },
            )
        },
    ) { padding ->
        Column(Modifier.padding(padding).fillMaxSize()) {
            TabRow(selectedTabIndex = tab) {
                Tab(selected = tab == 0, onClick = { tab = 0 }, text = { Text("音乐库") })
                Tab(selected = tab == 1, onClick = { tab = 1 }, text = { Text("在线") })
                Tab(selected = tab == 2, onClick = { tab = 2 }, text = { Text("下载") })
            }
            when (tab) {
                0 -> LibraryTab(onOpenPlayer)
                1 -> OnlineTab(onOpenSettings = { showSettings = true })
                2 -> DownloadsTab(onOpenPlayer)
            }
        }
    }
    if (showSettings) {
        MusicSettingsDialog(onDismiss = { showSettings = false })
    }
}

@Composable
private fun LibraryTab(onOpenPlayer: () -> Unit) {
    val context = LocalContext.current
    var tracks by remember { mutableStateOf<List<Track>>(emptyList()) }
    var loading by remember { mutableStateOf(true) }
    var error by remember { mutableStateOf<String?>(null) }
    var reload by remember { mutableIntStateOf(0) }
    val playback by Playback.state.collectAsStateWithLifecycle()
    LaunchedEffect(reload) {
        loading = true
        error = null
        val result = runCatching {
            withContext(Dispatchers.IO) {
                val downloaded = MusicRepository.downloadedTracks(context)
                val local =
                    runCatching { MusicRepository.localTracks(context) }.getOrElse { emptyList() }
                downloaded + local
            }
        }
        tracks = result.getOrElse { emptyList() }
        error = result.exceptionOrNull()?.message
        loading = false
    }
    Column(Modifier.fillMaxSize()) {
        Row(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 4.dp)) {
            Text(
                "已下载 ${tracks.count { it.source == "download" }} · 本地 ${tracks.count { it.source == "local" }}",
                style = MaterialTheme.typography.bodySmall,
                modifier = Modifier.weight(1f),
            )
            IconButton(onClick = { reload++ }) { Icon(Icons.Filled.Refresh, "刷新") }
        }
        when {
            loading -> Box(Modifier.fillMaxSize(), Alignment.Center) { CircularProgressIndicator() }
            error != null -> Text(
                error ?: "",
                color = MaterialTheme.colorScheme.error,
                modifier = Modifier.padding(16.dp),
            )
            tracks.isEmpty() -> Text(
                "还没有音乐。在“在线”页签搜索并下载，或授予权限读取本地音乐。",
                modifier = Modifier.padding(24.dp),
                style = MaterialTheme.typography.bodyMedium,
            )
            else -> LazyColumn(Modifier.fillMaxSize()) {
                itemsIndexed(tracks, key = { index, track -> "${track.location}#$index" }) { index, track ->
                    TrackRow(
                        track = track,
                        active = playback.track?.location == track.location,
                        onClick = {
                            Playback.play(context, tracks, index)
                            onOpenPlayer()
                        },
                    )
                }
            }
        }
    }
}

@Composable
private fun TrackRow(track: Track, active: Boolean, onClick: () -> Unit) {
    Row(
        Modifier
            .fillMaxWidth()
            .clickable(onClick = onClick)
            .background(
                if (active) {
                    MaterialTheme.colorScheme.primary.copy(alpha = 0.12f)
                } else {
                    MaterialTheme.colorScheme.surface
                },
            )
            .padding(horizontal = 16.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        TrackCover(track = track, size = 44)
        Spacer(Modifier.width(12.dp))
        Column(Modifier.weight(1f)) {
            Text(
                track.title.ifEmpty { track.location.substringAfterLast('/') },
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                style = MaterialTheme.typography.bodyLarge,
                color =
                    if (active) MaterialTheme.colorScheme.primary
                    else MaterialTheme.colorScheme.onSurface,
            )
            val subtitle = listOf(track.artist, track.album).filter { it.isNotEmpty() }.joinToString(" · ")
            if (subtitle.isNotEmpty()) {
                Text(
                    subtitle,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
        if (track.durationUs > 0) {
            Text(
                formatDuration(track.durationUs),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

@Composable
private fun TrackCover(track: Track, size: Int) {
    val coverPath = track.coverPath
    val bitmap by produceState<android.graphics.Bitmap?>(initialValue = null, coverPath) {
        value =
            if (coverPath == null) {
                null
            } else {
                withContext(Dispatchers.IO) { runCatching { BitmapFactory.decodeFile(coverPath) }.getOrNull() }
            }
    }
    Box(
        Modifier.size(size.dp).clip(RoundedCornerShape(6.dp))
            .background(MaterialTheme.colorScheme.surfaceVariant),
        contentAlignment = Alignment.Center,
    ) {
        if (bitmap != null) {
            Image(
                bitmap!!.asImageBitmap(),
                null,
                Modifier.fillMaxSize(),
                contentScale = ContentScale.Crop,
            )
        } else {
            Icon(
                Icons.Filled.MusicNote,
                null,
                Modifier.size((size / 2).dp),
                tint = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun OnlineTab(onOpenSettings: () -> Unit) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var keyword by rememberSaveable { mutableStateOf("") }
    var sourceId by rememberSaveable { mutableStateOf("") }
    var descriptors by remember { mutableStateOf<List<SourceDescriptor>>(emptyList()) }
    var keyError by remember { mutableStateOf<String?>(null) }
    var songs by remember { mutableStateOf<List<OnlineSong>>(emptyList()) }
    var hasMore by remember { mutableStateOf(false) }
    var page by remember { mutableIntStateOf(0) }
    var searching by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }

    val reloadSources: () -> Unit = {
        scope.launch {
            val result = runCatching {
                withContext(Dispatchers.IO) { MusicRepository.sources(context) }
            }
            descriptors = result.getOrElse { emptyList() }
            keyError = result.exceptionOrNull()?.message
            val preferred = MusicRepository.sourceId(context)
            if (sourceId.isEmpty() || descriptors.none { it.id == sourceId }) {
                sourceId = preferred.ifEmpty { descriptors.firstOrNull()?.id ?: "" }
            }
        }
    }
    LaunchedEffect(Unit) { reloadSources() }

    val descriptor = descriptors.firstOrNull { it.id == sourceId }
    val quality = MusicRepository.quality(context).ifEmpty { descriptor?.defaultQuality ?: "lossless" }

    fun search(reset: Boolean) {
        if (keyword.isBlank() || sourceId.isEmpty() || searching) return
        searching = true
        error = null
        val target = if (reset) 0 else page
        scope.launch {
            val result = runCatching {
                withContext(Dispatchers.IO) {
                    MusicRepository.search(context, sourceId, keyword.trim(), target)
                }
            }
            result.onSuccess { found ->
                songs = if (reset) found.songs else songs + found.songs
                hasMore = found.hasMore
                page = target + 1
            }.onFailure { error = it.message }
            searching = false
        }
    }

    Column(Modifier.fillMaxSize()) {
        if (descriptors.isEmpty()) {
            Surface(
                Modifier.fillMaxWidth().padding(16.dp),
                color = MaterialTheme.colorScheme.surfaceVariant,
                shape = RoundedCornerShape(10.dp),
            ) {
                Column(Modifier.padding(16.dp)) {
                    Text(
                        keyError ?: "在线音乐需要 chksz API 密钥。",
                        style = MaterialTheme.typography.bodyMedium,
                    )
                    Spacer(Modifier.height(8.dp))
                    Button(onClick = onOpenSettings) { Text("设置密钥") }
                }
            }
        } else {
            Row(
                Modifier.fillMaxWidth().padding(horizontal = 12.dp),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                descriptors.forEach { item ->
                    FilterChip(
                        selected = item.id == sourceId,
                        onClick = { sourceId = item.id },
                        label = { Text(item.name) },
                    )
                }
            }
        }
        Row(Modifier.fillMaxWidth().padding(12.dp), verticalAlignment = Alignment.CenterVertically) {
            OutlinedTextField(
                value = keyword,
                onValueChange = { keyword = it },
                modifier = Modifier.weight(1f),
                placeholder = { Text("歌曲、歌手或专辑") },
                singleLine = true,
            )
            IconButton(onClick = { search(reset = true) }, enabled = !searching) {
                if (searching) {
                    CircularProgressIndicator(Modifier.size(22.dp), strokeWidth = 2.dp)
                } else {
                    Icon(Icons.Filled.Search, "搜索")
                }
            }
        }
        error?.let {
            Text(
                it,
                color = MaterialTheme.colorScheme.error,
                modifier = Modifier.padding(horizontal = 16.dp),
                style = MaterialTheme.typography.bodySmall,
            )
        }
        LazyColumn(Modifier.fillMaxSize()) {
            items(songs, key = { "${it.source}:${it.id}" }) { song ->
                OnlineSongRow(song = song, quality = quality)
            }
            if (hasMore) {
                item {
                    TextButton(
                        onClick = { search(reset = false) },
                        modifier = Modifier.fillMaxWidth().padding(8.dp),
                    ) { Text(if (searching) "加载中…" else "加载更多") }
                }
            }
        }
    }
}

@Composable
private fun OnlineSongRow(song: OnlineSong, quality: String) {
    val context = LocalContext.current
    val download by DownloadManager.items.collectAsState()
    val active = download.firstOrNull { it.song.source == song.source && it.song.id == song.id }
    Row(
        Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        OnlineCover(url = song.cover, size = 44)
        Spacer(Modifier.width(12.dp))
        Column(Modifier.weight(1f)) {
            Text(song.title, maxLines = 1, overflow = TextOverflow.Ellipsis)
            Text(
                listOf(song.artist, song.album).filter { it.isNotEmpty() }.joinToString(" · "),
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        when (val status = active?.status) {
            is DownloadManager.Status.Running -> CircularProgressIndicator(
                Modifier.size(22.dp),
                strokeWidth = 2.dp,
            )
            is DownloadManager.Status.Done -> Icon(
                Icons.Filled.Download,
                "已下载",
                tint = MaterialTheme.colorScheme.primary,
            )
            is DownloadManager.Status.Failed -> Text(
                "重试",
                color = MaterialTheme.colorScheme.error,
                modifier = Modifier.clickable { DownloadManager.start(context, song, quality) },
            )
            null -> IconButton(onClick = { DownloadManager.start(context, song, quality) }) {
                Icon(Icons.Filled.Download, "下载")
            }
        }
    }
}

@Composable
private fun OnlineCover(url: String?, size: Int) {
    val bitmap by produceState<android.graphics.Bitmap?>(initialValue = null, url) {
        value =
            if (url.isNullOrEmpty()) {
                null
            } else {
                withContext(Dispatchers.IO) {
                    runCatching { Container(Native.nativeMusicCover(url, size * 3)).bitmap() }
                        .getOrNull()
                }
            }
    }
    Box(
        Modifier.size(size.dp).clip(RoundedCornerShape(6.dp))
            .background(MaterialTheme.colorScheme.surfaceVariant),
        contentAlignment = Alignment.Center,
    ) {
        if (bitmap != null) {
            Image(
                bitmap!!.asImageBitmap(),
                null,
                Modifier.fillMaxSize(),
                contentScale = ContentScale.Crop,
            )
        } else {
            Icon(
                Icons.Filled.MusicNote,
                null,
                Modifier.size((size / 2).dp),
                tint = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

@Composable
private fun DownloadsTab(onOpenPlayer: () -> Unit) {
    val context = LocalContext.current
    val items by DownloadManager.items.collectAsStateWithLifecycle()
    var finished by remember { mutableStateOf<List<Track>>(emptyList()) }
    var loading by remember { mutableStateOf(true) }
    val runningCount = items.count { it.status is DownloadManager.Status.Running }
    LaunchedEffect(runningCount, items.size) {
        loading = true
        finished = withContext(Dispatchers.IO) { MusicRepository.downloadedTracks(context) }
        loading = false
    }
    LazyColumn(Modifier.fillMaxSize()) {
        items(items, key = { it.handle }) { item ->
            Column(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp)) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Column(Modifier.weight(1f)) {
                        Text(item.song.title, maxLines = 1, overflow = TextOverflow.Ellipsis)
                        Text(
                            when (val status = item.status) {
                                is DownloadManager.Status.Running ->
                                    "下载中 ${formatBytes(status.bytes)} · ${item.quality}"
                                is DownloadManager.Status.Done -> "已完成 · ${item.quality}"
                                is DownloadManager.Status.Failed -> status.message
                            },
                            style = MaterialTheme.typography.bodySmall,
                            color =
                                if (item.status is DownloadManager.Status.Failed) {
                                    MaterialTheme.colorScheme.error
                                } else {
                                    MaterialTheme.colorScheme.onSurfaceVariant
                                },
                            maxLines = 2,
                        )
                    }
                    if (item.status is DownloadManager.Status.Running) {
                        IconButton(onClick = { DownloadManager.cancel(item.handle) }) {
                            Icon(Icons.Filled.Close, "取消")
                        }
                    }
                }
                if (item.status is DownloadManager.Status.Running) {
                    LinearProgressIndicator(Modifier.fillMaxWidth())
                }
            }
        }
        item {
            Row(
                Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    "已下载（${finished.size}）",
                    style = MaterialTheme.typography.titleSmall,
                    modifier = Modifier.weight(1f),
                )
                if (items.any { it.status !is DownloadManager.Status.Running }) {
                    TextButton(onClick = { DownloadManager.clearFinished() }) { Text("清理记录") }
                }
            }
        }
        if (loading) {
            item {
                Box(Modifier.fillMaxWidth().padding(24.dp), Alignment.Center) {
                    CircularProgressIndicator()
                }
            }
        }
        items(finished, key = { it.location }) { track ->
            TrackRow(
                track = track,
                active = false,
                onClick = { Playback.play(context, finished, finished.indexOf(track)) ; onOpenPlayer() },
            )
        }
    }
}

@Composable
private fun MusicSettingsDialog(onDismiss: () -> Unit) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var key by remember { mutableStateOf(MusicRepository.apiKey(context)) }
    var saved by remember { mutableStateOf(false) }
    androidx.compose.material3.AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("在线音乐设置") },
        text = {
            Column {
                OutlinedTextField(
                    value = key,
                    onValueChange = { key = it; saved = false },
                    label = { Text("chksz API 密钥") },
                    singleLine = true,
                )
                Text(
                    "密钥仅保存在本机，用于 chksz 在线音源的搜索与下载。",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(top = 8.dp),
                )
                if (saved) {
                    Text(
                        "已保存",
                        color = MaterialTheme.colorScheme.primary,
                        style = MaterialTheme.typography.bodySmall,
                        modifier = Modifier.padding(top = 8.dp),
                    )
                }
            }
        },
        confirmButton = {
            TextButton(onClick = {
                scope.launch {
                    withContext(Dispatchers.IO) { MusicRepository.setApiKey(context, key.trim()) }
                    saved = true
                }
            }) { Text("保存") }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text("关闭") } },
    )
}

/** Compact now-playing bar shown above the bottom navigation. */
@Composable
fun MiniPlayer(onOpen: () -> Unit) {
    val state by Playback.state.collectAsStateWithLifecycle()
    val track = state.track ?: return
    if (state.status == PlayStatus.Idle) return
    Surface(
        Modifier.fillMaxWidth(),
        color = MaterialTheme.colorScheme.surfaceVariant,
        tonalElevation = 3.dp,
    ) {
        Row(
            Modifier.clickable(onClick = onOpen).padding(horizontal = 12.dp, vertical = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Box(
                Modifier.size(36.dp).clip(RoundedCornerShape(6.dp))
                    .background(MaterialTheme.colorScheme.surface),
                contentAlignment = Alignment.Center,
            ) {
                val artwork = state.artwork
                if (artwork != null) {
                    Image(
                        artwork.asImageBitmap(),
                        null,
                        Modifier.fillMaxSize(),
                        contentScale = ContentScale.Crop,
                    )
                } else {
                    Icon(Icons.Filled.MusicNote, null, Modifier.size(20.dp))
                }
            }
            Spacer(Modifier.width(10.dp))
            Column(Modifier.weight(1f)) {
                Text(track.title, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Text(
                    formatDuration(state.positionUs) + " / " + formatDuration(state.durationUs),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            IconButton(onClick = {
                val controller = Playback.controller ?: return@IconButton
                if (state.status == PlayStatus.Playing) controller.pause() else controller.resume()
            }) {
                Icon(
                    if (state.status == PlayStatus.Playing) Icons.Filled.Pause else Icons.Filled.PlayArrow,
                    if (state.status == PlayStatus.Playing) "暂停" else "播放",
                )
            }
            IconButton(onClick = { Playback.controller?.next() }) {
                Icon(Icons.Filled.SkipNext, "下一首")
            }
        }
    }
}

fun formatDuration(us: Long): String {
    val total = (us / 1000).coerceAtLeast(0)
    val minutes = total / 60
    val seconds = total % 60
    return "%d:%02d".format(minutes, seconds)
}

fun formatBytes(bytes: Long): String =
    when {
        bytes >= 1L shl 20 -> "%.1f MB".format(bytes / 1048576.0)
        bytes >= 1L shl 10 -> "%.0f KB".format(bytes / 1024.0)
        else -> "$bytes B"
    }
