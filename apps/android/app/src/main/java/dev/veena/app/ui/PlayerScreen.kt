package dev.veena.app.ui

import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.Spring
import androidx.compose.animation.core.animateDpAsState
import androidx.compose.animation.core.spring
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.slideInHorizontally
import androidx.compose.animation.slideOutHorizontally
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.basicMarquee
import androidx.compose.foundation.gestures.Orientation
import androidx.compose.foundation.gestures.draggable
import androidx.compose.foundation.gestures.rememberDraggableState
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.systemBarsPadding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.pager.HorizontalPager
import androidx.compose.foundation.pager.PagerState
import androidx.compose.foundation.pager.rememberPagerState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.FilledIconButton
import androidx.compose.material3.FilledTonalIconToggleButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.IconToggleButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.LocalContentColor
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Slider
import androidx.compose.material3.Surface
import androidx.compose.material3.SwipeToDismissBox
import androidx.compose.material3.SwipeToDismissBoxValue
import androidx.compose.material3.rememberSwipeToDismissBoxState
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import androidx.compose.ui.util.lerp
import dev.veena.app.R
import dev.veena.app.LyricsUi
import dev.veena.app.SleepTimer
import kotlinx.coroutines.delay
import sh.calvin.reorderable.ReorderableItem
import sh.calvin.reorderable.rememberReorderableLazyListState
import uniffi.veena_ffi.TrackInfo
import kotlin.math.absoluteValue
import kotlin.math.roundToInt
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.drop
import kotlinx.coroutines.launch
import uniffi.veena_ffi.PlayState
import uniffi.veena_ffi.PlayerState
import uniffi.veena_ffi.RepeatMode

/**
 * The full-screen player, coloured from the song's artwork. Swipe the cover
 * to change songs; drag the top part down to close.
 */
