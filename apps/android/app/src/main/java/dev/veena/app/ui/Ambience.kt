package dev.veena.app.ui

import android.content.Context
import android.os.PowerManager
import androidx.compose.foundation.Canvas
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.State
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.BlendMode
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.CompositingStrategy
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.PathOperation
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.rotate
import androidx.compose.ui.graphics.drawscope.withTransform
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.lerp
import androidx.compose.material3.MaterialTheme
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import dev.veena.app.DayPart
import dev.veena.app.Season
import dev.veena.app.SettingsState
import java.time.LocalDate
import java.time.LocalDateTime
import java.time.Month
import java.time.temporal.ChronoUnit
import kotlin.math.PI
import kotlin.math.abs
import kotlin.math.cos
import kotlin.math.floor
import kotlin.math.sin
import kotlin.random.Random
import kotlinx.coroutines.delay

/** Days that get their own decorations. */
enum class Occasion(val greeting: String, val line: String) {
    DIWALI("Happy Diwali", "Lamps lit, sweets shared, music on."),
    NEW_YEAR("Happy New Year", "A fresh year of songs ahead."),
    VALENTINE("Happy Valentine's Day", "Play something for someone."),
}

/** What the living background shows: the hour (0–24), the season and any special day. */
data class Ambience(
    val hour: Float,
    val season: Season,
    val occasion: Occasion?,
    val date: LocalDate,
) {
    val dayPart: DayPart get() = dayPartOf(hour)

    val greeting: String
        get() = occasion?.greeting ?: when {
            hour >= 5f && hour < 12f -> "Good morning"
            hour >= 12f && hour < 17f -> "Good afternoon"
            hour >= 17f && hour < 22f -> "Good evening"
            else -> "Hello, night owl"
        }

    /** A short line under the greeting. */
    val line: String
        get() = occasion?.line ?: when (dayPart) {
            DayPart.DAWN -> when (season) {
                Season.WINTER -> "A frosty sunrise. Ease into the day."
                else -> "The sun's coming up. Ease into the day."
            }
            DayPart.DAY -> when (season) {
                Season.SPRING -> "Blossoms out. Something bright?"
                Season.SUMMER -> "Long sunny day. Turn it up."
                Season.AUTUMN -> "Leaves falling. Something warm?"
                Season.WINTER -> "Snow outside, songs inside."
            }
            DayPart.DUSK -> "The sky's turning gold. Time to unwind."
            DayPart.NIGHT -> when (season) {
                Season.SUMMER -> "Warm night, fireflies out."
                else -> "Quiet night, soft stars."
            }
        }
}

fun dayPartOf(hour: Float): DayPart = when {
    hour >= 5f && hour < 8f -> DayPart.DAWN
    hour >= 8f && hour < 17f -> DayPart.DAY
    hour >= 17f && hour < 20f -> DayPart.DUSK
    else -> DayPart.NIGHT
}

/** The hour a fixed part of the day is drawn at. */
fun DayPart.hour(): Float = when (this) {
    DayPart.DAWN -> 6.6f
    DayPart.DAY -> 12f
    DayPart.DUSK -> 18.4f
    DayPart.NIGHT -> 23f
}

/** Meteorological seasons: whole months, flipped for the southern hemisphere. */
fun seasonOf(date: LocalDate, southern: Boolean): Season {
    val north = when (date.month) {
        Month.MARCH, Month.APRIL, Month.MAY -> Season.SPRING
        Month.JUNE, Month.JULY, Month.AUGUST -> Season.SUMMER
        Month.SEPTEMBER, Month.OCTOBER, Month.NOVEMBER -> Season.AUTUMN
        else -> Season.WINTER
    }
    if (!southern) return north
    return when (north) {
        Season.SPRING -> Season.AUTUMN
        Season.SUMMER -> Season.WINTER
        Season.AUTUMN -> Season.SPRING
        Season.WINTER -> Season.SUMMER
    }
}

/**
 * Lakshmi Puja dates (India). Diwali follows the lunar calendar, so the
 * dates are listed; the decorations stay up two days either side.
 */
private val DIWALI = listOf(
    LocalDate.of(2026, 11, 8),
    LocalDate.of(2027, 10, 29),
    LocalDate.of(2028, 10, 17),
    LocalDate.of(2029, 11, 5),
    LocalDate.of(2030, 10, 26),
)

