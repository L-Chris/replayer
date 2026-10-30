package dev.replayer.comic

import android.content.Context
import dev.replayer.Native
import dev.replayer.sync.CloudSync
import dev.replayer.sync.SecretStore
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import org.json.JSONObject

data class KomiicView(val status: JSONObject = JSONObject().put("phase", "guest").put("message", "游客"),
    val busy: Boolean = false, val error: String? = null)

/** Local credentials only. Session cookies stay inside the shared Rust source. */
object KomiicAccount {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val operation = Mutex()
    private val mutableState = MutableStateFlow(KomiicView())
    val state = mutableState.asStateFlow()
    private fun prefs(context: Context) = context.getSharedPreferences("komiic", Context.MODE_PRIVATE)
    private fun key(email: String) = "komiic/${CloudSync.sha256(email.trim())}/password"

    fun saved(context: Context): JSONObject {
        val prefs = prefs(context)
        val email = prefs.getString("email", "").orEmpty()
        return JSONObject().put("email", email).put("auto_login", prefs.getBoolean("auto_login", false))
            .put("password", if (email.isEmpty()) "" else SecretStore.get(context, key(email)))
    }

    fun initialize(context: Context) {
        // Default anonymous startup must not construct HTTP clients before a
        // test runner or host has installed its proxy/environment settings.
        if (prefs(context).getString("email", "").isNullOrEmpty()) return
        runCatching { JSONObject(Native.nativeKomiicConfigure(saved(context).toString())) }
            .onSuccess { mutableState.value = KomiicView(it) }
            .onFailure { mutableState.value = KomiicView(error = it.message) }
    }

    fun refreshStatus() {
        scope.launch {
            if (operation.tryLock()) try {
                runCatching { JSONObject(Native.nativeKomiicStatus()) }
                    .onSuccess { mutableState.value = mutableState.value.copy(status = it) }
            } finally { operation.unlock() }
        }
    }

    fun saveAndLogin(context: Context, email: String, password: String, automatic: Boolean) = action {
        val old = saved(context)
        val account = email.trim()
        val secret = password.ifEmpty { if (account == old.optString("email")) old.optString("password") else "" }
        require(account.contains('@') && secret.isNotEmpty()) { "请填写 Komiic 邮箱和密码" }
        val next = JSONObject().put("email", account).put("password", secret).put("auto_login", automatic)
        val changes = linkedMapOf(key(account) to secret)
        if (account != old.optString("email") && old.optString("email").isNotEmpty()) {
            changes[key(old.getString("email"))] = ""
        }
        val previousSecrets = changes.mapValues { SecretStore.get(context, it.key) }
        Native.nativeKomiicConfigure(next.toString())
        try {
            SecretStore.put(context, changes)
            check(prefs(context).edit().putString("email", account).putBoolean("auto_login", automatic).commit()) {
                "无法保存 Komiic 配置 / Cannot save Komiic settings"
            }
        } catch (error: Exception) {
            // Keep the applied config and the vault consistent on a failed save.
            runCatching { SecretStore.put(context, previousSecrets) }
            prefs(context).edit().putString("email", old.optString("email"))
                .putBoolean("auto_login", old.optBoolean("auto_login")).commit()
            Native.nativeKomiicConfigure(old.toString())
            throw error
        }
        JSONObject(Native.nativeKomiicLogin())
    }

    fun logout(context: Context) = action {
        val old = saved(context)
        SecretStore.put(context, mapOf(key(old.optString("email")) to ""))
        try { check(prefs(context).edit().putBoolean("auto_login", false).commit()) {
            "无法保存 Komiic 配置 / Cannot save Komiic settings"
        } }
        catch (error: Exception) {
            SecretStore.put(context, mapOf(key(old.optString("email")) to old.optString("password")))
            prefs(context).edit().putBoolean("auto_login", old.optBoolean("auto_login")).commit()
            throw error
        }
        JSONObject(Native.nativeKomiicLogout())
    }

    private fun action(block: () -> JSONObject) = scope.launch {
        operation.withLock {
            mutableState.value = mutableState.value.copy(busy = true, error = null)
            try { mutableState.value = KomiicView(status = block()) }
            catch (error: Exception) {
                val status = runCatching { JSONObject(Native.nativeKomiicStatus()) }.getOrDefault(JSONObject())
                mutableState.value = KomiicView(status, error = error.message)
            }
        }
    }
}
