package dev.pordo.music

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Intent
import android.content.pm.ServiceInfo
import android.graphics.Bitmap
import android.graphics.drawable.Icon
import android.media.AudioAttributes
import android.media.AudioFocusRequest
import android.media.AudioManager
import android.media.MediaMetadata
import android.media.session.MediaSession
import android.media.session.PlaybackState
import android.os.Build
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import androidx.core.app.ServiceCompat
import dev.pordo.MainActivity
import kotlin.concurrent.thread

/**
 * Foreground media-playback service: owns the AudioEngine, the framework
 * MediaSession and the MediaStyle notification. UI screens never touch the
 * engine directly; they go through the [Playback] singleton.
 */
class PlaybackService : Service(), AudioEngine.Listener, Playback.Controller {
    companion object {
        private const val CHANNEL_ID = "playback"
        private const val NOTIFICATION_ID = 1
        private const val ACTION_TOGGLE = "dev.pordo.TOGGLE"
        private const val ACTION_NEXT = "dev.pordo.NEXT"
        private const val ACTION_PREVIOUS = "dev.pordo.PREVIOUS"
    }

    private var engine: AudioEngine? = null
    private var session: MediaSession? = null
    private var focusRequest: AudioFocusRequest? = null
    private val main = Handler(Looper.getMainLooper())
    private var lastProgressSave = 0L
    private val ticker = object : Runnable {
        override fun run() {
            val current = Playback.state.value
            if (current.status == PlayStatus.Playing) {
                val position = engine?.positionUs ?: current.positionUs
                Playback.publish { it.copy(positionUs = position) }
                updateSessionState()
                if (android.os.SystemClock.elapsedRealtime() - lastProgressSave >= 30_000) saveProgress()
            }
            main.postDelayed(this, 500)
        }
    }