fun occasionOf(date: LocalDate): Occasion? = when {
    DIWALI.any { abs(ChronoUnit.DAYS.between(it, date)) <= 2 } -> Occasion.DIWALI
    (date.month == Month.DECEMBER && date.dayOfMonth == 31) || (date.month == Month.JANUARY && date.dayOfMonth == 1) -> Occasion.NEW_YEAR
    date.month == Month.FEBRUARY && date.dayOfMonth == 14 -> Occasion.VALENTINE
    else -> null
}

/** The ambience for now, following the clock and calendar unless Settings fixes them. */
@Composable
fun rememberAmbience(settings: SettingsState): Ambience {
    val now by produceState(LocalDateTime.now()) {
        while (true) {
            delay(30_000)
            value = LocalDateTime.now()
        }
    }
    val date = now.toLocalDate()
    return Ambience(
        hour = settings.dayPart?.hour() ?: (now.hour + now.minute / 60f),
        season = settings.season ?: seasonOf(date, settings.southern),
        occasion = if (settings.specialDays) occasionOf(date) else null,
        date = date,
    )
}

/**
 * Seconds since the scene appeared, ticking about 24 times a second (plenty
 * for slow drifting). It starts a moment after the screen appears, so it
 * never competes with the app starting up. Frozen when the system asks for
 * no animations or Battery Saver is on.
 */
@Composable
private fun sceneClock(): State<Float> {
    val context = LocalContext.current
    val time = remember { mutableFloatStateOf(STILL_TIME) }
    val still = remember { prefersStill(context) }
    if (!still) {
        LaunchedEffect(Unit) {
            delay(2_500)
            val start = withFrameNanos { it }
            var last = 0L
            while (true) {
                withFrameNanos { now ->
                    if (now - last >= 41_000_000L) {
                        last = now
                        time.floatValue = STILL_TIME + (now - start) / 1e9f
                    }
                }
            }
        }
    }
    return time
}

/** A moment where every particle is already spread over the sky. */
private const val STILL_TIME = 40f

private fun prefersStill(context: Context): Boolean {
    val scale = android.provider.Settings.Global.getFloat(
        context.contentResolver,
        android.provider.Settings.Global.ANIMATOR_DURATION_SCALE,
        1f,
    )
    val power = context.getSystemService(Context.POWER_SERVICE) as? PowerManager
    return scale == 0f || power?.isPowerSaveMode == true
}

/**
 * The living background: a sky that follows the time of day (sun, moon,
 * clouds, stars), the season's particles, and special-day decorations. It
 * fades into the page at the bottom and blends with the theme's colours.
 */
@Composable
fun AmbientScene(ambience: Ambience, modifier: Modifier = Modifier, fadeBottom: Boolean = true) {
    val time by sceneClock()
    val dark = LocalDarkTheme.current
    val surface = MaterialTheme.colorScheme.surface
    val primary = MaterialTheme.colorScheme.primary
    val seeds = remember { Seeds() }
    Canvas(modifier.graphicsLayer(compositingStrategy = CompositingStrategy.Offscreen)) {
        val t = time
        val tone = Tone(surface, primary, dark)
        drawSky(ambience.hour, tone)
        val night = nightness(ambience.hour)
        drawStars(seeds, t, night)
        drawSunAndMoon(seeds, ambience.hour, t)
        drawClouds(seeds, t, ambience.hour, night, dark)
        when (ambience.season) {
            Season.WINTER -> drawSnow(seeds, t, dark)
            Season.SPRING -> drawPetals(seeds, t)
            Season.SUMMER -> drawSummer(seeds, t, night)
            Season.AUTUMN -> drawLeaves(seeds, t)
        }
        when (ambience.occasion) {
            Occasion.DIWALI -> {
                drawGarlands(t)
                if (night > 0.4f) drawFireworks(t, night, warm = true)
            }
            Occasion.NEW_YEAR -> drawFireworks(t, 0.5f + night / 2f, warm = false)
            Occasion.VALENTINE -> drawHearts(seeds, t)
            null -> Unit
        }
        if (fadeBottom) {
            drawRect(
                Brush.verticalGradient(0f to Color.Black, 0.4f to Color.Black.copy(alpha = 0.85f), 1f to Color.Transparent),
                blendMode = BlendMode.DstIn,
            )
        }
    }
}

/** How the sky is blended into the theme. */
private class Tone(val surface: Color, val primary: Color, val dark: Boolean) {
    fun sky(c: Color): Color {
        val tinted = lerp(c, primary, 0.12f)
        return lerp(tinted, surface, if (dark) 0.22f else 0.48f)
    }
}

