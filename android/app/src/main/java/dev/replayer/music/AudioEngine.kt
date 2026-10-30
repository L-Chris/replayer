package dev.replayer.music

import android.graphics.Bitmap
import android.media.AudioAttributes
import android.media.AudioFormat
import android.media.AudioTrack
import android.os.Handler
import android.os.HandlerThread
import android.os.SystemClock
import dev.replayer.Container
import dev.replayer.Native
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicLong

/**
 * remedia audio pump. Native sessions are thread-pinned, so one dedicated
 * worker thread owns the handle: it opens the session, decodes the first
 * frame to learn rate/channels, then feeds an AudioTrack (float PCM, source
 * sample rate). Seeks are queued to the worker and applied between writes;
 * the native side filters stale generations.
 */
class AudioEngine(private val listener: Listener) {
    interface Listener {
        fun onPrepared(info: MediaInfo) {}
        fun onArtwork(bitmap: Bitmap?) {}
        fun onCompleted() {}
        fun onError(message: String) {}
    }

    private val thread = HandlerThread("remdia-audio").apply { start() }
    private val handler = Handler(thread.looper)
    private val closed = AtomicBoolean(false)
    private val seekTarget = AtomicLong(-1)

    @Volatile private var handle = 0L
    @Volatile private var track: AudioTrack? = null
    @Volatile private var running = false
    @Volatile private var paused = false
    @Volatile private var rate = 48_000
    @Volatile private var channels = 2
    @Volatile private var baseUs = 0L
    @Volatile private var writtenSamples = 0L
    @Volatile var durationUs = 0L
        private set

    /** Playback position in microseconds; seeks reset the AudioTrack head. */
    val positionUs: Long
        get() {
            val audio = track ?: return baseUs
            return baseUs + headFrames(audio) * 1_000_000L / rate
        }

    /**
     * AudioTrack's playback head is an unsigned 32-bit frame count; sign
     * extension would flip it negative after 2^31 frames. Accumulating full
     * 2^32 wraps is unnecessary: seeks flush the track (head, baseUs and
     * writtenSamples reset together) and the EOF drain window is one buffer.
     */
    private fun headFrames(audio: AudioTrack): Long =
        audio.playbackHeadPosition.toLong() and 0xFFFFFFFFL

    fun open(path: String) {
        running = true
        handler.post {
            if (closed.get()) return@post
            try {
                handle = Native.nativeMediaOpen(path)
                val info = MediaInfo.parse(org.json.JSONObject(Native.nativeMediaInfo(handle)))
                durationUs = info.durationUs
                listener.onPrepared(info)
                // Artwork is auxiliary: a decode or protocol failure here must
                // not surface as a whole-track error.
                val art =
                    runCatching {
                        Native.nativeMediaArtwork(handle)?.let { Container(it).bitmap() }
                    }.getOrNull()
                listener.onArtwork(art)
                val first = Native.nativeMediaRead(handle)
                if (first == null) {
                    listener.onCompleted()
                    release()
                    return@post
                }
                pump(Container(first))
            } catch (error: Exception) {
                if (!closed.get()) listener.onError(error.message ?: "playback failed")
                release()
            }
        }
    }

    fun pause() {
        paused = true
        runCatching { track?.pause() }
    }

    fun resume() {
        paused = false
        runCatching { track?.play() }
    }

    fun seekTo(targetUs: Long) {
        seekTarget.set(targetUs)
    }

    /** Stop pumping and release the native session on the worker thread. */
    fun close() {
        if (!closed.compareAndSet(false, true)) return
        running = false
        // Pause first so a WRITE_BLOCKING write on the worker returns promptly
        // instead of draining the whole buffer before release() can run.
        runCatching { track?.pause() }
        handler.post { release() }
    }

    private fun pump(first: Container) {
        val meta = first.meta
        rate = meta.getInt("rate")
        channels = meta.getInt("channels").coerceAtLeast(1)
        val channelMask =
            if (channels == 1) AudioFormat.CHANNEL_OUT_MONO else AudioFormat.CHANNEL_OUT_STEREO
        val minBuffer = AudioTrack.getMinBufferSize(
            rate,
            channelMask,
            AudioFormat.ENCODING_PCM_FLOAT,
        )
        val buffer = maxOf(minBuffer * 2, first.payloadSize * 2, 64 * 1024)
        val audio = AudioTrack.Builder()
            .setAudioAttributes(
                AudioAttributes.Builder()
                    .setUsage(AudioAttributes.USAGE_MEDIA)
                    .setContentType(AudioAttributes.CONTENT_TYPE_MUSIC)
                    .build(),
            )
            .setAudioFormat(
                AudioFormat.Builder()
                    .setEncoding(AudioFormat.ENCODING_PCM_FLOAT)
                    .setSampleRate(rate)
                    .setChannelMask(channelMask)
                    .build(),
            )
            .setBufferSizeInBytes(buffer)
            .setTransferMode(AudioTrack.MODE_STREAM)
            .build()
        track = audio
        audio.play()
        var pending: Container? = first
        writtenSamples = 0
        baseUs = 0
        while (running && !closed.get()) {
            val seek = seekTarget.getAndSet(-1)
            if (seek >= 0) {
                audio.pause()
                audio.flush()
                Native.nativeMediaSeek(handle, seek)
                baseUs = seek
                writtenSamples = 0
                pending = null
                if (!paused) audio.play()
            }
            if (paused) {
                SystemClock.sleep(20)
                continue
            }
            val frame = pending ?: Native.nativeMediaRead(handle)?.let { Container(it) }
            pending = null
            if (frame == null) {
                // Drain the device buffer before reporting completion. The head
                // counts frames while writtenSamples counts floats, i.e. frames
                // times channels.
                while (running && !closed.get() &&
                    headFrames(audio) * channels < writtenSamples
                ) {
                    SystemClock.sleep(20)
                }
                if (running && !closed.get()) listener.onCompleted()
                break
            }
            writeFrame(audio, frame)
        }
    }

    private fun writeFrame(audio: AudioTrack, frame: Container) {
        val floats = FloatArray(frame.payloadSize / 4)
        frame.floatBuffer().get(floats)
        var offset = 0
        while (offset < floats.size && running && !closed.get() && seekTarget.get() < 0) {
            val written = audio.write(
                floats,
                offset,
                floats.size - offset,
                AudioTrack.WRITE_BLOCKING,
            )
            if (written <= 0) break
            offset += written
            // AudioTrack.write(float[]) returns the number of floats written
            // (frames * channels), not frames.
            writtenSamples += written
        }
    }

    private fun release() {
        running = false
        closed.set(true)
        runCatching { track?.let { it.pause(); it.flush(); it.release() } }
        track = null
        if (handle != 0L) runCatching { Native.nativeMediaClose(handle) }
        handle = 0
        thread.quitSafely()
    }
}
