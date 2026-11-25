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
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.FilledIconButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearWavyProgressIndicator
import androidx.compose.material3.ListItem
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
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
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.compositionLocalOf
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
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextOverflow
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

/** Room the floating mini player takes at the bottom of the page. */
val LocalMiniPlayerSpace = compositionLocalOf { 0.dp }

/** The app's three places. */
enum class Tab(val label: String, val icon: Int) {
    HOME("Home", R.drawable.ic_home),
    SEARCH("Search", R.drawable.ic_search),
    LIBRARY("Library", R.drawable.ic_library_music),
}

@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
fun VeenaScreen(viewModel: PlayerViewModel) {
    val player by viewModel.player.collectAsStateWithLifecycle()
    val message by viewModel.messages.collectAsStateWithLifecycle()
    val sleep by viewModel.sleep.collectAsStateWithLifecycle()
    val liked by viewModel.library.liked.collectAsStateWithLifecycle()
    val lyrics by viewModel.lyrics.collectAsStateWithLifecycle()
    val snackbars = remember { SnackbarHostState() }
    var tab by rememberSaveable { mutableStateOf(Tab.HOME) }
    var expanded by rememberSaveable { mutableStateOf(false) }
    var showLogs by rememberSaveable { mutableStateOf(false) }
    var showSettings by rememberSaveable { mutableStateOf(false) }
    val cached by viewModel.library.cached.collectAsStateWithLifecycle()
    var collection by remember { mutableStateOf<SongList?>(null) }
    var addingToPlaylist by remember { mutableStateOf<TrackInfo?>(null) }

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

    val playingId = player.current?.let { player.queue.getOrNull(it.toInt()) }?.let { "${it.kind}:${it.id}" }
    // Every song row offers the same actions.
    val actions = TrackActions(
        playingId = playingId,
        isLiked = { t -> liked.any { it.kind == t.kind && it.id == t.id } },
        isCached = { t -> cached.any { it.kind == t.kind && it.id == t.id } },
        onPlayNext = viewModel::playNext,
        onAddToQueue = viewModel::addToQueue,
        onToggleLike = viewModel::toggleLike,
        onAddToPlaylist = { addingToPlaylist = it },
    )

    Scaffold(
        snackbarHost = { SnackbarHost(snackbars) },
        bottomBar = {
            Column {
                NavigationBar {
                    Tab.entries.forEach { item ->
                        NavigationBarItem(
                            selected = tab == item && collection == null && !showSettings,
                            onClick = {
                                tab = item
                                collection = null
                                showSettings = false
                            },
                            icon = { Icon(painterResource(item.icon), contentDescription = null) },
                            label = { Text(item.label) },
                        )
                    }
                }
            }
        },
    ) { padding ->
        // The mini player floats over the page, so songs scroll behind it;
        // lists and buttons leave this much room so nothing stays hidden.
        // Its real height is measured (it changes with font size), with a
        // guess until the first measurement.
        val density = LocalDensity.current
        var miniPlayerHeight by remember { mutableStateOf(0.dp) }
        val miniPlayerSpace = when {
            player.current == null -> 0.dp
            miniPlayerHeight > 0.dp -> miniPlayerHeight + 8.dp
            else -> 128.dp
        }
        // Home's sky reaches up behind the status bar; other pages start below it.
        val below = Modifier.padding(top = padding.calculateTopPadding())
        Box(Modifier.fillMaxSize().padding(bottom = padding.calculateBottomPadding())) {
          CompositionLocalProvider(LocalMiniPlayerSpace provides miniPlayerSpace) {
            val open = collection
            val onHome = !showSettings && open == null && tab == Tab.HOME
            Box(if (onHome) Modifier.fillMaxSize() else below.fillMaxSize()) {
            when {
                showSettings -> {
                    BackHandler { showSettings = false }
                    SettingsScreen(
                        viewModel = viewModel,
                        onBack = { showSettings = false },
                        onLogs = { showLogs = true },
                        onOpenSaved = {
                            showSettings = false
                            collection = SongList.Offline
                        },
                    )
                }
                open != null -> {
                    BackHandler { collection = null }
                    CollectionScreen(
                        collection = open,
                        viewModel = viewModel,
                        actions = actions,
                        onBack = { collection = null },
                    )
                }
                tab == Tab.HOME -> HomeScreen(
                    viewModel = viewModel,
                    actions = actions,
                    onOpen = { collection = it },
                    onSearch = { query ->
                        if (query != null) {
                            viewModel.onQueryChange(query)
                            viewModel.search()
                        }
                        tab = Tab.SEARCH
                    },
                    onSettings = { showSettings = true },
                )
                tab == Tab.SEARCH -> SearchScreen(viewModel, actions, onLogs = { showLogs = true })
                else -> LibraryScreen(viewModel, onOpen = { collection = it })
            }
            }
          }
            if (player.current != null) {
                NowPlayingBar(
                    player = player,
                    onToggle = viewModel::togglePause,
                    onNext = viewModel::next,
                    onPrevious = viewModel::previous,
                    onOpen = { expanded = true },
                    modifier = Modifier
                        .align(Alignment.BottomCenter)
                        .onSizeChanged { miniPlayerHeight = with(density) { it.height.toDp() } },
                )
            }
        }
    }

    if (showLogs) LogsSheet(onDismiss = { showLogs = false })
    addingToPlaylist?.let { track ->
        AddToPlaylistSheet(
            track = track,
            playlists = viewModel.library.playlists.collectAsStateWithLifecycle().value,
            onPick = { id, name ->
                viewModel.addToPlaylist(track, id, name)
                addingToPlaylist = null
            },
            onDismiss = { addingToPlaylist = null },
        )
    }

    AnimatedVisibility(
        visible = expanded && player.current != null,
        enter = slideInVertically { it },
        exit = slideOutVertically { it },
    ) {
        BackHandler { expanded = false }
        val current = player.current?.let { player.queue.getOrNull(it.toInt()) }
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
            liked = current != null && actions.isLiked(current),
            onToggleLike = { current?.let(viewModel::toggleLike) },
            onAddToPlaylist = { addingToPlaylist = current },
            lyrics = lyrics,
        )
    }
}