/** Random but fixed numbers for every particle, so nothing is allocated per frame. */
private class Seeds {
    private val r = Random(1729)
    val stars = Array(48) { floatArrayOf(r.nextFloat(), r.nextFloat() * 0.72f, 0.7f + r.nextFloat() * 1.4f, 0.6f + r.nextFloat() * 2f, r.nextFloat() * 6.28f) }
    val particles = Array(36) { FloatArray(6) { r.nextFloat() } }
    /** Shapes drawn many times, made once at unit size and scaled when drawn. */
    val leaf = Path().apply {
        moveTo(-0.5f, 0f)
        quadraticTo(0f, -0.45f, 0.5f, 0f)
        quadraticTo(0f, 0.45f, -0.5f, 0f)
        close()
    }
    val heart = Path().apply {
        moveTo(0f, 0.35f)
        cubicTo(-0.6f, -0.05f, -0.35f, -0.55f, 0f, -0.22f)
        cubicTo(0.35f, -0.55f, 0.6f, -0.05f, 0f, 0.35f)
        close()
    }
    val crescent = Path().apply {
        val disc = Path().apply { addOval(androidx.compose.ui.geometry.Rect(Offset.Zero, 1f)) }
        val bite = Path().apply { addOval(androidx.compose.ui.geometry.Rect(Offset(0.42f, -0.28f), 0.92f)) }
        op(disc, bite, PathOperation.Difference)
    }
    val clouds = Array(4) { floatArrayOf(r.nextFloat(), 0.12f + r.nextFloat() * 0.3f, 0.7f + r.nextFloat() * 0.6f, 4f + r.nextFloat() * 6f) }
}

// ---------------------------------------------------------------- Sky

private data class Key(val hour: Float, val top: Color, val bottom: Color)

private val SKY = listOf(
    Key(0f, Color(0xFF0B1026), Color(0xFF1B2A4A)),
    Key(4.8f, Color(0xFF141A38), Color(0xFF2E2C55)),
    Key(6.4f, Color(0xFF3E4A89), Color(0xFFF2A07B)),
    Key(8.5f, Color(0xFF5B9BD5), Color(0xFFBFE0F5)),
    Key(12f, Color(0xFF4A90D9), Color(0xFFA9D4F5)),
    Key(16.3f, Color(0xFF5A97D0), Color(0xFFCDE3F2)),
    Key(18.4f, Color(0xFF3A3F7A), Color(0xFFF08A5D)),
    Key(20f, Color(0xFF1C2347), Color(0xFF4A3B66)),
    Key(24f, Color(0xFF0B1026), Color(0xFF1B2A4A)),
)

private fun skyAt(hour: Float): Pair<Color, Color> {
    val h = ((hour % 24f) + 24f) % 24f
    val i = SKY.indexOfLast { it.hour <= h }.coerceIn(0, SKY.size - 2)
    val a = SKY[i]
    val b = SKY[i + 1]
    val f = ((h - a.hour) / (b.hour - a.hour)).coerceIn(0f, 1f)
    return lerp(a.top, b.top, f) to lerp(a.bottom, b.bottom, f)
}

/** 1 at night, 0 by day, easing through dawn and dusk. */
private fun nightness(hour: Float): Float = when {
    hour < 4.5f || hour >= 20.5f -> 1f
    hour < 7.5f -> 1f - (hour - 4.5f) / 3f
    hour < 17.5f -> 0f
    else -> (hour - 17.5f) / 3f
}

/** How golden the light is: highest at sunrise and sunset. */
private fun goldenness(hour: Float): Float {
    fun peak(center: Float) = (1f - abs(hour - center) / 1.6f).coerceIn(0f, 1f)
    return maxOf(peak(6.6f), peak(18.4f))
}

private fun DrawScope.drawSky(hour: Float, tone: Tone) {
    val (top, bottom) = skyAt(hour)
    drawRect(Brush.verticalGradient(listOf(tone.sky(top), tone.sky(bottom))))
}

