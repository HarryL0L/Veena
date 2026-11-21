package dev.veena.app.ui

import android.content.Context
import android.graphics.Bitmap
import android.util.LruCache
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.tween
import androidx.compose.material3.ColorScheme
import androidx.compose.material3.MaterialExpressiveTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.MotionScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.lerp
import androidx.compose.ui.platform.LocalContext
import coil3.imageLoader
import coil3.request.ImageRequest
import coil3.request.SuccessResult
import coil3.request.allowHardware
import coil3.toBitmap
import com.materialkolor.hct.Hct
import com.materialkolor.quantize.QuantizerCelebi
import com.materialkolor.scheme.DynamicScheme
import com.materialkolor.scheme.SchemeContent
import com.materialkolor.score.Score
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/**
 * Material You colours taken from a song's artwork, the way the system takes
 * them from the wallpaper. Changes between songs fade over half a second.
 * Without artwork, or while it loads, the app's normal colours are used.
 */
@Composable
fun ArtworkTheme(artwork: String?, content: @Composable () -> Unit) {
    val context = LocalContext.current
    val dark = LocalDarkTheme.current
    val base = MaterialTheme.colorScheme
    var from by remember { mutableStateOf(base) }
    var to by remember { mutableStateOf(base) }
    val progress = remember { Animatable(1f) }

    LaunchedEffect(artwork, dark, base) {
        val target = artwork?.let { schemeFor(context, it, dark) } ?: base
        if (target == to) return@LaunchedEffect
        from = lerpScheme(from, to, progress.value)
        to = target
        progress.snapTo(0f)
        progress.animateTo(1f, tween(500))
    }

    MaterialExpressiveTheme(
        colorScheme = lerpScheme(from, to, progress.value),
        motionScheme = MotionScheme.expressive(),
        typography = MaterialTheme.typography,
        shapes = MaterialTheme.shapes,
        content = content,
    )
}

private val cache = LruCache<String, ColorScheme>(64)

/** The colour scheme for one picture; cached, computed off the main thread. */
private suspend fun schemeFor(context: Context, url: String, dark: Boolean): ColorScheme? {
    val key = "$dark:$url"
    cache.get(key)?.let { return it }
    val request = ImageRequest.Builder(context)
        .data(url)
        .size(112)
        .allowHardware(false)
        .build()
    val bitmap = (context.imageLoader.execute(request) as? SuccessResult)?.image?.toBitmap()
        ?: url.takeIf { "/hq720.jpg" in it }?.let { fallback ->
            val retry = ImageRequest.Builder(context).data(fallback.replace("/hq720.jpg", "/hqdefault.jpg")).size(112).allowHardware(false).build()
            (context.imageLoader.execute(retry) as? SuccessResult)?.image?.toBitmap()
        }
        ?: return null
    return withContext(Dispatchers.Default) {
        val seed = seedColor(bitmap) ?: return@withContext null
        SchemeContent(Hct.fromInt(seed), dark, 0.0).toColorScheme()
    }?.also { cache.put(key, it) }
}

/** The colour that best represents the picture, as Material You picks it. */
private fun seedColor(bitmap: Bitmap): Int? {
    val pixels = IntArray(bitmap.width * bitmap.height)
    bitmap.getPixels(pixels, 0, bitmap.width, 0, 0, bitmap.width, bitmap.height)
    val colors = QuantizerCelebi.quantize(pixels, 128)
    return Score.score(colors, desired = 1, fallbackColorArgb = null).firstOrNull()
}

