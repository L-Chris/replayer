package dev.pordo.comic

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalUriHandler
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ContentServiceScreen(onBack: () -> Unit) {
    val context = LocalContext.current.applicationContext
    val uriHandler = LocalUriHandler.current
    val view by ContentService.state.collectAsStateWithLifecycle()
    var url by remember { mutableStateOf("") }
    var token by remember { mutableStateOf("") }
    var hasToken by remember { mutableStateOf(false) }
    var loaded by remember { mutableStateOf(false) }
    var loadError by remember { mutableStateOf<String?>(null) }
    LaunchedEffect(Unit) {
        runCatching { withContext(Dispatchers.IO) { ContentService.saved(context) } }
            .onSuccess { url = it.optString("url"); hasToken = it.optString("token").isNotEmpty(); loaded = true }
            .onFailure { loadError = it.message }
    }
    LaunchedEffect(view.busy) {
        if (!view.busy && loaded) hasToken = withContext(Dispatchers.IO) {
            runCatching { ContentService.saved(context).optString("token").isNotEmpty() }.getOrDefault(false)
        }
    }
    Scaffold(topBar = { TopAppBar(title = { Text("内容服务") }, navigationIcon = {
        IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Filled.ArrowBack, "返回") }
    }) }) { padding ->
        Column(Modifier.padding(padding).verticalScroll(rememberScrollState()).padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp)) {
            OutlinedTextField(url, { url = it }, label = { Text("服务地址") }, singleLine = true,
                enabled = loaded && !view.busy, keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri),
                placeholder = { Text("https://content.example.com") },
                modifier = Modifier.fillMaxWidth().testTag("service-url"))
            OutlinedTextField(token, { token = it }, label = { Text("设备 Token") }, singleLine = true,
                placeholder = { Text(if (hasToken) "留空使用此服务已保存的 Token" else "pt_…") },
                visualTransformation = PasswordVisualTransformation(),
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password), enabled = loaded && !view.busy,
                modifier = Modifier.fillMaxWidth().testTag("service-token"))
            Button(onClick = { ContentService.saveAndTest(context, url, token); token = "" },
                enabled = loaded && !view.busy, modifier = Modifier.testTag("service-save")) {
                Text(if (view.busy) "正在连接…" else "保存并测试连接")
            }
            if (url.startsWith("https://") || url.startsWith("http://")) {
                TextButton(onClick = { uriHandler.openUri(url) }) { Text("打开管理页面") }
            }
            TextButton(onClick = { ContentService.saveAndTest(context, "", ""); url = ""; token = "" },
                enabled = loaded && !view.busy) { Text("断开并清除 Token") }
            Text(loadError ?: view.error ?: view.message, Modifier.testTag("service-status"))
            for (site in view.sites) Text(site)
            Text("源账号和密码请在服务管理页面配置。设备 Token 使用 Android 系统密钥加密保存在本机，不参与云同步。")
        }
    }
}
