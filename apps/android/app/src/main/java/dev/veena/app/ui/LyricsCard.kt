package dev.veena.app.ui

import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.Spring
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.spring
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.State
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.withFrameMillis
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Shadow
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import dev.veena.app.LyricsUi
import uniffi.veena_ffi.LyricLineInfo
import uniffi.veena_ffi.LyricsInfo

/**
 * Lyrics on the full player. Synced lyrics highlight the line being sung
 * and keep it in view; tap a line to jump there.
 */
@Composable
fun LyricsCard(lyrics: LyricsUi, positionMs: Long, playing: Boolean, onSeek: (Long) -> Unit) {
    if (lyrics == LyricsUi.None) return
    Surface(
        modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp),
        shape = RoundedCornerShape(28.dp),
        color = MaterialTheme.colorScheme.secondaryContainer,
        contentColor = MaterialTheme.colorScheme.onSecondaryContainer,
    ) {
        Column(Modifier.padding(top = 20.dp)) {
            Text(
                "Lyrics",
                style = MaterialTheme.typography.titleMedium,
                fontWeight = FontWeight.Bold,
                modifier = Modifier.padding(horizontal = 24.dp),
            )
            when (lyrics) {
                LyricsUi.Loading -> Message("Looking for lyrics…")
                LyricsUi.NotFound -> Message("No lyrics found for this song.")
                is LyricsUi.Found -> when {
                    lyrics.lyrics.instrumental -> Message("Instrumental: no words in this one.")
                    else -> Lines(lyrics.lyrics, positionMs, playing, onSeek)
                }
                LyricsUi.None -> {}
            }
        }
    }
}

@Composable
private fun Message(text: String) {
    Text(text, style = MaterialTheme.typography.bodyLarge, modifier = Modifier.padding(24.dp))
}

@Composable
private fun Lines(lyrics: LyricsInfo, positionMs: Long, playing: Boolean, onSeek: (Long) -> Unit) {
    val list = rememberLazyListState()
    val timed = remember(lyrics) { if (lyrics.synced) timeWords(lyrics.lines) else emptyList() }
    // The line being sung: the last one that has started (a little early,
    // so the highlight doesn't lag the voice).
    val current = if (lyrics.synced) {
        lyrics.lines.indexOfLast { (it.timeMs?.toLong() ?: Long.MAX_VALUE) <= positionMs + 300 }
    } else {
        -1
    }
    LaunchedEffect(current) {
        if (current >= 0) list.animateScrollToItem((current - 2).coerceAtLeast(0))
    }
    val dim = MaterialTheme.colorScheme.onSecondaryContainer.copy(alpha = 0.4f)
    val bright = MaterialTheme.colorScheme.onSecondaryContainer
    Box(Modifier.height(320.dp)) {
        LazyColumn(
            state = list,
            modifier = Modifier.fillMaxSize(),
            contentPadding = PaddingValues(horizontal = 24.dp, vertical = 12.dp),
        ) {
            itemsIndexed(lyrics.lines) { index, line ->
                val seek = line.timeMs?.let { t -> Modifier.clickable { onSeek(t.toLong()) } } ?: Modifier
                val style = if (lyrics.synced) MaterialTheme.typography.headlineSmall else MaterialTheme.typography.bodyLarge
                if (index == current && timed.getOrNull(index)?.isNotEmpty() == true) {
                    SungLine(timed[index], positionMs, playing, style, bright, dim, Modifier.fillMaxWidth().padding(vertical = 6.dp).then(seek))
                } else {
                    val color by animateColorAsState(
                        when {
                            !lyrics.synced -> bright
                            index < current -> bright.copy(alpha = 0.7f)
                            else -> dim
                        },
                        label = "lyric line",
                    )
                    Text(
                        line.text.ifBlank { "♪" },
                        style = style,
                        fontWeight = FontWeight.SemiBold,
                        color = color,
                        modifier = Modifier.fillMaxWidth().padding(vertical = 6.dp).then(seek),
                    )
                }
            }
        }
    }
    Text(
        "Lyrics from ${lyrics.source}",
        style = MaterialTheme.typography.labelSmall,
        modifier = Modifier.padding(start = 24.dp, bottom = 16.dp),
    )
}

