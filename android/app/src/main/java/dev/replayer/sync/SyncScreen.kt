package dev.replayer.sync

import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilterChip
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.input.VisualTransformation
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import dev.replayer.Native
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONArray
import org.json.JSONObject

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SyncScreen(onBack: () -> Unit) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val state by CloudSync.state.collectAsStateWithLifecycle()
    // Secrets are deliberately not rememberSaveable / saved-instance-state data.
    var draft by remember { mutableStateOf<JSONObject?>(null) }
    var providers by remember { mutableStateOf(JSONArray()) }
    var error by remember { mutableStateOf<String?>(null) }
    LaunchedEffect(Unit) {
        runCatching { withContext(Dispatchers.IO) { CloudSync.config(context) to JSONArray(Native.nativeSyncProviders()) } }
            .onSuccess { draft = it.first; providers = it.second }.onFailure { error = it.message }
    }
    fun set(key: String, value: Any) { draft = JSONObject(draft!!.toString()).put(key, value) }
    Scaffold(topBar = { TopAppBar(title = { Text("云同步") },
        navigationIcon = { TextButton(onClick = onBack) { Text("返回") } }) }) { padding ->
        Column(Modifier.padding(padding).fillMaxSize().verticalScroll(rememberScrollState()).padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp)) {
            Text("与桌面端共用 WebDAV 同步：漫画收藏与阅读进度、在线歌曲收藏与播放进度、音源和音质设置。媒体文件、缓存及本地文件路径不上传。",
                style = MaterialTheme.typography.bodySmall)
            val current = draft
            if (current != null) {
                Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
                    Text("启用同步")
                    Switch(current.optBoolean("enabled"), { set("enabled", it) }, Modifier.testTag("sync-enabled"))
                }
                Text("提供商")
                Row(Modifier.horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    for (index in 0 until providers.length()) {
                        val provider = providers.getJSONObject(index)
                        FilterChip(selected = current.optString("provider", "Custom") == provider.getString("id"),
                            onClick = { draft = JSONObject(current.toString()).put("provider", provider.getString("id"))
                                .put("base_url", provider.getString("url")) },
                            label = { Text(provider.getString("label")) })
                    }
                }
                for ((key, label, secret) in listOf(
                    Triple("base_url", "WebDAV 地址（HTTPS）", false),
                    Triple("username", "用户名", false),
                    Triple("password", "WebDAV 密码 / 应用密码", true),
                    Triple("device_name", "设备名称", false),
                )) {
                    OutlinedTextField(current.optString(key), { set(key, it) }, label = { Text(label) },
                        singleLine = true, modifier = Modifier.fillMaxWidth().testTag("sync-$key"),
                        visualTransformation = if (secret) PasswordVisualTransformation() else VisualTransformation.None)
                }
                Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
                    Text("同步 API Key")
                    Switch(current.optBoolean("sync_keys"), { set("sync_keys", it) }, Modifier.testTag("sync-keys"))
                }
                if (current.optBoolean("sync_keys")) {
                    OutlinedTextField(current.optString("passphrase"), { set("passphrase", it) },
                        label = { Text("独立同步口令（至少 12 字符）") }, singleLine = true,
                        visualTransformation = PasswordVisualTransformation(), modifier = Modifier.fillMaxWidth())
                    Text("使用与桌面端相同的同步口令加密 API Key。WebDAV 登录密码和同步口令只保存在本机系统密钥库保护的存储中。",
                        style = MaterialTheme.typography.bodySmall)
                }
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    Button(onClick = {
                        scope.launch {
                            runCatching { withContext(Dispatchers.IO) { CloudSync.save(context, current) } }
                                .onSuccess { error = null }.onFailure { error = it.message }
                        }
                    }, modifier = Modifier.testTag("sync-save")) { Text("保存") }
                    TextButton(enabled = !state.busy, onClick = { CloudSync.synchronize(manual = true, probe = current) },
                        modifier = Modifier.testTag("sync-test")) { Text("测试连接") }
                    TextButton(enabled = current.optBoolean("enabled") && !state.busy,
                        onClick = { CloudSync.synchronize(manual = true) }, modifier = Modifier.testTag("sync-now")) { Text("立即同步") }
                }
            }
            Text(error ?: state.message, color = if (error != null || state.failed) MaterialTheme.colorScheme.error
                else MaterialTheme.colorScheme.onSurface, modifier = Modifier.testTag("sync-status"))
            if (state.lastSuccess > 0) {
                Text("上次完成：" + java.text.DateFormat.getDateTimeInstance().format(java.util.Date(state.lastSuccess * 1000)),
                    style = MaterialTheme.typography.bodySmall)
            }
            Spacer(Modifier.height(8.dp))
            Text("启动、回到前台时自动同步；前台每 5 分钟检查，收藏变化后延迟同步，进度每 30 秒合并。退到后台时尝试提交待同步变化。",
                style = MaterialTheme.typography.bodySmall)
        }
    }
}
