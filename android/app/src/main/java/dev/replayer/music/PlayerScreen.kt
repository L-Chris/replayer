package dev.replayer.music

import androidx.compose.ui.res.painterResource
import dev.replayer.R
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
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
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Slider
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun PlayerScreen(onBack: () -> Unit) {
    val state by Playback.state.collectAsStateWithLifecycle()
    val track = state.track
    Scaffold(
        topBar = {
            TopAppBar(
                title = {
                    Column {
                        Text(
                            track?.title ?: "未在播放",
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                            style = MaterialTheme.typography.titleMedium,
                        )
                        track?.artist?.takeIf { it.isNotEmpty() }?.let {
                            Text(
                                it,
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis,
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
        if (track == null) {
            Box(Modifier.padding(padding).fillMaxSize(), Alignment.Center) {
                Text("从音乐库选择一首歌曲", color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
            return@Scaffold
        }
        Column(Modifier.padding(padding).fillMaxSize()) {
            Box(
                Modifier
                    .fillMaxWidth()
                    .padding(horizontal = 40.dp, vertical = 12.dp)
                    .aspectRatio(1f)
                    .clip(RoundedCornerShape(14.dp))
                    .background(MaterialTheme.colorScheme.surfaceVariant),
                contentAlignment = Alignment.Center,
            ) {
                val artwork = state.artwork
                if (artwork != null) {
                    Image(
                        artwork.asImageBitmap(),
                        track.title,
                        Modifier.fillMaxSize(),
                        contentScale = ContentScale.Crop,
                    )
                } else if (state.status == PlayStatus.Loading) {
                    CircularProgressIndicator()
                } else {
                    Icon(
                        painterResource(R.drawable.ic_music_note),
                        null,
                        Modifier.size(84.dp),
                        tint = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
            state.error?.let {
                Text(
                    it,
                    color = MaterialTheme.colorScheme.error,
                    style = MaterialTheme.typography.bodySmall,
                    modifier = Modifier.padding(horizontal = 24.dp),
                    maxLines = 2,
                )
            }
            PositionSlider(state = state)
            TransportControls(status = state.status)
            LyricsPane(state = state, modifier = Modifier.weight(1f))
        }
    }
}

@Composable
private fun PositionSlider(state: PlaybackState) {
    val duration = state.durationUs.coerceAtLeast(1)
    var scrubUs by remember(state.track) { mutableStateOf<Long?>(null) }
    val shown = scrubUs ?: state.positionUs
    Column(Modifier.fillMaxWidth().padding(horizontal = 20.dp)) {
        Slider(
            value = (shown.toFloat() / duration.toFloat()).coerceIn(0f, 1f),
            onValueChange = { fraction -> scrubUs = (fraction * duration).toLong() },
            onValueChangeFinished = {
                scrubUs?.let { Playback.controller?.seekTo(it) }
                scrubUs = null
            },
        )
        Row(Modifier.fillMaxWidth()) {
            Text(
                formatDuration(shown),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Spacer(Modifier.weight(1f))
            Text(
                formatDuration(state.durationUs),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

@Composable
private fun TransportControls(status: PlayStatus) {
    Row(
        Modifier.fillMaxWidth().padding(vertical = 4.dp),
        horizontalArrangement = Arrangement.Center,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        IconButton(onClick = { Playback.controller?.previous() }, modifier = Modifier.size(56.dp)) {
            Icon(painterResource(R.drawable.ic_skip_previous), "上一首", Modifier.size(34.dp))
        }
        Spacer(Modifier.size(18.dp))
        IconButton(
            onClick = {
                val controller = Playback.controller ?: return@IconButton
                if (status == PlayStatus.Playing) controller.pause() else controller.resume()
            },
            modifier = Modifier
                .size(74.dp)
                .background(MaterialTheme.colorScheme.primary, RoundedCornerShape(37.dp)),
        ) {
            Icon(
                if (status == PlayStatus.Playing) painterResource(R.drawable.ic_pause) else painterResource(R.drawable.ic_play_arrow),
                if (status == PlayStatus.Playing) "暂停" else "播放",
                Modifier.size(40.dp),
                tint = MaterialTheme.colorScheme.onPrimary,
            )
        }
        Spacer(Modifier.size(18.dp))
        IconButton(onClick = { Playback.controller?.next() }, modifier = Modifier.size(56.dp)) {
            Icon(painterResource(R.drawable.ic_skip_next), "下一首", Modifier.size(34.dp))
        }
    }
}

@Composable
private fun LyricsPane(state: PlaybackState, modifier: Modifier = Modifier) {
    val lyrics = state.lyrics
    if (lyrics == null || lyrics.lines.isEmpty()) {
        Box(modifier.fillMaxWidth(), Alignment.Center) {
            Text(
                if (state.track?.source == "local") "本地文件不支持歌词侧车" else "暂无歌词",
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                style = MaterialTheme.typography.bodySmall,
            )
        }
        return
    }
    val positionSeconds = state.positionUs / 1_000_000.0
    val current = lyrics.indexAt(positionSeconds)
    val listState = rememberLazyListState()
    LaunchedEffect(current) {
        if (lyrics.synced && current >= 0) {
            listState.animateScrollToItem(current, -120)
        }
    }
    LazyColumn(
        state = listState,
        modifier = modifier.fillMaxWidth().padding(horizontal = 24.dp),
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        itemsIndexed(lyrics.lines) { index, line ->
            val active = lyrics.synced && index == current
            Text(
                line.text,
                textAlign = TextAlign.Center,
                modifier = Modifier.fillMaxWidth(),
                style =
                    if (active) MaterialTheme.typography.titleMedium
                    else MaterialTheme.typography.bodyMedium,
                color =
                    if (active) MaterialTheme.colorScheme.primary else Color.White.copy(alpha = 0.55f),
            )
        }
        item { Spacer(Modifier.height(48.dp)) }
    }
}