/** What a song row can do, shared by every list of songs. */
class TrackActions(
    val playingId: String?,
    val isLiked: (TrackInfo) -> Boolean,
    /** Kept on this phone for offline listening. */
    val isCached: (TrackInfo) -> Boolean,
    val onPlayNext: (TrackInfo) -> Unit,
    val onAddToQueue: (TrackInfo) -> Unit,
    val onToggleLike: (TrackInfo) -> Unit,
    val onAddToPlaylist: (TrackInfo) -> Unit,
) {
    fun isPlaying(track: TrackInfo) = playingId == "${track.kind}:${track.id}"
}

/** Search, with Music and Videos sections. */
@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
private fun SearchScreen(viewModel: PlayerViewModel, actions: TrackActions, onLogs: () -> Unit) {
    val search by viewModel.search.collectAsStateWithLifecycle()
    val offline = viewModel.settings.collectAsStateWithLifecycle().value.offline
    Column(Modifier.fillMaxSize()) {
        Column(Modifier.padding(horizontal = 16.dp).padding(top = 8.dp, bottom = 8.dp)) {
            OutlinedTextField(
                value = search.query,
                onValueChange = viewModel::onQueryChange,
                modifier = Modifier.fillMaxWidth(),
                placeholder = { Text(if (offline) "Search songs kept on this phone" else "Search songs") },
                leadingIcon = { Icon(painterResource(R.drawable.ic_search), contentDescription = null) },
                trailingIcon = {
                    IconButton(onClick = onLogs) {
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
        Box(Modifier.fillMaxSize()) {
            when {
                search.searching -> LoadingIndicator(Modifier.align(Alignment.Center).size(64.dp))
                search.results.isEmpty() -> EmptyState(Modifier.align(Alignment.Center))
                search.shown.isEmpty() -> Text(
                    "No ${search.section.label.lowercase()} found",
                    modifier = Modifier.align(Alignment.Center),
                    style = MaterialTheme.typography.titleMedium,
                )
                else -> LazyColumn(contentPadding = PaddingValues(bottom = 16.dp + LocalMiniPlayerSpace.current)) {
                    itemsIndexed(search.shown, key = { i, t -> "$i:${t.kind}:${t.id}" }) { index, track ->
                        TrackRow(track, actions, onClick = { viewModel.play(index) })
                    }
                }
            }
        }
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

/**
 * A song in a list. Tap to play; hold (or tap ⋮) for more. [extra] adds
 * actions that only make sense in one list, such as removing from a playlist.
 */
@Composable
fun TrackRow(
    track: TrackInfo,
    actions: TrackActions,
    onClick: () -> Unit,
    showSource: Boolean = true,
    extra: List<Pair<String, () -> Unit>> = emptyList(),
) {
    var menu by remember { mutableStateOf(false) }
    val playing = actions.isPlaying(track)
    Box {
        ListItem(
            selected = playing,
            onClick = onClick,
            onLongClick = { menu = true },
            onLongClickLabel = "More options",
            modifier = Modifier.padding(horizontal = 8.dp),
            supportingContent = {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    if (actions.isCached(track)) {
                        Icon(
                            painterResource(R.drawable.ic_offline_pin),
                            contentDescription = "Kept on this phone",
                            tint = MaterialTheme.colorScheme.primary,
                            modifier = Modifier.size(16.dp).padding(end = 2.dp),
                        )
                        Spacer(Modifier.width(4.dp))
                    }
                    Text(
                        listOfNotNull(track.artist, track.source.takeIf { showSource && it.isNotEmpty() }).joinToString(" · "),
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
            },
            leadingContent = {
                Box {
                    Artwork(track.artwork, 52)
                    if (playing) PlayingOverlay(Modifier.matchParentSize().clip(RoundedCornerShape(13.dp)))
                }
            },
            trailingContent = {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    track.durationMs?.let { Text(formatTime(it.toLong()), style = MaterialTheme.typography.labelMedium) }
                    IconButton(onClick = { menu = true }) {
                        Icon(painterResource(R.drawable.ic_more_vert), contentDescription = "More options")
                    }
                }
            },
        ) {
            Text(
                track.title,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                fontWeight = if (playing) FontWeight.Bold else null,
            )
        }
        if (menu) {
            val liked = actions.isLiked(track)
            SongSheet(
                track = track,
                actions = listOf(
                    SongAction("Play next", R.drawable.ic_skip_next) { actions.onPlayNext(track) },
                    SongAction("Add to queue", R.drawable.ic_queue_music) { actions.onAddToQueue(track) },
                    SongAction(
                        if (liked) "Remove from liked" else "Like",
                        if (liked) R.drawable.ic_favorite else R.drawable.ic_favorite_border,
                    ) { actions.onToggleLike(track) },
                    SongAction("Add to playlist…", R.drawable.ic_playlist_add) { actions.onAddToPlaylist(track) },
                ) + extra.map { (label, action) -> SongAction(label, REMOVE_ICON, action) },
                onDismiss = { menu = false },
            )
        }
    }
}

@Composable
internal fun Artwork(url: String?, sizeDp: Int, modifier: Modifier = Modifier.size(sizeDp.dp)) {
    val shape = RoundedCornerShape((sizeDp / 4).coerceAtMost(28).dp)
    // Not every video has the large thumbnail; fall back to the standard one.
    var source by remember(url) { mutableStateOf(url) }
    if (source != null) {
        AsyncImage(
            model = source,
            contentDescription = null,
            contentScale = ContentScale.Crop,
            modifier = modifier.clip(shape),
            onError = {
                if (source?.contains("/hq720.jpg") == true) source = source!!.replace("/hq720.jpg", "/hqdefault.jpg")
            },
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
    modifier: Modifier = Modifier,
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
        modifier = modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 8.dp).then(gestures),
        shadowElevation = 6.dp,
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
