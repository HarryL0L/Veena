package dev.veena.app.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.systemBarsPadding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.FilledIconButton
import androidx.compose.material3.FilledTonalIconToggleButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.IconButtonDefaults
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Slider
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import dev.veena.app.R
import uniffi.veena_ffi.PlayState
import uniffi.veena_ffi.PlayerState
import uniffi.veena_ffi.RepeatMode

/** The full-screen player: big artwork, seek bar, controls and the queue. */
@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
fun PlayerScreen(
    player: PlayerState,
    onClose: () -> Unit,
    onToggle: () -> Unit,
    onNext: () -> Unit,
    onPrevious: () -> Unit,
    onSeek: (Long) -> Unit,
    onShuffle: () -> Unit,
    onRepeat: () -> Unit,
    onPlayQueueItem: (Int) -> Unit,
) {
    val currentIndex = player.current?.toInt() ?: return
    val track = player.queue.getOrNull(currentIndex) ?: return
    val duration = (player.durationMs ?: track.durationMs)?.toLong()
    val playing = player.state == PlayState.PLAYING
    // While the thumb is held, show where it is rather than the live position.
    var dragging by remember { mutableStateOf<Float?>(null) }

    Surface(Modifier.fillMaxSize(), color = MaterialTheme.colorScheme.surface) {
        LazyColumn(
            modifier = Modifier.systemBarsPadding(),
            state = rememberLazyListState(),
            contentPadding = PaddingValues(bottom = 24.dp),
        ) {
            item {
                Row(Modifier.fillMaxWidth().padding(8.dp), verticalAlignment = Alignment.CenterVertically) {
                    IconButton(onClick = onClose) {
                        Icon(painterResource(R.drawable.ic_expand_more), contentDescription = "Close player")
                    }
                    Text(
                        "Now playing",
                        style = MaterialTheme.typography.titleMedium,
                        modifier = Modifier.weight(1f),
                        textAlign = TextAlign.Center,
                    )
                    Spacer(Modifier.size(48.dp))
                }
            }

            item {
                Artwork(
                    track.artwork,
                    sizeDp = 400,
                    modifier = Modifier.padding(horizontal = 32.dp, vertical = 16.dp).fillMaxWidth().aspectRatio(1f),
                )
            }

            item {
                Text(
                    track.title,
                    style = MaterialTheme.typography.headlineSmall,
                    fontWeight = FontWeight.SemiBold,
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.padding(horizontal = 32.dp),
                )
                Text(
                    listOfNotNull(track.artist, track.album).joinToString(" · "),
                    style = MaterialTheme.typography.bodyLarge,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.padding(horizontal = 32.dp, vertical = 4.dp),
                )
            }

            item {
                val total = duration?.takeIf { it > 0 }
                val live = total?.let { (player.positionMs.toLong().toFloat() / it).coerceIn(0f, 1f) } ?: 0f
                val shown = dragging ?: live
                Slider(
                    value = shown,
                    onValueChange = { dragging = it },
                    onValueChangeFinished = {
                        val target = dragging
                        dragging = null
                        if (target != null && total != null) onSeek((target * total).toLong())
                    },
                    enabled = total != null,
                    modifier = Modifier.padding(horizontal = 24.dp).padding(top = 12.dp),
                )
                Row(Modifier.fillMaxWidth().padding(horizontal = 32.dp), horizontalArrangement = Arrangement.SpaceBetween) {
                    val position = if (dragging != null && total != null) (shown * total).toLong() else player.positionMs.toLong()
                    Text(formatTime(position), style = MaterialTheme.typography.labelMedium)
                    Text(
                        if (player.state == PlayState.LOADING) "Loading…" else total?.let { formatTime(it) } ?: "",
                        style = MaterialTheme.typography.labelMedium,
                    )
                }
            }

            item {
                Row(
                    Modifier.fillMaxWidth().padding(horizontal = 24.dp, vertical = 16.dp),
                    horizontalArrangement = Arrangement.SpaceEvenly,
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    FilledTonalIconToggleButton(checked = player.shuffle, onCheckedChange = { onShuffle() }) {
                        Icon(painterResource(R.drawable.ic_shuffle), contentDescription = "Shuffle")
                    }
                    IconButton(onClick = onPrevious, modifier = Modifier.size(56.dp)) {
                        Icon(painterResource(R.drawable.ic_skip_previous), contentDescription = "Previous", Modifier.size(32.dp))
                    }
                    FilledIconButton(
                        onClick = onToggle,
                        modifier = Modifier.size(80.dp),
                        shape = IconButtonDefaults.mediumSquareShape,
                    ) {
                        Icon(
                            painterResource(if (playing) R.drawable.ic_pause else R.drawable.ic_play),
                            contentDescription = if (playing) "Pause" else "Play",
                            modifier = Modifier.size(40.dp),
                        )
                    }
                    IconButton(onClick = onNext, modifier = Modifier.size(56.dp)) {
                        Icon(painterResource(R.drawable.ic_skip_next), contentDescription = "Next", Modifier.size(32.dp))
                    }
                    FilledTonalIconToggleButton(
                        checked = player.repeat != RepeatMode.OFF,
                        onCheckedChange = { onRepeat() },
                    ) {
                        Icon(
                            painterResource(if (player.repeat == RepeatMode.ONE) R.drawable.ic_repeat_one else R.drawable.ic_repeat),
                            contentDescription = when (player.repeat) {
                                RepeatMode.OFF -> "Repeat off"
                                RepeatMode.ALL -> "Repeat all"
                                RepeatMode.ONE -> "Repeat one"
                            },
                        )
                    }
                }
            }

            if (player.queue.size > 1) {
                item {
                    Text(
                        "Queue",
                        style = MaterialTheme.typography.titleMedium,
                        modifier = Modifier.padding(horizontal = 24.dp, vertical = 8.dp),
                    )
                }
                itemsIndexed(player.queue, key = { i, t -> "q$i:${t.kind}:${t.id}" }) { index, item ->
                    val isCurrent = index == currentIndex
                    ListItem(
                        selected = isCurrent,
                        onClick = { onPlayQueueItem(index) },
                        modifier = Modifier.padding(horizontal = 8.dp),
                        supportingContent = item.artist?.let { { Text(it, maxLines = 1, overflow = TextOverflow.Ellipsis) } },
                        leadingContent = { Artwork(item.artwork, 44) },
                        trailingContent = { item.durationMs?.let { Text(formatTime(it.toLong())) } },
                    ) {
                        Text(
                            item.title,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                            fontWeight = if (isCurrent) FontWeight.Bold else null,
                        )
                    }
                }
            }
        }
    }
}
