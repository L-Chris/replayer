package dev.pordo.ui

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color

// Mint accent matching the desktop app.
val Mint = Color(0xFF7EDBB0)
val MintDim = Color(0xFF4FA980)
val DarkBackground = Color(0xFF0E1113)
val DarkSurface = Color(0xFF161B1E)
val DarkSurfaceVariant = Color(0xFF1F262A)

private val DarkColors = darkColorScheme(
    primary = Mint,
    onPrimary = Color(0xFF04241A),
    secondary = MintDim,
    background = DarkBackground,
    surface = DarkSurface,
    surfaceVariant = DarkSurfaceVariant,
)

private val LightColors = lightColorScheme(
    primary = Color(0xFF1E7A54),
    secondary = Color(0xFF3F8F6D),
)

@Composable
fun PordoTheme(content: @Composable () -> Unit) {
    MaterialTheme(
        colorScheme = if (isSystemInDarkTheme()) DarkColors else LightColors,
        content = content,
    )
}
