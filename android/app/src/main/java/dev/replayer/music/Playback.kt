package dev.replayer.music

import android.content.Context
import android.content.Intent
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow

/**
 * Single playback surface shared by the UI and [PlaybackService]. The
 * service publishes state; the UI drives it through [controller] after
 * [ensureService] bound the instance.
 */
object Playback {
    private val mutableState = MutableStateFlow(PlaybackState())
    val state: StateFlow<PlaybackState> = mutableState

    internal fun publish(transform: (PlaybackState) -> PlaybackState) {
        mutableState.value = transform(mutableState.value)
    }

    var controller: Controller? = null

    interface Controller {
        fun playQueue(queue: List<Track>, index: Int)
        fun resume()
        fun pause()
        fun seekTo(positionUs: Long)
        fun next()
        fun previous()
        fun stop()
    }

    fun ensureService(context: Context) {
        if (controller != null) return
        context.startForegroundService(Intent(context, PlaybackService::class.java))
    }

    /** Start playback from any screen; boots the service when needed. */
    fun play(context: Context, queue: List<Track>, index: Int) {
        ensureService(context)
        val service = controller
        if (service == null) {
            // Service not up yet: retry briefly on the main looper.
            android.os.Handler(context.mainLooper).postDelayed(
                { controller?.playQueue(queue, index) },
                150,
            )
        } else {
            service.playQueue(queue, index)
        }
    }
}
