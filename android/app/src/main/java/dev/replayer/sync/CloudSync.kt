package dev.replayer.sync

import android.content.Context
import android.os.Build
import dev.replayer.Native
import dev.replayer.music.MusicRepository
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.withContext
import org.json.JSONObject
import java.util.UUID

data class SyncState(val busy: Boolean = false, val message: String = "未启用", val failed: Boolean = false, val lastSuccess: Long = 0)

/** Foreground scheduling and OS storage; Rust owns the shared merge/transport protocol. */
object CloudSync {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private val operation = Mutex()
    private val mutableState = MutableStateFlow(SyncState())
    val state = mutableState.asStateFlow()
    private val mutableRevision = MutableStateFlow(0L)
    val revision = mutableRevision.asStateFlow()
    // Serialize local music/credential writes with the short apply phase, never with network IO.
    val localLock = Any()
    private var app: Context? = null
    private var foreground = false
    @Volatile private var generation = 0L
    @Volatile private var dirty: Job? = null
    @Volatile private var retryAfter = 0L

    fun initialize(context: Context) {
        if (app != null) return
        app = context.applicationContext
        scope.launch {
            runCatching { withContext(Dispatchers.IO) { MusicRepository.apiKey(context) } }
                .onFailure { mutableState.update { state -> state.copy(message = it.message ?: "凭据读取失败", failed = true) } }
            while (true) {
                delay(300_000)
                if (foreground) synchronize()
            }
        }
    }

    fun foreground(active: Boolean) {
        foreground = active
        if (active) synchronize() else if (dirty?.isActive == true) {
            dirty?.cancel()
            synchronize()
        }
    }

    fun config(context: Context): JSONObject {
        val prefs = context.getSharedPreferences("cloud", Context.MODE_PRIVATE)
        val device = prefs.getString("device_id", null) ?: UUID.randomUUID().toString().also {
            check(prefs.edit().putString("device_id", it).commit())
        }
        val json = JSONObject(prefs.getString("config", "{}")!!)
        return json.put("device_id", device)
            .put("device_name", json.optString("device_name", Build.MODEL))
            .put("password", SecretStore.get(context, "webdav/password"))
            .put("passphrase", SecretStore.get(context, "webdav/passphrase"))
    }

    fun save(context: Context, value: JSONObject) {
        synchronized(localLock) {
            val copy = JSONObject(value.toString())
            if (copy.optBoolean("enabled")) Native.nativeSyncValidate(copy.toString())
            SecretStore.put(context, mapOf("webdav/password" to copy.optString("password"),
                "webdav/passphrase" to copy.optString("passphrase")))
            copy.remove("password")
            copy.remove("passphrase")
            copy.remove("device_id") // This installation's identity cannot be imported from another device.
            check(context.getSharedPreferences("cloud", Context.MODE_PRIVATE).edit().putString("config", copy.toString()).commit())
            generation++
            retryAfter = 0
            mutableState.value = SyncState(message = if (copy.optBoolean("enabled")) "等待同步" else "未启用")
        }
        synchronize()
    }

    fun changed(progressOnly: Boolean = false) {
        mutableRevision.update { it + 1 }
        scope.launch {
            val context = app ?: return@launch
            val enabled = JSONObject(context.getSharedPreferences("cloud", Context.MODE_PRIVATE)
                .getString("config", "{}")!!).optBoolean("enabled")
            if (!enabled) return@launch
            mutableState.update { if (it.busy) it else it.copy(message = "等待同步") }
            if (progressOnly && dirty?.isActive == true) return@launch
            dirty?.cancel()
            dirty = launch { delay(if (progressOnly) 30_000 else 5_000); synchronize() }
        }
    }

    private fun snapshot(context: Context): JSONObject = synchronized(localLock) {
        JSONObject(MusicRepository.prefs(context).getString("cloud_records", "{}")!!)
            .put("music/source", MusicRepository.sourceId(context))
            .put("music/quality", MusicRepository.quality(context))
    }

    private fun secrets(context: Context, account: String): JSONObject =
        JSONObject(SecretStore.get(context, "sync/$account/keys").ifEmpty { "{}" })
            .put("music/chksz", MusicRepository.apiKey(context))

    fun synchronize(manual: Boolean = false, probe: JSONObject? = null) {
        val context = app ?: return
        scope.launch {
            if (!manual && System.currentTimeMillis() < retryAfter) return@launch
            if (!operation.tryLock()) return@launch
            val token = generation
            try {
                val config = withContext(Dispatchers.IO) { probe ?: config(context) }
                if (probe == null && !config.optBoolean("enabled")) return@launch
                dirty?.cancel()
                dirty = null
                mutableState.update { it.copy(busy = true, message = if (probe == null) "正在同步…" else "正在测试连接…", failed = false) }
                withContext(Dispatchers.IO) {
                    // Native returns the account fingerprint without serializing transport credentials.
                    val start = JSONObject().put("config", config).put("probe", probe != null)
                    // Account partitioning uses the same SHA-256 identity as desktop Settings::account.
                    val identity = config.optString("base_url").trim().trimEnd('/') + "\u0000" + config.optString("username").trim()
                    val account = sha256(identity)
                    if (probe == null) synchronized(localLock) {
                        start.put("snapshot", snapshot(context)).put("secrets", secrets(context, account))
                    }
                    val exchange = JSONObject(Native.nativeSyncExchange(start.toString()))
                    if (token != generation) return@withContext
                    if (probe != null) {
                        mutableState.update { it.copy(message = "连接测试通过", failed = false) }
                    } else synchronized(localLock) {
                        if (token != generation) return@synchronized
                        val finish = JSONObject().put("exchange", exchange).put("snapshot", snapshot(context))
                            .put("secrets", secrets(context, account))
                        val result = JSONObject(Native.nativeSyncFinish(finish.toString()))
                        val keys = result.getJSONObject("secrets")
                        // Retain decrypted keys for unsupported providers too, so mobile never publishes empty replacements.
                        SecretStore.put(context, mapOf("sync/$account/keys" to keys.toString(),
                            "music/api_key" to keys.optString("music/chksz")))
                        val data = result.getJSONObject("snapshot")
                        val editor = MusicRepository.prefs(context).edit()
                        if (data.has("music/source")) editor.putString("source_id", data.getString("music/source"))
                        if (data.has("music/quality")) editor.putString("quality", data.getString("music/quality"))
                        data.remove("music/source"); data.remove("music/quality")
                        check(editor.putString("cloud_records", data.toString()).commit())
                        mutableRevision.update { it + 1 }
                        val pending = result.optBoolean("pending") || dirty?.isActive == true
                        mutableState.value = SyncState(busy = true, message = if (pending) "等待同步" else "同步完成",
                            lastSuccess = result.getLong("lastSuccess"))
                        retryAfter = 0
                        if (result.optBoolean("pending")) changed()
                    }
                }
            } catch (error: Exception) {
                if (token == generation) {
                    retryAfter = System.currentTimeMillis() + 300_000
                    mutableState.update { it.copy(message = error.message ?: "同步失败", failed = true) }
                }
            } finally {
                if (token == generation) mutableState.update { it.copy(busy = false) }
                operation.unlock()
                if (token != generation) synchronize()
            }
        }
    }

    fun sha256(value: String): String = java.security.MessageDigest.getInstance("SHA-256")
        .digest(value.toByteArray()).joinToString("") { "%02x".format(it.toInt() and 255) }
}