@OptIn(ExperimentalMaterial3ExpressiveApi::class, ExperimentalFoundationApi::class)
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
    onRemove: (Int) -> Unit,
    onMove: (from: Int, to: Int) -> Unit,
    sleep: SleepTimer?,
    onSleep: (minutes: Int?) -> Unit,
    onCancelSleep: () -> Unit,
    liked: Boolean,
    onToggleLike: () -> Unit,
    onAddToPlaylist: () -> Unit,
    lyrics: LyricsUi,
) {
    val currentIndex = player.current?.toInt() ?: return
    val track = player.queue.getOrNull(currentIndex) ?: return
    val duration = (player.durationMs ?: track.durationMs)?.toLong()
    val playing = player.state == PlayState.PLAYING
    // While the thumb is held, show where it is rather than the live position.
    var dragging by remember { mutableStateOf<Float?>(null) }

    // Drag down to close: the screen follows the finger, then closes or springs back.
    val scope = rememberCoroutineScope()
    val offset = remember { Animatable(0f) }
    val closeDistance = with(LocalDensity.current) { 160.dp.toPx() }
    val dragToClose = Modifier.draggable(
        orientation = Orientation.Vertical,
        state = rememberDraggableState { delta ->
            scope.launch { offset.snapTo((offset.value + delta).coerceAtLeast(0f)) }
        },
        onDragStopped = { velocity ->
            if (offset.value > closeDistance || velocity > 2500f) {
                onClose()
            } else {
                offset.animateTo(0f, spring(dampingRatio = Spring.DampingRatioMediumBouncy))
            }
        },
    )

    ArtworkTheme(track.artwork) {
        val colors = MaterialTheme.colorScheme
        Box(
            Modifier
                .fillMaxSize()
                .offset { IntOffset(0, offset.value.roundToInt()) }
                .background(Brush.verticalGradient(listOf(colors.primaryContainer, colors.surface, colors.surface))),
        ) {
            // The queue as shown, reordered live while dragging and sent to
            // the player when the drag ends.
            val entriesState = remember(player.queue) {
                mutableStateOf(player.queue.mapIndexed { i, t -> QueueEntry("q$i:${t.kind}:${t.id}", i, t) })
            }
            var entries by entriesState
            val listState = rememberLazyListState()
            val reorder = rememberReorderableLazyListState(listState) { from, to ->
                val a = entries.indexOfFirst { it.key == from.key }
                val b = entries.indexOfFirst { it.key == to.key }
                if (a >= 0 && b >= 0) entries = entries.toMutableList().apply { add(b, removeAt(a)) }
            }
            LazyColumn(
                state = listState,
                modifier = Modifier.systemBarsPadding(),
                contentPadding = PaddingValues(bottom = 24.dp),
            ) {
                item {
                    Row(
                        Modifier.fillMaxWidth().padding(8.dp).then(dragToClose),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        IconButton(onClick = onClose) {
                            Icon(painterResource(R.drawable.ic_expand_more), contentDescription = "Close player")
                        }
                        Column(Modifier.weight(1f), horizontalAlignment = Alignment.CenterHorizontally) {
                            Text(
                                "Playing from ${track.source.ifEmpty { "your queue" }}",
                                style = MaterialTheme.typography.labelMedium,
                                color = colors.onSurfaceVariant,
                            )
                            Text(
                                sleepLabel(sleep) ?: "${currentIndex + 1} of ${player.queue.size}",
                                style = MaterialTheme.typography.titleSmall,
                                textAlign = TextAlign.Center,
                            )
                        }
                        SleepMenu(sleep, onSleep, onCancelSleep)
                    }
                }

                item {
                    Box(dragToClose) {
                        CoverPager(player, currentIndex, onPlayQueueItem)
                    }
                }

                item {
                    AnimatedContent(
                        targetState = track,
                        transitionSpec = {
                            (slideInHorizontally { it / 4 } + fadeIn()) togetherWith (slideOutHorizontally { -it / 4 } + fadeOut())
                        },
                        contentKey = { "${it.kind}:${it.id}" },
                        label = "title",
                    ) { shown ->
                        Row(Modifier.fillMaxWidth().padding(start = 32.dp, end = 16.dp), verticalAlignment = Alignment.CenterVertically) {
                        Column(Modifier.weight(1f)) {
                            Text(
                                shown.title,
                                style = MaterialTheme.typography.headlineSmall,
                                fontWeight = FontWeight.Bold,
                                maxLines = 1,
                                modifier = Modifier.basicMarquee(),
                            )
                            Text(
                                listOfNotNull(shown.artist, shown.album).joinToString(" · "),
                                style = MaterialTheme.typography.bodyLarge,
                                color = colors.onSurfaceVariant,
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis,
                                modifier = Modifier.padding(top = 4.dp),
                            )
                        }
                        IconButton(onClick = onAddToPlaylist) {
                            Icon(painterResource(R.drawable.ic_playlist_add), contentDescription = "Add to playlist")
                        }
                        IconToggleButton(checked = liked, onCheckedChange = { onToggleLike() }) {
                            Icon(
                                painterResource(if (liked) R.drawable.ic_favorite else R.drawable.ic_favorite_border),
                                contentDescription = if (liked) "Remove from liked" else "Like",
                                tint = if (liked) colors.primary else LocalContentColor.current,
                            )
                        }
                        }
                    }
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
                        modifier = Modifier.padding(horizontal = 24.dp).padding(top = 16.dp),
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
                    Controls(player, playing, onToggle, onNext, onPrevious, onShuffle, onRepeat)
                }

                item {
                    LyricsCard(lyrics, player.positionMs.toLong(), onSeek)
                }

                if (player.queue.size > 1) {
                    item {
                        Column(Modifier.padding(horizontal = 24.dp, vertical = 8.dp)) {
                            Text("Queue", style = MaterialTheme.typography.titleMedium)
                            Text(
                                "Drag ≡ to reorder · swipe to remove",
                                style = MaterialTheme.typography.labelMedium,
                                color = colors.onSurfaceVariant,
                            )
                        }
                    }
                    items(entries, key = { it.key }) { entry ->
                        ReorderableItem(reorder, key = entry.key) { isDragging ->
                            QueueRow(
                                entry = entry,
                                isCurrent = entry.index == currentIndex,
                                dragging = isDragging,
                                onClick = { onPlayQueueItem(entry.index) },
                                onRemove = { onRemove(entry.index) },
                                handle = Modifier.draggableHandle(
                                    onDragStopped = {
                                        val to = entriesState.value.indexOf(entry)
                                        if (to >= 0 && to != entry.index) onMove(entry.index, to)
                                    },
                                ),
                            )
                        }
                    }
                }
            }
        }
    }
}

/** One song in the queue, as captured when the screen last updated. */
private data class QueueEntry(val key: String, val index: Int, val track: TrackInfo)