    override fun onCreate() {
        super.onCreate()
        val manager = getSystemService(NotificationManager::class.java)
        val channel = NotificationChannel(
            CHANNEL_ID,
            getString(dev.pordo.R.string.app_name),
            NotificationManager.IMPORTANCE_LOW,
        )
        manager.createNotificationChannel(channel)
        val mediaSession = MediaSession(this, "pordo")
        mediaSession.setCallback(
            object : MediaSession.Callback() {
                override fun onPlay() = resume()
                override fun onPause() = pause()
                override fun onStop() = stop()
                override fun onSeekTo(pos: Long) = seekTo(pos * 1000)
                override fun onSkipToNext() = next()
                override fun onSkipToPrevious() = previous()
            },
        )
        mediaSession.isActive = true
        session = mediaSession
        Playback.controller = this
        main.post(ticker)
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            ACTION_TOGGLE -> if (Playback.state.value.status == PlayStatus.Playing) pause() else resume()
            ACTION_NEXT -> next()
            ACTION_PREVIOUS -> previous()
        }
        // startForegroundService requires prompt foreground entry; show the
        // current (possibly idle) notification, playback upgrades it.
        startInForeground()
        return START_NOT_STICKY
    }

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onDestroy() {
        saveProgress()
        Playback.controller = null
        main.removeCallbacks(ticker)
        engine?.close()
        engine = null
        abandonFocus()
        session?.release()
        session = null
        super.onDestroy()
    }

    // ---- Playback.Controller ----

    override fun playQueue(queue: List<Track>, index: Int) {
        if (index !in queue.indices) return
        Playback.publish {
            it.copy(queue = queue, index = index, error = null)
        }
        openTrack(queue[index])
    }

    override fun resume() {
        if (Playback.state.value.status == PlayStatus.Paused) {
            requestFocus()
            engine?.resume()
            Playback.publish { it.copy(status = PlayStatus.Playing) }
            refreshNotification()
            updateSessionState()
        }
    }

    override fun pause() {
        if (Playback.state.value.status == PlayStatus.Playing) {
            engine?.pause()
            abandonFocus()
            Playback.publish {
                it.copy(status = PlayStatus.Paused, positionUs = engine?.positionUs ?: it.positionUs)
            }
            saveProgress()
            refreshNotification()
            updateSessionState()
        }
    }

    override fun seekTo(positionUs: Long) {
        engine?.seekTo(positionUs)
        Playback.publish { it.copy(positionUs = positionUs) }
        saveProgress()
        updateSessionState()
    }

    override fun next() {
        val state = Playback.state.value
        val target = state.index + 1
        if (target in state.queue.indices) playQueue(state.queue, target)
    }

    override fun previous() {
        val state = Playback.state.value
        // Restart the current track within its first 3 seconds, else go back.
        if (state.positionUs > 3_000_000) seekTo(0)
        else if (state.index - 1 in state.queue.indices) playQueue(state.queue, state.index - 1)
    }

    override fun stop() {
        saveProgress()
        engine?.close()
        engine = null
        abandonFocus()
        Playback.publish {
            it.copy(
                status = PlayStatus.Idle,
                track = null,
                positionUs = 0,
                durationUs = 0,
                artwork = null,
                lyrics = null,
            )
        }
        ServiceCompat.stopForeground(this, ServiceCompat.STOP_FOREGROUND_REMOVE)
        stopSelf()
    }

    // ---- engine lifecycle ----

    private fun openTrack(track: Track) {
        saveProgress()
        engine?.close()
        val audio = AudioEngine(this)
        engine = audio
        Playback.publish {
            it.copy(
                status = PlayStatus.Loading,
                track = track,
                positionUs = 0,
                durationUs = track.durationUs,
                artwork = null,
                lyrics = null,
            )
        }
        requestFocus()
        audio.open(track.location)
        loadLyrics(track)
        startInForeground()
    }

    private fun saveProgress() {
        val state = Playback.state.value
        MusicRepository.savePlayback(this, state.track, state.positionUs, state.durationUs)
        lastProgressSave = android.os.SystemClock.elapsedRealtime()
    }

    private fun loadLyrics(track: Track) {
        if (!track.location.startsWith("/")) return
        thread(name = "lyrics") {
            val lyrics = runCatching {
                val raw = dev.pordo.Native.nativeMusicLyrics(track.location)
                val value = dev.pordo.Native.parse(raw) as? org.json.JSONObject ?: return@runCatching null
                val lines = value.getJSONArray("lines")
                LyricsDoc(
                    synced = value.getBoolean("synced"),
                    lines = (0 until lines.length()).map { i ->
                        val line = lines.getJSONObject(i)
                        LyricLine(line.getDouble("time"), line.getString("text"))
                    },
                )
            }.getOrNull()
            if (Playback.state.value.track == track) {
                Playback.publish { it.copy(lyrics = lyrics) }
            }
        }
    }

    // ---- AudioEngine.Listener (called on the engine worker thread) ----

    override fun onPrepared(info: MediaInfo) {
        main.post {
            val state = Playback.state.value
            val track = state.track ?: return@post
            val title = info.title.ifEmpty { track.title }
            val artist = info.artist.ifEmpty { track.artist }
            val duration = if (info.durationUs > 0) info.durationUs else track.durationUs
            val resume = MusicRepository.resumePosition(this, track).takeIf { it > 0 && it < duration - 3_000_000 } ?: 0
            if (resume > 0) engine?.seekTo(resume)
            Playback.publish {
                it.copy(status = PlayStatus.Playing, durationUs = duration, positionUs = resume)
            }
            session?.setMetadata(
                MediaMetadata.Builder()
                    .putString(MediaMetadata.METADATA_KEY_TITLE, title)
                    .putString(MediaMetadata.METADATA_KEY_ARTIST, artist)
                    .putString(MediaMetadata.METADATA_KEY_ALBUM, info.album.ifEmpty { track.album })
                    .putLong(MediaMetadata.METADATA_KEY_DURATION, duration / 1000)
                    .build(),
            )
            refreshNotification()
            updateSessionState()
        }
    }

    override fun onArtwork(bitmap: Bitmap?) {
        main.post {
            if (bitmap == null) return@post
            Playback.publish { it.copy(artwork = bitmap) }
            val builder = session?.controller?.metadata?.let {
                MediaMetadata.Builder(it)
            } ?: MediaMetadata.Builder()
            session?.setMetadata(
                builder.putBitmap(MediaMetadata.METADATA_KEY_ALBUM_ART, bitmap).build(),
            )
            refreshNotification()
        }
    }

    override fun onCompleted() {
        main.post {
            val state = Playback.state.value
            MusicRepository.savePlayback(this, state.track, state.durationUs, state.durationUs)
            Playback.publish { it.copy(positionUs = it.durationUs) }
            val target = state.index + 1
            if (target in state.queue.indices) {
                playQueue(state.queue, target)
            } else {
                engine?.close()
                engine = null
                abandonFocus()
                Playback.publish { it.copy(status = PlayStatus.Completed, positionUs = it.durationUs) }
                refreshNotification()
                updateSessionState()
            }
        }
    }

    override fun onError(message: String) {
        main.post {
            engine?.close()
            engine = null
            abandonFocus()
            Playback.publish { it.copy(status = PlayStatus.Error, error = message) }
            refreshNotification()
            updateSessionState()
        }
    }

    // ---- audio focus ----

    private fun requestFocus() {
        if (focusRequest != null) return
        val manager = getSystemService(AudioManager::class.java)
        val request = AudioFocusRequest.Builder(AudioManager.AUDIOFOCUS_GAIN)
            .setAudioAttributes(
                AudioAttributes.Builder()
                    .setUsage(AudioAttributes.USAGE_MEDIA)
                    .setContentType(AudioAttributes.CONTENT_TYPE_MUSIC)
                    .build(),
            )
            .setOnAudioFocusChangeListener({ change ->
                when (change) {
                    AudioManager.AUDIOFOCUS_LOSS,
                    AudioManager.AUDIOFOCUS_LOSS_TRANSIENT,
                    -> pause()
                }
            }, main)
            .build()
        focusRequest = request
        manager.requestAudioFocus(request)
    }

    private fun abandonFocus() {
        focusRequest?.let { getSystemService(AudioManager::class.java).abandonAudioFocusRequest(it) }
        focusRequest = null
    }

    // ---- notification + session state ----

    private fun startInForeground() {
        val type =
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PLAYBACK
            } else {
                0
            }
        ServiceCompat.startForeground(this, NOTIFICATION_ID, buildNotification(), type)
    }

    private fun refreshNotification() {
        if (Playback.state.value.status == PlayStatus.Idle) return
        getSystemService(NotificationManager::class.java)
            .notify(NOTIFICATION_ID, buildNotification())
    }

    private fun buildNotification(): Notification {
        val state = Playback.state.value
        val track = state.track
        val playing = state.status == PlayStatus.Playing
        val content = PendingIntent.getActivity(
            this,
            0,
            Intent(this, MainActivity::class.java),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        val builder = Notification.Builder(this, CHANNEL_ID)
            .setSmallIcon(android.R.drawable.ic_media_play)
            .setContentTitle(track?.title ?: getString(dev.pordo.R.string.app_name))
            .setContentText(track?.artist ?: "")
            .setContentIntent(content)
            .setOnlyAlertOnce(true)
            .setStyle(
                Notification.MediaStyle()
                    .setMediaSession(session?.sessionToken)
                    .setShowActionsInCompactView(0, 1, 2),
            )
        state.artwork?.let { builder.setLargeIcon(it) }
        builder.addAction(
            Notification.Action.Builder(
                Icon.createWithResource(this, android.R.drawable.ic_media_previous),
                "Previous",
                actionIntent(ACTION_PREVIOUS),
            ).build(),
        )
        builder.addAction(
            Notification.Action.Builder(
                Icon.createWithResource(
                    this,
                    if (playing) android.R.drawable.ic_media_pause else android.R.drawable.ic_media_play,
                ),
                if (playing) "Pause" else "Play",
                actionIntent(ACTION_TOGGLE),
            ).build(),
        )
        builder.addAction(
            Notification.Action.Builder(
                Icon.createWithResource(this, android.R.drawable.ic_media_next),
                "Next",
                actionIntent(ACTION_NEXT),
            ).build(),
        )
        return builder.build()
    }

    private fun actionIntent(action: String): PendingIntent =
        PendingIntent.getService(
            this,
            action.hashCode(),
            Intent(this, PlaybackService::class.java).setAction(action),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )

    private fun updateSessionState() {
        val state = Playback.state.value
        val engineState =
            when (state.status) {
                PlayStatus.Playing -> PlaybackState.STATE_PLAYING
                PlayStatus.Paused -> PlaybackState.STATE_PAUSED
                PlayStatus.Loading -> PlaybackState.STATE_BUFFERING
                PlayStatus.Error, PlayStatus.Completed -> PlaybackState.STATE_STOPPED
                PlayStatus.Idle -> PlaybackState.STATE_NONE
            }
        session?.setPlaybackState(
            PlaybackState.Builder()
                .setActions(
                    PlaybackState.ACTION_PLAY or
                        PlaybackState.ACTION_PAUSE or
                        PlaybackState.ACTION_PLAY_PAUSE or
                        PlaybackState.ACTION_SEEK_TO or
                        PlaybackState.ACTION_SKIP_TO_NEXT or
                        PlaybackState.ACTION_SKIP_TO_PREVIOUS or
                        PlaybackState.ACTION_STOP,
                )
                .setState(
                    engineState,
                    (engine?.positionUs ?: state.positionUs) / 1000,
                    if (state.status == PlayStatus.Playing) 1.0f else 0.0f,
                )
                .build(),
        )
    }
}
