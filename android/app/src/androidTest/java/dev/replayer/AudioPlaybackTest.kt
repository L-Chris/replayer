package dev.replayer

import android.graphics.Bitmap
import android.graphics.Color
import androidx.test.ext.junit.runners.AndroidJUnit4
import dev.replayer.music.AudioEngine
import dev.replayer.music.MediaInfo
import java.io.File
import java.util.concurrent.CopyOnWriteArrayList
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

/**
 * Offline playback pipeline checks: no network, no bundled fixtures — the
 * audio itself is synthesized so the suite stays deterministic.
 *
 * - Stereo EOF drain: `AudioTrack.write(float[])` returns sample counts while
 *   the playback head counts frames; mixing the units made the completion
 *   drain loop spin forever on stereo streams (`onCompleted` never fired).
 * - Artwork protocol: `nativeMediaArtwork` must emit the same tiles metadata
 *   as comic pages, or `Container.bitmap()` throws and (before isolation)
 *   turned auxiliary artwork into a whole-track error.
 */
@RunWith(AndroidJUnit4::class)
class AudioPlaybackTest {
    private class Recorder : AudioEngine.Listener {
        val prepared = CountDownLatch(1)
        val completed = CountDownLatch(1)
        val artworkArrived = CountDownLatch(1)
        val errors = CopyOnWriteArrayList<String>()

        @Volatile var info: MediaInfo? = null
        @Volatile var artwork: Bitmap? = null

        override fun onPrepared(info: MediaInfo) {
            this.info = info
            prepared.countDown()
        }

        override fun onArtwork(bitmap: Bitmap?) {
            artwork = bitmap
            artworkArrived.countDown()
        }

        override fun onCompleted() = completed.countDown()

        override fun onError(message: String) {
            errors += message
        }
    }

    @Test
    fun stereoPlaybackReachesCompletionAfterDrain() {
        assertTrue("bridge must initialize", Native.initialize(testContext))
        val wav = File(testContext.cacheDir, "stereo-eof-${System.nanoTime()}.wav")
        val engine = Recorder()
        try {
            writeStereoWav(wav, seconds = 0.5)
            val player = AudioEngine(engine)
            try {
                player.open(wav.absolutePath)
                assertTrue("onPrepared within 15s", engine.prepared.await(15, TimeUnit.SECONDS))
                val info = requireNotNull(engine.info)
                // ~0.5s stream; the window catches unit confusion (s vs us).
                assertTrue("durationUs was ${info.durationUs}", info.durationUs in 100_000..1_500_000)
                assertTrue(
                    "onCompleted within 30s (stereo EOF drain); errors=${engine.errors}",
                    engine.completed.await(30, TimeUnit.SECONDS),
                )
                assertTrue("no playback errors: ${engine.errors}", engine.errors.isEmpty())
            } finally {
                player.close()
            }
        } finally {
            wav.delete()
        }
    }

    @Test
    fun sidecarCoverArtworkFlowsThroughTheTilesProtocol() {
        assertTrue("bridge must initialize", Native.initialize(testContext))
        val wav = File(testContext.cacheDir, "stereo-art-${System.nanoTime()}.wav")
        // media.rs reads the sidecar as "<media path>.replayer-cover.png".
        val cover = File(wav.absolutePath + ".replayer-cover.png")
        val engine = Recorder()
        try {
            writeStereoWav(wav, seconds = 0.3)
            writeRedPng(cover, width = 100, height = 60)
            val player = AudioEngine(engine)
            try {
                player.open(wav.absolutePath)
                assertTrue("onPrepared within 15s", engine.prepared.await(15, TimeUnit.SECONDS))
                assertTrue(
                    "artwork callback within 15s; errors=${engine.errors}",
                    engine.artworkArrived.await(15, TimeUnit.SECONDS),
                )
                assertTrue("no playback errors: ${engine.errors}", engine.errors.isEmpty())
                val art = checkNotNull(engine.artwork) { "sidecar artwork must decode" }
                // pixels::thumbnail bounds covers to 600x600 preserving aspect.
                assertTrue("artwork width ${art.width}", art.width in 1..600)
                assertTrue("artwork height ${art.height}", art.height in 1..600)
                assertEquals(Color.RED, art.getPixel(art.width / 2, art.height / 2))
                assertTrue(
                    "completion still fires with artwork",
                    engine.completed.await(30, TimeUnit.SECONDS),
                )
            } finally {
                player.close()
            }
        } finally {
            wav.delete()
            cover.delete()
        }
    }