private fun DrawScope.drawStars(seeds: Seeds, t: Float, night: Float) {
    if (night <= 0.01f) return
    for (s in seeds.stars) {
        val twinkle = 0.35f + 0.65f * ((sin(t * s[3] + s[4]) + 1f) / 2f)
        drawCircle(Color.White.copy(alpha = night * twinkle * 0.9f), s[2].dp.toPx(), Offset(s[0] * size.width, s[1] * size.height))
    }
    // Now and then a shooting star.
    val period = 11f
    val k = floor(t / period).toInt()
    val p = (t - k * period) / 1.1f
    if (p < 1f) {
        val r = Random(k)
        val start = Offset(size.width * (0.35f + r.nextFloat() * 0.6f), size.height * (0.05f + r.nextFloat() * 0.25f))
        val travel = Offset(-140.dp.toPx(), 60.dp.toPx())
        val head = start + travel * p
        val tail = start + travel * (p - 0.35f).coerceAtLeast(0f)
        drawLine(
            Brush.linearGradient(listOf(Color.Transparent, Color.White.copy(alpha = night * (1f - p))), tail, head),
            tail, head, strokeWidth = 1.6.dp.toPx(), cap = StrokeCap.Round,
        )
    }
}

/** A point on the arc the sun or moon follows; [f] goes 0 → 1 across the sky. */
private fun DrawScope.arc(f: Float): Offset =
    Offset(size.width * (0.1f + 0.8f * f), size.height * (0.7f - 0.55f * sin(PI.toFloat() * f)))

private fun DrawScope.drawSunAndMoon(seeds: Seeds, hour: Float, t: Float) {
    // The sun: from about 5:30 to 19:30.
    if (hour in 5.4f..19.6f) {
        val f = ((hour - 5.6f) / 13.8f).coerceIn(0f, 1f)
        val c = arc(f)
        val gold = goldenness(hour)
        val color = lerp(Color(0xFFFFF1C1), Color(0xFFFFA36C), gold)
        val fade = (minOf(hour - 5.4f, 19.6f - hour) / 0.8f).coerceIn(0f, 1f)
        val pulse = 1f + 0.05f * sin(t * 0.9f)
        val glow = 70.dp.toPx() * pulse
        drawCircle(Brush.radialGradient(listOf(color.copy(alpha = 0.55f * fade), Color.Transparent), c, glow), glow, c)
        drawCircle(color.copy(alpha = fade), 17.dp.toPx(), c)
    }
    // The moon: from about 19:00 to 6:00.
    val h = if (hour < 12f) hour + 24f else hour
    if (h in 18.8f..30.2f) {
        val f = ((h - 19f) / 11f).coerceIn(0f, 1f)
        val c = arc(f) + Offset(0f, 2.dp.toPx() * sin(t * 0.4f))
        val fade = (minOf(h - 18.8f, 30.2f - h) / 0.8f).coerceIn(0f, 1f)
        val r = 14.dp.toPx()
        val glow = 56.dp.toPx()
        drawCircle(Brush.radialGradient(listOf(Color(0x55E8E6FF).copy(alpha = 0.33f * fade), Color.Transparent), c, glow), glow, c)
        withTransform({
            translate(c.x, c.y)
            scale(r, r, Offset.Zero)
        }) { drawPath(seeds.crescent, Color(0xFFF6F2DE).copy(alpha = fade)) }
    }
}

private fun DrawScope.drawClouds(seeds: Seeds, t: Float, hour: Float, night: Float, dark: Boolean) {
    val day = 1f - night
    if (day <= 0.02f) return
    val tint = lerp(Color.White, Color(0xFFFFC9A8), goldenness(hour))
    val alpha = day * if (dark) 0.22f else 0.55f
    for (c in seeds.clouds) {
        val w = 90.dp.toPx() * c[2]
        val span = size.width + w * 2
        val x = ((c[0] * span + t * c[3].dp.toPx()) % span) - w
        val y = size.height * c[1]
        val color = tint.copy(alpha = alpha)
        drawCircle(color, w * 0.22f, Offset(x + w * 0.25f, y + w * 0.06f))
        drawCircle(color, w * 0.3f, Offset(x + w * 0.5f, y - w * 0.04f))
        drawCircle(color, w * 0.2f, Offset(x + w * 0.76f, y + w * 0.08f))
        drawRoundRect(color, Offset(x + w * 0.1f, y + w * 0.02f), Size(w * 0.8f, w * 0.24f), androidx.compose.ui.geometry.CornerRadius(w * 0.12f))
    }
}

// ---------------------------------------------------------------- Seasons

/**
 * Where a falling particle is: [p] holds its fixed random numbers. It falls at
 * [speed] dp a second, sways sideways by [sway] dp and wraps round the top.
 */
