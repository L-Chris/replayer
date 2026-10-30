package dev.replayer.music

import android.content.ContentUris
import android.content.Context
import android.content.SharedPreferences
import android.provider.MediaStore
import dev.replayer.Native
import org.json.JSONObject
import java.io.File

data class Quality(val id: String, val name: String)

data class SourceDescriptor(
    val id: String,
    val name: String,
    val qualities: List<Quality>,
    val defaultQuality: String,
)

data class OnlineSong(
    val source: String,
    val id: String,
    val title: String,
    val artist: String,
    val album: String,
    val cover: String?,
) {
    fun toJson(): String = JSONObject()
        .put("source", source)
        .put("id", id)
        .put("title", title)
        .put("artist", artist)
        .put("album", album)
        .put("cover", cover)
        .toString()

    fun toTrack(): Track = Track(
        title = title,
        artist = artist,
        album = album,
        source = source,
        location = "",
        durationUs = 0,
        coverUrl = cover,
        songJson = toJson(),
    )

    companion object {
        fun parse(json: JSONObject) = OnlineSong(
            source = json.getString("source"),
            id = json.getString("id"),
            title = json.optString("title"),
            artist = json.optString("artist"),
            album = json.optString("album"),
            cover = json.optString("cover").ifEmpty { null },
        )
    }
}

sealed interface DownloadPoll {
    data class Running(val bytes: Long) : DownloadPoll
    data class Done(val path: String) : DownloadPoll
    data class Failed(val message: String) : DownloadPoll
}

/**
 * Music discovery, downloads and local collections. The chksz API key lives
 * only in app preferences; native calls receive it per request and never
 * persist it.
 */
object MusicRepository {
    private const val PREFS = "music"
    private val AUDIO_EXTENSIONS = setOf(
        "flac", "mp3", "ogg", "oga", "opus", "m4a", "mp4", "aac", "wav", "wma", "ape", "wv", "aiff",
    )

