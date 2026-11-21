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
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.systemBarsPadding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.pager.HorizontalPager
import androidx.compose.foundation.pager.PagerState
import androidx.compose.foundation.pager.rememberPagerState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.FilledIconButton
import androidx.compose.material3.FilledTonalIconToggleButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Slider
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Brush
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
            LazyColumn(
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
                                "${currentIndex + 1} of ${player.queue.size}",
                                style = MaterialTheme.typography.titleSmall,
                                textAlign = TextAlign.Center,
                            )
                        }
                        Spacer(Modifier.size(48.dp))
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
                        Column(Modifier.fillMaxWidth().padding(horizontal = 32.dp)) {
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

                if (player.queue.size > 1) {
                    item {
                        Text(
                            "Up next",
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
