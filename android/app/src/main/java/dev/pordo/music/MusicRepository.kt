package dev.pordo.music

import android.content.ContentUris
import android.content.Context
import android.content.SharedPreferences
import android.provider.MediaStore
import dev.pordo.Native
import dev.pordo.sync.CloudSync
import dev.pordo.sync.SecretStore
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.launch
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
    val key: String get() {
        val platform = when (source) { "chksz-163" -> "netease"; "chksz-qq" -> "qq"; "chksz-kugou" -> "kugou"; else -> source }
        return CloudSync.sha256(platform + "\u0000" + id)
    }

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
            cover = if (json.isNull("cover")) null else json.optString("cover").ifEmpty { null },
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
 * in the OS-keystore-protected vault; native calls receive it per request.
 * Optional encrypted cloud sharing uses the desktop v1 protocol.
 */
object MusicRepository {
    private const val PREFS = "music"
    private val AUDIO_EXTENSIONS = setOf(
        "flac", "mp3", "ogg", "oga", "opus", "m4a", "mp4", "aac", "wav", "wma", "ape", "wv", "aiff",
    )

    fun prefs(context: Context): SharedPreferences =
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)

    fun apiKey(context: Context): String = synchronized(CloudSync.localLock) {
        val saved = SecretStore.get(context, "music/api_key")
        val legacy = prefs(context).getString("api_key", "").orEmpty()
        if (saved.isEmpty() && legacy.isNotEmpty()) {
            SecretStore.put(context, mapOf("music/api_key" to legacy))
            check(prefs(context).edit().remove("api_key").commit())
            legacy
        } else saved
    }

    fun setApiKey(context: Context, key: String) {
        synchronized(CloudSync.localLock) {
            SecretStore.put(context, mapOf("music/api_key" to key))
            check(prefs(context).edit().remove("api_key").commit())
        }
        CloudSync.changed()
    }
    fun sourceId(context: Context): String = prefs(context).getString("source_id", "chksz-163")!!
    fun quality(context: Context): String = prefs(context).getString("quality", "lossless")!!

    fun setSource(context: Context, id: String) {
        synchronized(CloudSync.localLock) { prefs(context).edit().putString("source_id", id).apply() }
        CloudSync.changed()
    }
    fun setQuality(context: Context, id: String) {
        synchronized(CloudSync.localLock) { prefs(context).edit().putString("quality", id).apply() }
        CloudSync.changed()
    }

    private fun records(context: Context): JSONObject = JSONObject(prefs(context).getString("cloud_records", "{}")!!)

    fun isFavorite(context: Context, song: OnlineSong): Boolean = records(context).optBoolean("song/${song.key}/favorite")

    fun setFavorite(context: Context, song: OnlineSong, favorite: Boolean) {
        synchronized(CloudSync.localLock) {
            val data = records(context)
            val prefix = "song/${song.key}"
            data.put("$prefix/song", JSONObject(song.toJson())).put("$prefix/favorite", favorite)
            if (favorite) data.put("$prefix/favorited_at", maxOf(System.currentTimeMillis(), data.optLong("$prefix/favorited_at") + 1))
            check(prefs(context).edit().putString("cloud_records", data.toString()).commit())
        }
        CloudSync.changed()
    }

    fun favorites(context: Context): List<OnlineSong> {
        val data = records(context)
        return data.keys().asSequence().filter { it.endsWith("/song") && data.optBoolean(it.removeSuffix("/song") + "/favorite") }
            .sortedByDescending { data.optLong(it.removeSuffix("/song") + "/favorited_at") }
            .map { OnlineSong.parse(data.getJSONObject(it)) }.toList()
    }

    fun resumePosition(context: Context, track: Track): Long {
        val song = track.songJson?.let { OnlineSong.parse(JSONObject(it)) } ?: return 0
        return (records(context).optJSONObject("song/${song.key}/progress")?.optDouble("position", 0.0)?.times(1_000_000))?.toLong() ?: 0
    }

    private val progressScope = CoroutineScope(SupervisorJob() + Dispatchers.IO.limitedParallelism(1))
    fun savePlayback(context: Context, track: Track?, positionUs: Long, durationUs: Long) {
        val json = track?.songJson ?: return
        val app = context.applicationContext
        val played = System.currentTimeMillis() / 1000
        progressScope.launch {
            runCatching {
                val song = OnlineSong.parse(JSONObject(json))
                synchronized(CloudSync.localLock) {
                    val data = records(app)
                    val prefix = "song/${song.key}"
                    if (!data.has("$prefix/favorite")) return@synchronized
                    data.put("$prefix/progress", JSONObject().put("position", positionUs.coerceAtLeast(0) / 1_000_000.0)
                        .put("duration", durationUs.coerceAtLeast(0) / 1_000_000.0).put("played", played))
                    check(prefs(app).edit().putString("cloud_records", data.toString()).commit())
                }
                CloudSync.changed(progressOnly = true)
            }.onFailure { android.util.Log.e("pordo", "Cannot save playback progress", it) }
        }
    }

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
            val cover = companion(file, "-cover.png")
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

    private fun companion(file: File, suffix: String): File =
        File(file.absolutePath + ".pordo" + suffix).takeIf { it.exists() }
            ?: File(file.absolutePath + ".replayer" + suffix)

    private fun sidecarMetadata(file: File): JSONObject? {
        val sidecar = companion(file, ".json")
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