    fun prefs(context: Context): SharedPreferences =
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)

    fun apiKey(context: Context): String = prefs(context).getString("api_key", "") ?: ""

    fun setApiKey(context: Context, key: String) =
        prefs(context).edit().putString("api_key", key).apply()
    fun sourceId(context: Context): String =
        prefs(context).getString("source_id", "") ?: ""
    fun quality(context: Context): String = prefs(context).getString("quality", "") ?: ""

    fun setSource(context: Context, id: String) =
        prefs(context).edit().putString("source_id", id).apply()

    fun setQuality(context: Context, id: String) =
        prefs(context).edit().putString("quality", id).apply()

    fun sources(context: Context): List<SourceDescriptor> {
        val raw = Native.parse(Native.nativeMusicSources(apiKey(context))) as? org.json.JSONArray
            ?: return emptyList()
        return (0 until raw.length()).map { i ->
            val json = raw.getJSONObject(i)
            val qualities = json.getJSONArray("qualities")
            SourceDescriptor(
                id = json.getString("id"),
                name = json.getString("name"),
                qualities = (0 until qualities.length()).map { q ->
                    val quality = qualities.getJSONObject(q)
                    Quality(quality.getString("id"), quality.getString("name"))
                },
                defaultQuality = json.optString("defaultQuality"),
            )
        }
    }

    data class SearchPage(val songs: List<OnlineSong>, val hasMore: Boolean)

    fun search(context: Context, sourceId: String, keyword: String, page: Int): SearchPage {
        val json = JSONObject(Native.nativeMusicSearch(apiKey(context), sourceId, keyword, page))
        val songs = json.getJSONArray("songs")
        return SearchPage(
            songs = (0 until songs.length()).map { OnlineSong.parse(songs.getJSONObject(it)) },
            hasMore = json.optBoolean("hasMore"),
        )
    }

    fun downloadDirectory(context: Context): File = File(context.filesDir, "music")

    fun startDownload(context: Context, song: OnlineSong, quality: String): Long =
        Native.nativeMusicDownloadStart(
            apiKey(context),
            song.source,
            song.toJson(),
            quality,
            downloadDirectory(context).absolutePath,
        )

    fun pollDownload(handle: Long): DownloadPoll {
        val json = JSONObject(Native.nativeMusicDownloadPoll(handle))
        return when (json.getString("state")) {
            "done" -> DownloadPoll.Done(json.getString("path"))
            "error" -> DownloadPoll.Failed(json.optString("error", "download failed"))
            else -> DownloadPoll.Running(json.optLong("bytes"))
        }
    }

    fun cancelDownload(handle: Long) = Native.nativeMusicDownloadCancel(handle)

    /** Finished downloads: audio files under filesDir/music with sidecars. */
    fun downloadedTracks(context: Context): List<Track> {
        val directory = downloadDirectory(context)
        val files = directory.listFiles()?.sortedByDescending { it.lastModified() } ?: return emptyList()
        return files.mapNotNull { file ->
            if (file.extension.lowercase() !in AUDIO_EXTENSIONS) return@mapNotNull null
            val metadata = sidecarMetadata(file)
            val cover = File(file.absolutePath + ".replayer-cover.png")
            Track(
                title = metadata?.optString("title")?.ifEmpty { null } ?: file.nameWithoutExtension,
                artist = metadata?.optString("artist")?.ifEmpty { null } ?: "",
                album = metadata?.optString("album")?.ifEmpty { null } ?: "",
                source = "download",
                location = file.absolutePath,
                durationUs = 0,
                coverPath = if (cover.exists()) cover.absolutePath else null,
                songJson = metadata?.optJSONObject("online")?.toString(),
            )
        }
    }

    private fun sidecarMetadata(file: File): JSONObject? {
        val sidecar = File(file.absolutePath + ".replayer.json")
        if (!sidecar.exists() || sidecar.length() > 64 * 1024) return null
        return runCatching { JSONObject(sidecar.readText()) }.getOrNull()
    }

    /** Local device music through MediaStore; playback uses content URIs. */
    fun localTracks(context: Context): List<Track> {
        val tracks = mutableListOf<Track>()
        val projection = arrayOf(
            MediaStore.Audio.Media._ID,
            MediaStore.Audio.Media.TITLE,
            MediaStore.Audio.Media.ARTIST,
            MediaStore.Audio.Media.ALBUM,
            MediaStore.Audio.Media.DURATION,
        )
        val selection = "${MediaStore.Audio.Media.IS_MUSIC} != 0"
        context.contentResolver.query(
            MediaStore.Audio.Media.EXTERNAL_CONTENT_URI,
            projection,
            selection,
            null,
            "${MediaStore.Audio.Media.TITLE} COLLATE NOCASE ASC",
        )?.use { cursor ->
            val id = cursor.getColumnIndexOrThrow(MediaStore.Audio.Media._ID)
            val title = cursor.getColumnIndexOrThrow(MediaStore.Audio.Media.TITLE)
            val artist = cursor.getColumnIndexOrThrow(MediaStore.Audio.Media.ARTIST)
            val album = cursor.getColumnIndexOrThrow(MediaStore.Audio.Media.ALBUM)
            val duration = cursor.getColumnIndexOrThrow(MediaStore.Audio.Media.DURATION)
            while (cursor.moveToNext()) {
                tracks += Track(
                    title = cursor.getString(title) ?: "",
                    artist = cursor.getString(artist) ?: "",
                    album = cursor.getString(album) ?: "",
                    source = "local",
                    location = ContentUris.withAppendedId(
                        MediaStore.Audio.Media.EXTERNAL_CONTENT_URI,
                        cursor.getLong(id),
                    ).toString(),
                    durationUs = cursor.getLong(duration) * 1000,
                )
            }
        }
        return tracks
    }

    fun lyrics(path: String): LyricsDoc? {
        val value = Native.parse(Native.nativeMusicLyrics(path)) as? JSONObject ?: return null
        val lines = value.getJSONArray("lines")
        return LyricsDoc(
            synced = value.getBoolean("synced"),
            lines = (0 until lines.length()).map { i ->
                val line = lines.getJSONObject(i)
                LyricLine(line.getDouble("time"), line.getString("text"))
            },
        )
    }
}
