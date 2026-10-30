package dev.replayer.music

import android.graphics.Bitmap
import org.json.JSONObject

/** One playable track from any origin (local MediaStore, download, online). */
data class Track(
    val title: String,
    val artist: String,
    val album: String,
    val source: String,
    /** File path for downloads, content:// URI for MediaStore tracks. */
    val location: String,
    val durationUs: Long,
    /** Online cover URL, cached through the shared music-cover cache. */
    val coverUrl: String? = null,
    /** `<file>.replayer-cover.png` sidecar of a download, when present. */
    val coverPath: String? = null,
    /** OnlineSong JSON so the UI can offer a download for this track. */
    val songJson: String? = null,
)

/** Parsed `nativeMediaInfo` result. */
data class MediaInfo(
    val kind: String,
    val title: String,
    val artist: String,
    val album: String,
    val durationUs: Long,
    val hasArtwork: Boolean,
) {
    companion object {
        fun parse(json: JSONObject) = MediaInfo(
            kind = json.optString("kind", "music"),
            title = json.optString("title"),
            artist = json.optString("artist"),
            album = json.optString("album"),
            durationUs = json.optLong("durationUs"),
            hasArtwork = json.optBoolean("hasArtwork"),
        )
    }
}

data class LyricLine(val time: Double, val text: String)

/** Parsed `nativeMusicLyrics` result; `synced` mirrors LRC timestamps. */
data class LyricsDoc(val synced: Boolean, val lines: List<LyricLine>) {
    fun indexAt(positionSeconds: Double): Int {
        if (!synced || lines.isEmpty()) return -1
        var index = -1
        for (i in lines.indices) {
            if (lines[i].time <= positionSeconds) index = i else break
        }
        return index
    }
}

enum class PlayStatus { Idle, Loading, Playing, Paused, Completed, Error }

data class PlaybackState(
    val status: PlayStatus = PlayStatus.Idle,
    val track: Track? = null,
    val queue: List<Track> = emptyList(),
    val index: Int = -1,
    val positionUs: Long = 0,
    val durationUs: Long = 0,
    val artwork: Bitmap? = null,
    val lyrics: LyricsDoc? = null,
    val error: String? = null,
)