@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
private fun QueueRow(
    entry: QueueEntry,
    isCurrent: Boolean,
    dragging: Boolean,
    onClick: () -> Unit,
    onRemove: () -> Unit,
    handle: Modifier,
) {
    val item = entry.track
    val swipe = rememberSwipeToDismissBoxState()
    val elevation by animateDpAsState(if (dragging) 8.dp else 0.dp, label = "drag elevation")
    SwipeToDismissBox(
        state = swipe,
        modifier = Modifier.padding(horizontal = 8.dp),
        onDismiss = { onRemove() },
        backgroundContent = {
            Box(
                Modifier
                    .fillMaxSize()
                    .clip(RoundedCornerShape(16.dp))
                    .background(MaterialTheme.colorScheme.errorContainer)
                    .padding(horizontal = 24.dp),
                contentAlignment = if (swipe.dismissDirection == SwipeToDismissBoxValue.StartToEnd) {
                    Alignment.CenterStart
                } else {
                    Alignment.CenterEnd
                },
            ) {
                Icon(
                    painterResource(R.drawable.ic_delete),
                    contentDescription = "Remove",
                    tint = MaterialTheme.colorScheme.onErrorContainer,
                )
            }
        },
    ) {
        Surface(shadowElevation = elevation, shape = RoundedCornerShape(16.dp), color = Color.Transparent) {
            ListItem(
                selected = isCurrent,
                onClick = onClick,
                supportingContent = item.artist?.let { { Text(it, maxLines = 1, overflow = TextOverflow.Ellipsis) } },
                leadingContent = { Artwork(item.artwork, 44) },
                trailingContent = {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        item.durationMs?.let { Text(formatTime(it.toLong())) }
                        IconButton(onClick = {}, modifier = handle) {
                            Icon(painterResource(R.drawable.ic_drag_handle), contentDescription = "Reorder")
                        }
                    }
                },
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

/** "Sleep in 12 min" while a sleep timer runs; updates every few seconds. */
@Composable
private fun sleepLabel(sleep: SleepTimer?): String? {
    if (sleep == null) return null
    val endsAt = sleep.endsAt ?: return "Pausing at the end of this song"
    val now by produceState(System.currentTimeMillis(), endsAt) {
        while (true) {
            value = System.currentTimeMillis()
            delay(5_000)
        }
    }
    val minutes = ((endsAt - now + 59_999) / 60_000).coerceAtLeast(1)
    return "Pausing in $minutes min"
}

@Composable
private fun SleepMenu(sleep: SleepTimer?, onSleep: (Int?) -> Unit, onCancel: () -> Unit) {
    var open by remember { mutableStateOf(false) }
    Box {
        IconButton(onClick = { open = true }) {
            Icon(
                painterResource(R.drawable.ic_bedtime),
                contentDescription = "Sleep timer",
                tint = if (sleep != null) MaterialTheme.colorScheme.primary else LocalContentColor.current,
            )
        }
        DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            Text(
                "Sleep timer",
                style = MaterialTheme.typography.labelLarge,
                modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp),
            )
            listOf(15, 30, 45, 60).forEach { minutes ->
                DropdownMenuItem(
                    text = { Text(if (minutes == 60) "1 hour" else "$minutes minutes") },
                    onClick = {
                        open = false
                        onSleep(minutes)
                    },
                )
            }
            DropdownMenuItem(
                text = { Text("End of this song") },
                onClick = {
                    open = false
                    onSleep(null)
                },
            )
            if (sleep != null) {
                DropdownMenuItem(
                    text = { Text("Turn off") },
                    onClick = {
                        open = false
                        onCancel()
                    },
                )
            }
        }
    }
}

/**
 * The covers of the whole queue side by side. Swiping to another one plays
 * it; when the song changes by itself, the covers slide along.
 */
@Composable
private fun CoverPager(player: PlayerState, currentIndex: Int, onPlayQueueItem: (Int) -> Unit) {
    val pager = rememberPagerState(initialPage = currentIndex) { player.queue.size }
    val current by rememberUpdatedState(currentIndex)
    val play by rememberUpdatedState(onPlayQueueItem)

    LaunchedEffect(currentIndex) {
        if (pager.currentPage != currentIndex) pager.animateScrollToPage(currentIndex)
    }
    LaunchedEffect(pager) {
        snapshotFlow { pager.settledPage }.distinctUntilChanged().drop(1).collect { page ->
            if (page != current) play(page)
        }
    }

    HorizontalPager(
        state = pager,
        contentPadding = PaddingValues(horizontal = 32.dp),
        pageSpacing = 16.dp,
        modifier = Modifier.padding(vertical = 16.dp),
    ) { page ->
        val item = player.queue[page]
        Artwork(
            item.artwork,
            sizeDp = 400,
            modifier = Modifier
                .fillMaxWidth()
                .aspectRatio(1f)
                .graphicsLayer {
                    // The cover in the middle is full size; its neighbours shrink and fade.
                    val distance = pager.offsetFor(page).absoluteValue.coerceIn(0f, 1f)
                    val scale = lerp(1f, 0.85f, distance)
                    scaleX = scale
                    scaleY = scale
                    alpha = lerp(1f, 0.5f, distance)
                },
        )
    }
}

private fun PagerState.offsetFor(page: Int) = (currentPage - page) + currentPageOffsetFraction

@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
private fun Controls(
    player: PlayerState,
    playing: Boolean,
    onToggle: () -> Unit,
    onNext: () -> Unit,
    onPrevious: () -> Unit,
    onShuffle: () -> Unit,
    onRepeat: () -> Unit,
) {
    // The play button is round while paused and turns into a rounded square
    // while playing, with a little bounce.
    val corner by animateDpAsState(
        targetValue = if (playing) 28.dp else 48.dp,
        animationSpec = spring(dampingRatio = Spring.DampingRatioMediumBouncy, stiffness = Spring.StiffnessMediumLow),
        label = "play button shape",
    )
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
            modifier = Modifier.size(96.dp),
            shape = RoundedCornerShape(corner),
        ) {
            Icon(
                painterResource(if (playing) R.drawable.ic_pause else R.drawable.ic_play),
                contentDescription = if (playing) "Pause" else "Play",
                modifier = Modifier.size(44.dp),
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
