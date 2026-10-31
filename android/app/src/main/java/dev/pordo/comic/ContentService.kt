package dev.pordo.comic

import android.content.Context
import dev.pordo.Native
import dev.pordo.sync.CloudSync
import dev.pordo.sync.SecretStore
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import org.json.JSONObject

data class ServiceView(val sites: List<String> = emptyList(), val busy: Boolean = false,
    val message: String = "配置服务地址和设备 Token", val error: String? = null)

/** Upstream credentials stay on the server; this device stores only its API token. */
object ContentService {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val operation = Mutex()
    private val mutableState = MutableStateFlow(ServiceView())
    val state = mutableState.asStateFlow()
    private fun prefs(context: Context) = context.getSharedPreferences("content-service", Context.MODE_PRIVATE)
    private fun normalized(url: String) = url.trim().trimEnd('/')
    private fun key(url: String) = "content-service/${CloudSync.sha256(normalized(url))}/token"

    fun saved(context: Context): JSONObject {
        val url = prefs(context).getString("url", "").orEmpty()
        return JSONObject().put("url", url).put("token", if (url.isEmpty()) "" else SecretStore.get(context, key(url)))
    }

    fun initialize(context: Context) {
        // A vault/service configuration failure must not disable music or media JNI.
        runCatching { Native.nativeComicServiceConfigure(saved(context).toString()) }
            .onFailure { mutableState.value = ServiceView(error = it.message) }
    }

    /** Save locally before testing, so temporary service outages don't discard configuration. */
    fun save(context: Context, address: String, token: String): JSONObject {
        val old = saved(context)
        val url = normalized(address)
        val secret = token.ifEmpty { if (url == old.optString("url")) old.optString("token") else "" }
        val next = JSONObject().put("url", url).put("token", if (url.isEmpty()) "" else secret)
        Native.nativeComicServiceConfigure(next.toString())
        val changes = linkedMapOf(key(url) to next.getString("token"))
        if (url != old.optString("url")) changes[key(old.optString("url"))] = ""
        val previousSecrets = changes.mapValues { SecretStore.get(context, it.key) }
        try {
            SecretStore.put(context, changes)
            check(prefs(context).edit().putString("url", url).commit()) { "无法保存内容服务设置" }
        } catch (error: Exception) {
            runCatching { SecretStore.put(context, previousSecrets) }
            prefs(context).edit().putString("url", old.optString("url")).commit()
            Native.nativeComicServiceConfigure(old.toString())
            throw error
        }
        return next
    }

    fun saveAndTest(context: Context, url: String, token: String) = scope.launch {
        operation.withLock {
            mutableState.value = ServiceView(busy = true, message = "正在连接…")
            try {
                val saved = save(context, url, token)
                if (saved.getString("url").isEmpty()) {
                    mutableState.value = ServiceView(message = "已断开内容服务")
                } else {
                    val sites = JSONObject(Native.nativeComicServiceTest()).getJSONArray("sites")
                    mutableState.value = ServiceView(sites = (0 until sites.length()).map {
                        sites.getJSONObject(it).getString("siteName")
                    }, message = "连接正常")
                }
            } catch (error: Exception) {
                mutableState.value = ServiceView(error = error.message)
            }
        }
    }
}
