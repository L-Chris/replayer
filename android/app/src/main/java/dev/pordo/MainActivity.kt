package dev.pordo

import androidx.compose.ui.res.painterResource
import android.Manifest
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Button
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.ExperimentalComposeUiApi
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.testTagsAsResourceId
import androidx.compose.ui.unit.dp
import androidx.core.content.ContextCompat
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.currentBackStackEntryAsState
import androidx.navigation.compose.rememberNavController
import dev.pordo.comic.BookDetailScreen
import dev.pordo.comic.ComicHome
import dev.pordo.comic.ReaderScreen
import dev.pordo.music.MiniPlayer
import dev.pordo.music.MusicHome
import dev.pordo.music.PlayerScreen
import dev.pordo.ui.PordoTheme

/**
 * Route payloads for the comic screens. Book/chapter JSON is too large for
 * route arguments; the activity is recreation-safe for rotation via
 * android:configChanges, and process death restarts at the bookshelf.
 */
object AppNav {
    var bookJson: String? = null
    var chapterJson: String? = null
    var initialPage: Int = 0
    var initialFraction: Float = 0f
    /** Shelf entry JSON so the reader can persist progress. */
    var entryJson: String? = null
}

class MainActivity : ComponentActivity() {
    private val permissions = registerForActivityResult(
        ActivityResultContracts.RequestMultiplePermissions(),
    ) { }

    @OptIn(ExperimentalComposeUiApi::class)
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        requestRuntimePermissions()
        setContent {
            PordoTheme {
                // Publish test tags as resource ids so adb/UI Automator driven
                // checks (permissions, screenshots, system dialogs) can select
                // the same nodes the instrumentation tests use.
                Box(Modifier.semantics { testTagsAsResourceId = true }) {
                    NativeGate()
                }
            }
        }
    }

    override fun onStart() {
        super.onStart()
        if (Native.ready) dev.pordo.sync.CloudSync.foreground(true)
    }

    override fun onStop() {
        dev.pordo.sync.CloudSync.foreground(false)
        super.onStop()
    }

    private fun requestRuntimePermissions() {
        val wanted = mutableListOf<String>()
        val notification =
            if (Build.VERSION.SDK_INT >= 33) Manifest.permission.POST_NOTIFICATIONS else null
        if (notification != null && !granted(notification)) wanted += notification
        val audio =
            if (Build.VERSION.SDK_INT >= 33) {
                Manifest.permission.READ_MEDIA_AUDIO
            } else {
                Manifest.permission.READ_EXTERNAL_STORAGE
            }
        if (!granted(audio)) wanted += audio
        if (wanted.isNotEmpty()) permissions.launch(wanted.toTypedArray())
    }

    private fun granted(permission: String): Boolean =
        ContextCompat.checkSelfPermission(this, permission) == PackageManager.PERMISSION_GRANTED
}

/**
 * Startup gate: the app is unusable until the native bridge initialized, and a
 * failure must stay diagnosable instead of crashing before the first frame or
 * letting screens issue JNI calls that would abort. Retry re-runs the same
 * initialization, which is safe after a failure.
 */
@Composable
private fun NativeGate() {
    val context = LocalContext.current
    var ready by remember { mutableStateOf(Native.ready) }
    var error by remember { mutableStateOf(Native.initError) }
    if (ready) {
        App()
        return
    }
    Box(Modifier.fillMaxSize().padding(24.dp), Alignment.Center) {
        Column(horizontalAlignment = Alignment.CenterHorizontally) {
            Text(
                "原生库初始化失败 / Native initialization failed",
                style = MaterialTheme.typography.titleMedium,
                modifier = Modifier.testTag("init-error-title"),
            )
            Spacer(Modifier.height(12.dp))
            Text(
                error ?: "未知原因 / unknown cause",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.error,
                modifier = Modifier.testTag("init-error-detail"),
            )
            Spacer(Modifier.height(16.dp))
            Button(
                onClick = {
                    ready = Native.initialize(context)
                    error = Native.initError
                },
                modifier = Modifier.testTag("init-retry"),
            ) { Text("重试 / Retry") }
        }
    }
}

@Composable
private fun App() {
    val nav = rememberNavController()
    val backStack by nav.currentBackStackEntryAsState()
    val route = backStack?.destination?.route
    Scaffold(
        modifier = Modifier.fillMaxSize(),
        bottomBar = {
            if (route == "music" || route == "comics") {
                Column {
                    MiniPlayer(
                        onOpen = { nav.navigate("player") },
                    )
                    NavigationBar {
                        NavigationBarItem(
                            selected = route == "music",
                            onClick = {
                                nav.navigate("music") {
                                    popUpTo(nav.graph.startDestinationId) { saveState = true }
                                    launchSingleTop = true
                                    restoreState = true
                                }
                            },
                            icon = { Icon(painterResource(R.drawable.ic_music_note), null) },
                            label = { Text("音乐") },
                            modifier = Modifier.testTag("nav-music"),
                        )
                        NavigationBarItem(
                            selected = route == "comics",
                            onClick = {
                                nav.navigate("comics") {
                                    popUpTo(nav.graph.startDestinationId) { saveState = true }
                                    launchSingleTop = true
                                    restoreState = true
                                }
                            },
                            icon = { Icon(painterResource(R.drawable.ic_menu_book), null) },
                            label = { Text("漫画") },
                            modifier = Modifier.testTag("nav-comics"),
                        )
                    }
                }
            }
        },
    ) { padding ->
        Box(Modifier.padding(padding)) {
            NavHost(nav, startDestination = "music") {
                composable("music") { MusicHome(onOpenPlayer = { nav.navigate("player") }, onOpenSync = { nav.navigate("sync") }) }
                composable("comics") {
                    ComicHome(
                        onOpenSourceSettings = { nav.navigate("komiic-account") },
                        onOpenSync = { nav.navigate("sync") },
                        onOpenDetail = {
                            nav.navigate("detail")
                        },
                        onOpenReader = { nav.navigate("reader") },
                    )
                }
                composable("sync") { dev.pordo.sync.SyncScreen(onBack = { nav.popBackStack() }) }
                composable("komiic-account") { dev.pordo.comic.KomiicAccountScreen(onBack = { nav.popBackStack() }) }
                composable("player") { PlayerScreen(onBack = { nav.popBackStack() }) }
                composable("detail") {
                    BookDetailScreen(
                        onBack = { nav.popBackStack() },
                        onOpenReader = { nav.navigate("reader") },
                    )
                }
                composable("reader") { ReaderScreen(onBack = { nav.popBackStack() }) }
            }
        }
    }
}