private fun DrawScope.falling(p: FloatArray, t: Float, speed: Float, sway: Float, drift: Float = 0f): Offset {
    val margin = 16.dp.toPx()
    val h = size.height + margin * 2
    val v = (speed * (0.6f + 0.8f * p[2])).dp.toPx()
    val y = ((p[3] * h + t * v) % h + h) % h - margin
    val w = size.width + margin * 2
    val x = ((p[0] * w + t * drift.dp.toPx()) % w + w) % w - margin +
        sin(t * (0.5f + p[4]) + p[5] * 6.28f) * sway.dp.toPx()
    return Offset(x, y)
}

private fun DrawScope.drawSnow(seeds: Seeds, t: Float, dark: Boolean) {
    val color = if (dark) Color.White else Color(0xFFF4F8FF)
    val edge = Color(0xFF7D8FB3).copy(alpha = if (dark) 0f else 0.35f)
    val ring = Stroke(0.6.dp.toPx())
    for (p in seeds.particles) {
        val c = falling(p, t, speed = 22f, sway = 10f)
        val r = (1.3f + 2.4f * p[1]).dp.toPx()
        drawCircle(color.copy(alpha = 0.55f + 0.4f * p[1]), r, c)
        if (!dark) drawCircle(edge, r, c, style = ring)
    }
}

private val PETALS = listOf(Color(0xFFF7B6CF), Color(0xFFFFD6E5), Color(0xFFF48FB1), Color(0xFFFFE4EE))

private fun DrawScope.drawPetals(seeds: Seeds, t: Float) {
    for (i in 0 until 20) {
        val p = seeds.particles[i]
        val c = falling(p, t, speed = 16f, sway = 14f, drift = 10f)
        val s = 0.7f + 0.6f * p[1]
        val w = 7.dp.toPx() * s
        val h = 4.5.dp.toPx() * s
        rotate(t * (40f + 90f * p[4]) + p[5] * 360f, c) {
            drawOval(PETALS[i % PETALS.size].copy(alpha = 0.9f), Offset(c.x - w / 2, c.y - h / 2), Size(w, h))
        }
    }
}

private val LEAVES = listOf(Color(0xFFE07A3F), Color(0xFFC8553D), Color(0xFFE9A23B), Color(0xFFA0522D))

private fun DrawScope.drawLeaves(seeds: Seeds, t: Float) {
    for (i in 0 until 16) {
        val p = seeds.particles[i]
        val c = falling(p, t, speed = 18f, sway = 18f, drift = -6f)
        val len = (8f + 5f * p[1]).dp.toPx()
        val angle = sin(t * (0.8f + p[4]) + p[5] * 6.28f) * 70f + p[5] * 360f
        val color = LEAVES[i % LEAVES.size]
        withTransform({
            translate(c.x, c.y)
            rotate(angle, Offset.Zero)
            scale(len, len, Offset.Zero)
        }) { drawPath(seeds.leaf, color.copy(alpha = 0.92f)) }
        rotate(angle, c) {
            drawLine(lerp(color, Color.Black, 0.3f), Offset(c.x - len / 2, c.y), Offset(c.x + len * 0.6f, c.y), strokeWidth = 0.8.dp.toPx())
        }
    }
}

/** Summer: light motes drifting up by day, fireflies by night. */
private fun DrawScope.drawSummer(seeds: Seeds, t: Float, night: Float) {
    for (i in 0 until 18) {
        val p = seeds.particles[i]
        if (night < 0.98f) {
            // Rising motes of light.
            val c = falling(p, -t, speed = 7f, sway = 8f)
            val a = (1f - night) * (0.4f + 0.4f * sin(t * 1.3f + p[5] * 6.28f).coerceAtLeast(0f))
            val r = (1.4f + 1.8f * p[1]).dp.toPx()
            drawCircle(Color(0xFFFFF4C2).copy(alpha = a * 0.35f), r * 3f, c)
            drawCircle(Color(0xFFFFF8DC).copy(alpha = a), r, c)
        }
        if (night > 0.02f) {
            // Fireflies wander and blink.
            val c = Offset(
                size.width * p[0] + sin(t * (0.25f + p[4] * 0.3f) + p[5] * 6.28f) * 34.dp.toPx(),
                size.height * (0.2f + 0.7f * p[3]) + cos(t * (0.3f + p[2] * 0.3f) + p[5] * 6.28f) * 18.dp.toPx(),
            )
            val blink = sin(t * (1.2f + p[1]) + p[5] * 6.28f).coerceAtLeast(0f)
            val a = night * blink
            drawCircle(Color(0xFFE6FF8A).copy(alpha = a * 0.3f), 7.dp.toPx(), c)
            drawCircle(Color(0xFFF4FFB8).copy(alpha = a), 1.8.dp.toPx(), c)
        }
    }
}

