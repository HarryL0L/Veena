package dev.veena.app.ui

import androidx.activity.compose.BackHandler
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.slideInVertically
import androidx.compose.animation.slideOutVertically
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.detectDragGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.FilledIconButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearWavyProgressIndicator
import androidx.compose.material3.ListItem
import androidx.compose.material3.LoadingIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.SnackbarDuration
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarResult
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.DpOffset
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import coil3.compose.AsyncImage
import dev.veena.app.PlayerViewModel
import dev.veena.app.R
import dev.veena.app.Section
import uniffi.veena_ffi.PlayState
import uniffi.veena_ffi.PlayerState
import uniffi.veena_ffi.TrackInfo
import kotlin.math.absoluteValue

@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
fun VeenaScreen(viewModel: PlayerViewModel) {
    val search by viewModel.search.collectAsStateWithLifecycle()
    val player by viewModel.player.collectAsStateWithLifecycle()
    val message by viewModel.messages.collectAsStateWithLifecycle()
    val sleep by viewModel.sleep.collectAsStateWithLifecycle()
    val snackbars = remember { SnackbarHostState() }
    var expanded by rememberSaveable { mutableStateOf(false) }
    var showLogs by rememberSaveable { mutableStateOf(false) }

    LaunchedEffect(message) {
        message?.let {
            val result = if (it.error) {
                snackbars.showSnackbar(it.text, actionLabel = "Details", duration = SnackbarDuration.Long)
            } else {
                snackbars.showSnackbar(it.text, duration = SnackbarDuration.Short)
            }
            if (result == SnackbarResult.ActionPerformed) showLogs = true
            viewModel.messageShown()
        }
    }

    Scaffold(
        snackbarHost = { SnackbarHost(snackbars) },
        topBar = {
            Column(Modifier.padding(horizontal = 16.dp).padding(top = 48.dp, bottom = 8.dp)) {
                OutlinedTextField(
                    value = search.query,
                    onValueChange = viewModel::onQueryChange,
                    modifier = Modifier.fillMaxWidth(),
                    placeholder = { Text("Search songs") },
                    leadingIcon = { Icon(painterResource(R.drawable.ic_search), contentDescription = null) },
                    trailingIcon = {
                        IconButton(onClick = { showLogs = true }) {
                            Icon(painterResource(R.drawable.ic_bug), contentDescription = "Logs")
                        }
                    },
                    singleLine = true,
                    shape = RoundedCornerShape(28.dp),
                    keyboardOptions = KeyboardOptions(imeAction = ImeAction.Search),
                    keyboardActions = KeyboardActions(onSearch = { viewModel.search() }),
                )
                Spacer(Modifier.size(8.dp))
                SingleChoiceSegmentedButtonRow(Modifier.fillMaxWidth()) {
                    Section.entries.forEachIndexed { index, section ->
                        SegmentedButton(
                            selected = search.section == section,
                            onClick = { viewModel.onSectionChange(section) },
                            shape = SegmentedButtonDefaults.itemShape(index, Section.entries.size),
                        ) {
                            Text(section.label)
                        }
                    }
                }
            }
        },
        bottomBar = {
            if (player.current != null) {
                NowPlayingBar(
                    player = player,
                    onToggle = viewModel::togglePause,
                    onNext = viewModel::next,
                    onPrevious = viewModel::previous,
                    onOpen = { expanded = true },
                )
            }
        },
    ) { padding ->
        Box(Modifier.fillMaxSize().padding(padding)) {
            when {
                search.searching -> LoadingIndicator(Modifier.align(Alignment.Center).size(64.dp))
                search.results.isEmpty() -> EmptyState(Modifier.align(Alignment.Center))
                search.shown.isEmpty() -> Text(
                    "No ${search.section.label.lowercase()} found",
                    modifier = Modifier.align(Alignment.Center),
                    style = MaterialTheme.typography.titleMedium,
                )
                else -> LazyColumn(contentPadding = PaddingValues(bottom = 16.dp)) {
                    itemsIndexed(search.shown, key = { i, t -> "$i:${t.kind}:${t.id}" }) { index, track ->
                        TrackRow(
                            track = track,
                            playing = player.queue.getOrNull(player.current?.toInt() ?: -1)?.id == track.id,
                            onClick = { viewModel.play(index) },
                            onPlayNext = { viewModel.playNext(track) },
                            onAddToQueue = { viewModel.addToQueue(track) },
                        )
                    }
                }
            }
        }
    }

    if (showLogs) LogsSheet(onDismiss = { showLogs = false })

    AnimatedVisibility(
        visible = expanded && player.current != null,
        enter = slideInVertically { it },
        exit = slideOutVertically { it },
    ) {
        BackHandler { expanded = false }
        PlayerScreen(
            player = player,
            onClose = { expanded = false },
            onToggle = viewModel::togglePause,
            onNext = viewModel::next,
            onPrevious = viewModel::previous,
            onSeek = viewModel::seek,
            onShuffle = viewModel::toggleShuffle,
            onRepeat = viewModel::cycleRepeat,
            onPlayQueueItem = viewModel::playQueueItem,
            onRemove = viewModel::removeFromQueue,
            onMove = viewModel::moveInQueue,
            sleep = sleep,
            onSleep = viewModel::setSleepTimer,
            onCancelSleep = viewModel::cancelSleepTimer,
        )
    }
}

