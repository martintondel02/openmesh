package org.openmesh.android.ui.theme

import android.app.Activity
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.SideEffect
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.platform.LocalView
import androidx.core.view.WindowCompat

private val DarkColors = darkColorScheme(
    primary = MeshPrimary,
    onPrimary = MeshBackground,
    primaryContainer = MeshPrimaryDark,
    onPrimaryContainer = MeshPrimary,
    secondary = MeshSecondary,
    onSecondary = MeshBackground,
    tertiary = MeshTertiary,
    onTertiary = MeshBackground,
    error = MeshError,
    onError = MeshBackground,
    background = MeshBackground,
    onBackground = MeshOnBackground,
    surface = MeshSurface,
    onSurface = MeshOnBackground,
    surfaceVariant = MeshSurfaceHigh,
    onSurfaceVariant = MeshOnSurfaceVariant,
    outline = MeshOutline,
)

private val LightColors = lightColorScheme(
    primary = MeshPrimaryDark,
    onPrimary = MeshBackground,
    secondary = MeshSecondary,
    tertiary = MeshTertiary,
    background = MeshBackground,
    surface = MeshSurface,
    onSurface = MeshOnBackground,
)

@Composable
fun OpenMeshTheme(
    darkTheme: Boolean = isSystemInDarkTheme(),
    content: @Composable () -> Unit,
) {
    val colors = if (darkTheme) DarkColors else LightColors
    val view = LocalView.current
    if (!view.isInEditMode) {
        SideEffect {
            val window = (view.context as? Activity)?.window ?: return@SideEffect
            window.statusBarColor = colors.background.toArgb()
            window.navigationBarColor = colors.background.toArgb()
            WindowCompat.getInsetsController(window, view).isAppearanceLightStatusBars = !darkTheme
        }
    }
    MaterialTheme(colorScheme = colors, typography = OpenMeshTypography, content = content)
}