// ---------------------------------------------------------------- Special days

private val GARLAND = listOf(Color(0xFFFFD54F), Color(0xFFFF8A65), Color(0xFFF06292), Color(0xFFFFB300), Color(0xFFAED581))

/** Diwali: strings of twinkling lights hung across the top. */
private fun DrawScope.drawGarlands(t: Float) {
    val rows = listOf(10.dp.toPx() to 22.dp.toPx(), 4.dp.toPx() to 40.dp.toPx())
    rows.forEachIndexed { row, (top, sag) ->
        val wire = Path()
        val steps = 40
        for (s in 0..steps) {
            val f = s / steps.toFloat()
            val x = size.width * f
            val y = top + sag * 4f * f * (1f - f)
            if (s == 0) wire.moveTo(x, y) else wire.lineTo(x, y)
        }
        drawPath(wire, Color(0xFF6D4C41).copy(alpha = 0.6f), style = Stroke(1.dp.toPx()))
        val bulbs = (size.width / 24.dp.toPx()).toInt()
        for (b in 1 until bulbs) {
            val f = b / bulbs.toFloat()
            val c = Offset(size.width * f, top + sag * 4f * f * (1f - f) + 4.dp.toPx())
            val color = GARLAND[(b + row * 2) % GARLAND.size]
            val glow = 0.45f + 0.55f * ((sin(t * 2.6f + b * 1.7f + row) + 1f) / 2f)
            drawCircle(color.copy(alpha = 0.35f * glow), 8.dp.toPx(), c)
            drawCircle(color.copy(alpha = 0.6f + 0.4f * glow), 3.dp.toPx(), c)
        }
    }
}

private val WARM = listOf(Color(0xFFFFD54F), Color(0xFFFF8A65), Color(0xFFFFB74D))
private val BRIGHT = listOf(Color(0xFF80DEEA), Color(0xFFF48FB1), Color(0xFFFFF176), Color(0xFFB39DDB), Color(0xFFA5D6A7))

/** Bursts of sparks, two at a time, slightly out of step. */
private fun DrawScope.drawFireworks(t: Float, strength: Float, warm: Boolean) {
    val period = 2.8f
    for (lane in 0..1) {
        val lt = t + lane * period / 2f
        val k = floor(lt / period).toInt()
        val p = (lt - k * period) / period
        val r = Random(k * 2 + lane)
        val center = Offset(size.width * (0.15f + 0.7f * r.nextFloat()), size.height * (0.15f + 0.3f * r.nextFloat()))
        val colors = if (warm) WARM else BRIGHT
        val color = colors[r.nextInt(colors.size)]
        val ease = 1f - (1f - p) * (1f - p)
        val radius = (46f + 30f * r.nextFloat()).dp.toPx() * ease
        val fall = 26.dp.toPx() * p * p
        val alpha = strength * (1f - p)
        val sparks = 22
        for (s in 0 until sparks) {
            val a = s * 2f * PI.toFloat() / sparks + r.nextFloat() * 0.2f
            val c = center + Offset(cos(a) * radius, sin(a) * radius + fall)
            drawCircle(color.copy(alpha = alpha), 1.8.dp.toPx(), c)
            drawCircle(color.copy(alpha = alpha * 0.25f), 4.5.dp.toPx(), c)
        }
    }
}

/** Valentine's Day: hearts floating up. */
private fun DrawScope.drawHearts(seeds: Seeds, t: Float) {
    for (i in 0 until 12) {
        val p = seeds.particles[seeds.particles.size - 12 + i]
        val c = falling(p, -t, speed = 13f, sway = 12f)
        val s = (9f + 6f * p[1]).dp.toPx()
        val color = if (i % 3 == 0) Color(0xFFE57373) else Color(0xFFF48FB1)
        withTransform({
            translate(c.x, c.y)
            scale(s, s, Offset.Zero)
        }) { drawPath(seeds.heart, color.copy(alpha = 0.8f)) }
    }
}