private fun DynamicScheme.toColorScheme(): ColorScheme {
    fun c(argb: Int) = Color(argb)
    return ColorScheme(
        primary = c(primary),
        onPrimary = c(onPrimary),
        primaryContainer = c(primaryContainer),
        onPrimaryContainer = c(onPrimaryContainer),
        inversePrimary = c(inversePrimary),
        secondary = c(secondary),
        onSecondary = c(onSecondary),
        secondaryContainer = c(secondaryContainer),
        onSecondaryContainer = c(onSecondaryContainer),
        tertiary = c(tertiary),
        onTertiary = c(onTertiary),
        tertiaryContainer = c(tertiaryContainer),
        onTertiaryContainer = c(onTertiaryContainer),
        background = c(background),
        onBackground = c(onBackground),
        surface = c(surface),
        onSurface = c(onSurface),
        surfaceVariant = c(surfaceVariant),
        onSurfaceVariant = c(onSurfaceVariant),
        surfaceTint = c(surfaceTint),
        inverseSurface = c(inverseSurface),
        inverseOnSurface = c(inverseOnSurface),
        error = c(error),
        onError = c(onError),
        errorContainer = c(errorContainer),
        onErrorContainer = c(onErrorContainer),
        outline = c(outline),
        outlineVariant = c(outlineVariant),
        scrim = c(scrim),
        surfaceBright = c(surfaceBright),
        surfaceContainer = c(surfaceContainer),
        surfaceContainerHigh = c(surfaceContainerHigh),
        surfaceContainerHighest = c(surfaceContainerHighest),
        surfaceContainerLow = c(surfaceContainerLow),
        surfaceContainerLowest = c(surfaceContainerLowest),
        surfaceDim = c(surfaceDim),
        primaryFixed = c(primaryFixed),
        primaryFixedDim = c(primaryFixedDim),
        onPrimaryFixed = c(onPrimaryFixed),
        onPrimaryFixedVariant = c(onPrimaryFixedVariant),
        secondaryFixed = c(secondaryFixed),
        secondaryFixedDim = c(secondaryFixedDim),
        onSecondaryFixed = c(onSecondaryFixed),
        onSecondaryFixedVariant = c(onSecondaryFixedVariant),
        tertiaryFixed = c(tertiaryFixed),
        tertiaryFixedDim = c(tertiaryFixedDim),
        onTertiaryFixed = c(onTertiaryFixed),
        onTertiaryFixedVariant = c(onTertiaryFixedVariant),
    )
}

/** A scheme part way between two others, for fading between songs. */
private fun lerpScheme(a: ColorScheme, b: ColorScheme, t: Float): ColorScheme {
    if (t >= 1f) return b
    if (t <= 0f) return a
    fun l(x: Color, y: Color) = lerp(x, y, t)
    return b.copy(
        primary = l(a.primary, b.primary),
        onPrimary = l(a.onPrimary, b.onPrimary),
        primaryContainer = l(a.primaryContainer, b.primaryContainer),
        onPrimaryContainer = l(a.onPrimaryContainer, b.onPrimaryContainer),
        secondary = l(a.secondary, b.secondary),
        onSecondary = l(a.onSecondary, b.onSecondary),
        secondaryContainer = l(a.secondaryContainer, b.secondaryContainer),
        onSecondaryContainer = l(a.onSecondaryContainer, b.onSecondaryContainer),
        tertiary = l(a.tertiary, b.tertiary),
        tertiaryContainer = l(a.tertiaryContainer, b.tertiaryContainer),
        background = l(a.background, b.background),
        onBackground = l(a.onBackground, b.onBackground),
        surface = l(a.surface, b.surface),
        onSurface = l(a.onSurface, b.onSurface),
        surfaceVariant = l(a.surfaceVariant, b.surfaceVariant),
        onSurfaceVariant = l(a.onSurfaceVariant, b.onSurfaceVariant),
        outline = l(a.outline, b.outline),
        outlineVariant = l(a.outlineVariant, b.outlineVariant),
        surfaceContainer = l(a.surfaceContainer, b.surfaceContainer),
        surfaceContainerLow = l(a.surfaceContainerLow, b.surfaceContainerLow),
        surfaceContainerHigh = l(a.surfaceContainerHigh, b.surfaceContainerHigh),
        surfaceContainerHighest = l(a.surfaceContainerHighest, b.surfaceContainerHighest),
    )
}
