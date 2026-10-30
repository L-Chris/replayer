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
import java.nio.ByteBuffer
import java.nio.ByteOrder
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
    @Volatile private var writtenFrames = 0L
    @Volatile var durationUs = 0L
        private set

    /** Playback position in microseconds; seeks reset the AudioTrack head. */
    val positionUs: Long
        get() {
            val audio = track ?: return baseUs
            return baseUs + audio.playbackHeadPosition.toLong() * 1_000_000L / rate
        }

    fun open(path: String) {
        running = true
        handler.post {
            if (closed.get()) return@post
            try {
                handle = Native.nativeMediaOpen(path)
                val info = MediaInfo.parse(org.json.JSONObject(Native.nativeMediaInfo(handle)))
                durationUs = info.durationUs
                listener.onPrepared(info)
                val art = Native.nativeMediaArtwork(handle)?.let { Container(it).bitmap() }
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
        val buffer = maxOf(minBuffer * 2, first.payload.size * 2, 64 * 1024)
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
        writtenFrames = 0
        baseUs = 0
        while (running && !closed.get()) {
            val seek = seekTarget.getAndSet(-1)
            if (seek >= 0) {
                audio.pause()
                audio.flush()
                Native.nativeMediaSeek(handle, seek)
                baseUs = seek
                writtenFrames = 0
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
                // Drain the device buffer before reporting completion.
                while (running && !closed.get() &&
                    audio.playbackHeadPosition.toLong() < writtenFrames
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
        val floats = FloatArray(frame.payload.size / 4)
        ByteBuffer.wrap(frame.payload).order(ByteOrder.LITTLE_ENDIAN).asFloatBuffer().get(floats)
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
            writtenFrames += written
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
