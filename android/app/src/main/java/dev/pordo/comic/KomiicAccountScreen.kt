package dev.pordo.comic

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.Button
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
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
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.withContext

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun KomiicAccountScreen(onBack: () -> Unit) {
    val context = LocalContext.current.applicationContext
    val view by KomiicAccount.state.collectAsStateWithLifecycle()
    var email by remember { mutableStateOf("") }
    // Passwords are never put in saved instance state or prefilled in the UI.
    var password by remember { mutableStateOf("") }
    var automatic by remember { mutableStateOf(true) }
    var savedPassword by remember { mutableStateOf(false) }
    var loaded by remember { mutableStateOf(false) }
    var loadError by remember { mutableStateOf<String?>(null) }
    LaunchedEffect(Unit) {
        runCatching { withContext(Dispatchers.IO) { KomiicAccount.saved(context) } }
            .onSuccess {
                email = it.optString("email")
                automatic = if (email.isEmpty()) true else it.optBoolean("auto_login")
                savedPassword = it.optString("password").isNotEmpty()
                loaded = true
            }.onFailure { loadError = it.message }
        while (true) { KomiicAccount.refreshStatus(); delay(1_000) }
    }
    LaunchedEffect(view.busy) {
        if (!view.busy && loaded) {
            savedPassword = withContext(Dispatchers.IO) {
                runCatching { KomiicAccount.saved(context).optString("password").isNotEmpty() }.getOrDefault(false)
            }
        }
    }
    Scaffold(topBar = { TopAppBar(title = { Text("Komiic 账号") }, navigationIcon = {
        IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Filled.ArrowBack, "返回") }
    }) }) { padding ->
        Column(Modifier.padding(padding).verticalScroll(rememberScrollState()).padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp)) {
            OutlinedTextField(email, { email = it }, label = { Text("邮箱") }, singleLine = true,
                enabled = loaded && !view.busy, keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Email),
                modifier = Modifier.fillMaxWidth().testTag("komiic-email"))
            OutlinedTextField(password, { password = it }, label = { Text("密码") }, singleLine = true,
                placeholder = { Text(if (savedPassword) "留空使用已保存的密码" else "输入密码") },
                visualTransformation = PasswordVisualTransformation(),
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password), enabled = loaded && !view.busy,
                modifier = Modifier.fillMaxWidth().testTag("komiic-password"))
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text("自动登录", Modifier.weight(1f))
                Switch(automatic, { automatic = it }, enabled = loaded && !view.busy, modifier = Modifier.testTag("komiic-automatic"))
            }
            Button(onClick = {
                KomiicAccount.saveAndLogin(context, email, password, automatic)
                password = ""
            }, enabled = loaded && !view.busy, modifier = Modifier.testTag("komiic-login")) {
                Text(if (view.busy) "正在处理…" else "保存并登录")
            }
            TextButton(onClick = {
                password = ""; automatic = false
                KomiicAccount.logout(context)
            }, enabled = loaded && !view.busy, modifier = Modifier.testTag("komiic-logout")) { Text("退出并清除密码") }
            Text(loadError ?: view.error ?: view.status.optString("message"), Modifier.testTag("komiic-status"))
            val nickname = view.status.optString("nickname")
            if (nickname.isNotEmpty()) Text(nickname)
            if (!view.status.isNull("limit") && view.status.has("limit")) {
                Text("图片用量：${view.status.optLong("usage")}/${view.status.optLong("limit")}")
            }
            Text("密码通过 Android 系统密钥加密保存在本机，不参与云同步。会话过期时自动刷新；密码错误或需要额外验证时，请修改后重新登录。")
        }
    }
}