/** A word and the time span in which it is sung. */
private class TimedWord(val text: String, val start: Long, val end: Long)

/**
 * Times for every word. Word-timed lyrics give them directly; for lines
 * timed only as a whole, the line's time is shared out by word length.
 */
private fun timeWords(lines: List<LyricLineInfo>): List<List<TimedWord>> = lines.mapIndexed { i, line ->
    val start = line.timeMs?.toLong() ?: return@mapIndexed emptyList()
    val next = lines.getOrNull(i + 1)?.timeMs?.toLong()
    // Leave a short pause before the next line; cap long gaps (instrumental breaks).
    val end = minOf(next?.let { it - 150 } ?: (start + 4000), start + 8000).coerceAtLeast(start + 300)
    if (line.words.isNotEmpty()) {
        line.words.mapIndexed { w, word ->
            val wordEnd = line.words.getOrNull(w + 1)?.timeMs?.toLong() ?: end
            TimedWord(word.text, word.timeMs.toLong(), wordEnd.coerceAtLeast(word.timeMs.toLong() + 80))
        }
    } else {
        val words = Regex("""\S+\s*""").findAll(line.text).map { it.value }.toList()
        val total = words.sumOf { it.trim().length.coerceAtLeast(1) }.coerceAtLeast(1)
        var at = start.toDouble()
        val span = (end - start).toDouble()
        words.map { word ->
            val length = span * word.trim().length.coerceAtLeast(1) / total
            TimedWord(word, at.toLong(), (at + length).toLong()).also { at += length }
        }
    }
}

/**
 * The line being sung. Each word fills with colour as it is sung, rises a
 * little and glows, then settles; words still to come stay faint.
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun SungLine(
    words: List<TimedWord>,
    positionMs: Long,
    playing: Boolean,
    style: TextStyle,
    bright: Color,
    dim: Color,
    modifier: Modifier,
) {
    val now by rememberSmoothPosition(positionMs, playing)
    val glow = MaterialTheme.colorScheme.primary
    FlowRow(modifier) {
        words.forEach { word ->
            val progress = ((now - word.start).toFloat() / (word.end - word.start)).coerceIn(0f, 1f)
            val singing = now >= word.start && now < word.end
            val lift by animateFloatAsState(
                targetValue = if (singing) 1f else 0f,
                animationSpec = spring(dampingRatio = 0.45f, stiffness = Spring.StiffnessMediumLow),
                label = "word lift",
            )
            val brush = Brush.horizontalGradient(
                0f to bright,
                progress to bright,
                (progress + 0.02f).coerceAtMost(1f) to dim,
                1f to dim,
            )
            Text(
                word.text,
                style = style.copy(
                    brush = brush,
                    fontWeight = FontWeight.Bold,
                    shadow = Shadow(glow.copy(alpha = 0.6f * lift), blurRadius = 18f * lift),
                ),
                modifier = Modifier.graphicsLayer {
                    val scale = 1f + 0.08f * lift
                    scaleX = scale
                    scaleY = scale
                    translationY = -6.dp.toPx() * lift
                },
            )
        }
    }
}

/**
 * The playback position, moved forward every frame between the player's
 * updates (which come a few times a second) so the colour sweep is smooth.
 */
@Composable
private fun rememberSmoothPosition(positionMs: Long, playing: Boolean): State<Long> {
    val smooth = remember { mutableLongStateOf(positionMs) }
    LaunchedEffect(positionMs, playing) {
        smooth.longValue = positionMs
        if (!playing) return@LaunchedEffect
        val since = withFrameMillis { it }
        while (true) {
            withFrameMillis { frame -> smooth.longValue = positionMs + (frame - since) }
        }
    }
    return smooth
}