@Composable
private fun EmptyState(modifier: Modifier = Modifier) {
    Column(modifier, horizontalAlignment = Alignment.CenterHorizontally) {
        Icon(
            painterResource(R.drawable.ic_music_note),
            contentDescription = null,
            modifier = Modifier.size(56.dp),
            tint = MaterialTheme.colorScheme.primary,
        )
        Spacer(Modifier.size(12.dp))
        Text("Search for something to play", style = MaterialTheme.typography.titleMedium)
    }
}

/** A search result. Tap to play; hold for more. */
@Composable
private fun TrackRow(
    track: TrackInfo,
    playing: Boolean,
    onClick: () -> Unit,
    onPlayNext: () -> Unit,
    onAddToQueue: () -> Unit,
) {
    var menu by remember { mutableStateOf(false) }
    Box {
        ListItem(
            selected = playing,
            onClick = onClick,
            onLongClick = { menu = true },
            onLongClickLabel = "More options",
            modifier = Modifier.padding(horizontal = 8.dp),
            supportingContent = {
                Text(
                    listOfNotNull(track.artist, track.source.ifEmpty { null }).joinToString(" · "),
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            },
            leadingContent = { Artwork(track.artwork, 52) },
            trailingContent = { track.durationMs?.let { Text(formatTime(it.toLong())) } },
        ) {
            Text(
                track.title,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                fontWeight = if (playing) FontWeight.Bold else null,
            )
        }
        DropdownMenu(expanded = menu, onDismissRequest = { menu = false }, offset = DpOffset(24.dp, 0.dp)) {
            DropdownMenuItem(
                text = { Text("Play next") },
                leadingIcon = { Icon(painterResource(R.drawable.ic_skip_next), contentDescription = null) },
                onClick = {
                    menu = false
                    onPlayNext()
                },
            )
            DropdownMenuItem(
                text = { Text("Add to queue") },
                leadingIcon = { Icon(painterResource(R.drawable.ic_queue_music), contentDescription = null) },
                onClick = {
                    menu = false
                    onAddToQueue()
                },
            )
        }
    }
}

@Composable
internal fun Artwork(url: String?, sizeDp: Int, modifier: Modifier = Modifier.size(sizeDp.dp)) {
    val shape = RoundedCornerShape((sizeDp / 4).coerceAtMost(28).dp)
    if (url != null) {
        AsyncImage(
            model = url,
            contentDescription = null,
            contentScale = ContentScale.Crop,
            modifier = modifier.clip(shape),
        )
    } else {
        Box(
            modifier.clip(shape).background(MaterialTheme.colorScheme.surfaceVariant),
            contentAlignment = Alignment.Center,
        ) {
            Icon(painterResource(R.drawable.ic_music_note), contentDescription = null)
        }
    }
}

@Composable
private fun NowPlayingBar(
    player: PlayerState,
    onToggle: () -> Unit,
    onNext: () -> Unit,
    onPrevious: () -> Unit,
    onOpen: () -> Unit,
) {
    val track = player.queue.getOrNull(player.current?.toInt() ?: -1) ?: return
    val duration = player.durationMs?.toLong() ?: track.durationMs?.toLong()
    val density = LocalDensity.current
    // Drag up to open the full player; swipe sideways to change songs.
    val gestures = Modifier.pointerInput(Unit) {
        var dx = 0f
        var dy = 0f
        val threshold = with(density) { 56.dp.toPx() }
        detectDragGestures(
            onDragStart = { dx = 0f; dy = 0f },
            onDrag = { change, amount ->
                change.consume()
                dx += amount.x
                dy += amount.y
            },
            onDragEnd = {
                when {
                    dy.absoluteValue > dx.absoluteValue && dy < -threshold -> onOpen()
                    dx.absoluteValue > dy.absoluteValue && dx < -threshold -> onNext()
                    dx.absoluteValue > dy.absoluteValue && dx > threshold -> onPrevious()
                }
            },
        )
    }
    ArtworkTheme(track.artwork) {
    Surface(
        onClick = onOpen,
        modifier = Modifier.fillMaxWidth().navigationBarsPadding().padding(12.dp).then(gestures),
        shape = RoundedCornerShape(32.dp),
        color = MaterialTheme.colorScheme.primaryContainer,
        contentColor = MaterialTheme.colorScheme.onPrimaryContainer,
        tonalElevation = 3.dp,
    ) {
        Column(Modifier.padding(horizontal = 16.dp, vertical = 12.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Artwork(track.artwork, 48)
                Spacer(Modifier.width(12.dp))
                Column(Modifier.weight(1f)) {
                    Text(track.title, maxLines = 1, overflow = TextOverflow.Ellipsis, style = MaterialTheme.typography.titleMedium)
                    Text(
                        track.artist ?: "",
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                        style = MaterialTheme.typography.bodyMedium,
                    )
                }
                IconButton(onClick = onPrevious) { Icon(painterResource(R.drawable.ic_skip_previous), contentDescription = "Previous") }
                FilledIconButton(onClick = onToggle, modifier = Modifier.size(52.dp)) {
                    Icon(
                        painterResource(if (player.state == PlayState.PLAYING) R.drawable.ic_pause else R.drawable.ic_play),
                        contentDescription = if (player.state == PlayState.PLAYING) "Pause" else "Play",
                    )
                }
                IconButton(onClick = onNext) { Icon(painterResource(R.drawable.ic_skip_next), contentDescription = "Next") }
            }
            Spacer(Modifier.size(10.dp))
            val progress = if (duration != null && duration > 0) {
                (player.positionMs.toLong().toFloat() / duration).coerceIn(0f, 1f)
            } else {
                0f
            }
            LinearWavyProgressIndicator(progress = { progress }, modifier = Modifier.fillMaxWidth())
            Row(Modifier.fillMaxWidth().padding(top = 4.dp), horizontalArrangement = Arrangement.SpaceBetween) {
                Text(formatTime(player.positionMs.toLong()), style = MaterialTheme.typography.labelSmall)
                Text(
                    if (player.state == PlayState.LOADING) "Loading…" else duration?.let { formatTime(it) } ?: "",
                    style = MaterialTheme.typography.labelSmall,
                )
            }
        }
    }
}
}

internal fun formatTime(ms: Long): String {
    val total = ms / 1000
    val (h, m, s) = Triple(total / 3600, total / 60 % 60, total % 60)
    return if (h > 0) "%d:%02d:%02d".format(h, m, s) else "%d:%02d".format(m, s)
}
