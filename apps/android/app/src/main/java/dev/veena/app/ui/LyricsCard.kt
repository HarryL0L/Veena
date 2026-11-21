package dev.veena.app.ui

import androidx.compose.animation.animateColorAsState
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
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
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import dev.veena.app.LyricsUi
import uniffi.veena_ffi.LyricsInfo

/**
 * Lyrics on the full player. Synced lyrics highlight the line being sung
 * and keep it in view; tap a line to jump there.
 */
@Composable
fun LyricsCard(lyrics: LyricsUi, positionMs: Long, onSeek: (Long) -> Unit) {
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
                    else -> Lines(lyrics.lyrics, positionMs, onSeek)
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
private fun Lines(lyrics: LyricsInfo, positionMs: Long, onSeek: (Long) -> Unit) {
    val list = rememberLazyListState()
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
    Box(Modifier.height(320.dp)) {
        LazyColumn(
            state = list,
            modifier = Modifier.fillMaxSize(),
            contentPadding = PaddingValues(horizontal = 24.dp, vertical = 12.dp),
        ) {
            itemsIndexed(lyrics.lines) { index, line ->
                val active = index == current
                val color by animateColorAsState(
                    when {
                        !lyrics.synced -> MaterialTheme.colorScheme.onSecondaryContainer
                        active -> MaterialTheme.colorScheme.onSecondaryContainer
                        else -> MaterialTheme.colorScheme.onSecondaryContainer.copy(alpha = 0.45f)
                    },
                    label = "lyric line",
                )
                Text(
                    line.text.ifBlank { "♪" },
                    style = if (lyrics.synced) MaterialTheme.typography.headlineSmall else MaterialTheme.typography.bodyLarge,
                    fontWeight = if (active) FontWeight.Bold else FontWeight.SemiBold,
                    color = color,
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(vertical = 6.dp)
                        .then(line.timeMs?.let { t -> Modifier.clickable { onSeek(t.toLong()) } } ?: Modifier),
                )
            }
        }
    }
    Text(
        "Lyrics from ${lyrics.source}",
        style = MaterialTheme.typography.labelSmall,
        modifier = Modifier.padding(start = 24.dp, bottom = 16.dp),
    )
}