    @Test
    fun containerBitmapHandlesSingleTileAndStrips() {
        // Single full-cover tile: the fast path artwork and <=2048px pages use.
        val single =
            container(
                4,
                2,
                listOf(Tile(4, 2, rgba(Color.RED, 4 * 2))),
            ).bitmap()
        assertEquals(4, single.width)
        assertEquals(2, single.height)
        assertEquals(Color.RED, single.getPixel(0, 0))
        assertEquals(Color.RED, single.getPixel(3, 1))

        // Two full-width strips: the canvas reassembly path for long images.
        val strips =
            container(
                4,
                4,
                listOf(
                    Tile(4, 2, rgba(Color.GREEN, 4 * 2)),
                    Tile(4, 2, rgba(Color.BLUE, 4 * 2)),
                ),
            ).bitmap()
        assertEquals(4, strips.width)
        assertEquals(4, strips.height)
        assertEquals(Color.GREEN, strips.getPixel(0, 0))
        assertEquals(Color.GREEN, strips.getPixel(3, 1))
        assertEquals(Color.BLUE, strips.getPixel(0, 2))
        assertEquals(Color.BLUE, strips.getPixel(3, 3))
    }

    private class Tile(val width: Int, val height: Int, val pixels: ByteArray)

    private fun rgba(color: Int, pixels: Int): ByteArray {
        val out = ByteArray(pixels * 4)
        for (i in 0 until pixels) {
            out[i * 4] = (color shr 16 and 0xFF).toByte() // R
            out[i * 4 + 1] = (color shr 8 and 0xFF).toByte() // G
            out[i * 4 + 2] = (color and 0xFF).toByte() // B
            out[i * 4 + 3] = (color ushr 24 and 0xFF).toByte() // A
        }
        return out
    }

    /** Wire format of `container()` in the Rust bridge: BE length + JSON + payload. */
    private fun container(width: Int, height: Int, tiles: List<Tile>): Container {
        val payload = ByteArray(tiles.sumOf { it.pixels.size })
        val layout = StringBuilder("[")
        var offset = 0
        for ((index, tile) in tiles.withIndex()) {
            tile.pixels.copyInto(payload, offset)
            if (index > 0) layout.append(',')
            layout.append(
                """{"width":${tile.width},"height":${tile.height},"offset":$offset}""",
            )
            offset += tile.pixels.size
        }
        layout.append(']')
        val meta = """{"width":$width,"height":$height,"tiles":$layout}""".toByteArray()
        val bytes = ByteArray(4 + meta.size + payload.size)
        bytes[0] = (meta.size ushr 24).toByte()
        bytes[1] = (meta.size ushr 16).toByte()
        bytes[2] = (meta.size ushr 8).toByte()
        bytes[3] = meta.size.toByte()
        meta.copyInto(bytes, 4)
        payload.copyInto(bytes, 4 + meta.size)
        return Container(bytes)
    }

    /** Deterministic stereo PCM16 WAV: square tone, 44.1 kHz. */
    private fun writeStereoWav(file: File, seconds: Double, rate: Int = 44_100) {
        val frames = (rate * seconds).toInt()
        val data = ByteArray(frames * 2 * 2) // stereo, 16-bit
        for (i in 0 until frames) {
            val sample: Short = if (i % 64 < 32) 1200 else -1200
            for (channel in 0..1) {
                val at = (i * 2 + channel) * 2
                data[at] = (sample.toInt() and 0xFF).toByte()
                data[at + 1] = (sample.toInt() shr 8 and 0xFF).toByte()
            }
        }
        val header = ByteArray(44)
        fun put(at: Int, value: Int, size: Int) {
            for (i in 0 until size) header[at + i] = (value ushr (8 * i) and 0xFF).toByte()
        }
        "RIFF".toByteArray().copyInto(header, 0)
        put(4, 36 + data.size, 4)
        "WAVEfmt ".toByteArray().copyInto(header, 8)
        put(16, 16, 4) // fmt chunk size
        put(20, 1, 2) // PCM
        put(22, 2, 2) // channels
        put(24, rate, 4)
        put(28, rate * 4, 4) // byte rate (stereo 16-bit)
        put(32, 4, 2) // block align
        put(34, 16, 2) // bits per sample
        "data".toByteArray().copyInto(header, 36)
        put(40, data.size, 4)
        file.outputStream().use {
            it.write(header)
            it.write(data)
        }
    }

    private fun writeRedPng(file: File, width: Int, height: Int) {
        val bitmap = Bitmap.createBitmap(width, height, Bitmap.Config.ARGB_8888)
        bitmap.eraseColor(Color.RED)
        file.outputStream().use { bitmap.compress(Bitmap.CompressFormat.PNG, 100, it) }
        bitmap.recycle()
    }
}
