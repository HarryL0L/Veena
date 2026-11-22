package dev.veena.app.ui

import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.LocalIndication
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsPressedAsState
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.asPaddingValues
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Button
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.FilledIconButton
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialShapes
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.SuggestionChip
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.toShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import dev.veena.app.DayPart
import dev.veena.app.PlayerViewModel
import dev.veena.app.R
import dev.veena.app.Season
import java.time.format.DateTimeFormatter
import java.util.Locale
import uniffi.veena_ffi.TrackInfo

/**
 * Home: a living sky that follows the time and season, then what to play
 * next — the last song, quick picks, recent plays, moods for this moment,
 * your collections and genres to explore.
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
fun HomeScreen(
    viewModel: PlayerViewModel,
    actions: TrackActions,
    onOpen: (SongList) -> Unit,
    onSearch: (String?) -> Unit,
    onSettings: () -> Unit,
) {
    val settings by viewModel.settings.collectAsStateWithLifecycle()
    val cached by viewModel.library.cached.collectAsStateWithLifecycle()
    val history by viewModel.library.history.collectAsStateWithLifecycle()
    val liked by viewModel.library.liked.collectAsStateWithLifecycle()
    val playlists by viewModel.library.playlists.collectAsStateWithLifecycle()
    val ambience = rememberAmbience(settings)
    val empty = history.isEmpty() && liked.isEmpty() && playlists.isEmpty()

    LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(bottom = 24.dp + LocalMiniPlayerSpace.current)) {
        item(key = "header") { HomeHeader(ambience, animated = settings.ambience, onSettings = onSettings) }

        if (settings.offline) {
            item(key = "offline") {
                Surface(
                    onClick = { onOpen(SongList.Offline) },
                    modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp),
                    shape = RoundedCornerShape(24.dp),
                    color = MaterialTheme.colorScheme.tertiaryContainer,
                ) {
                    Row(Modifier.padding(16.dp), verticalAlignment = Alignment.CenterVertically) {
                        Icon(painterResource(R.drawable.ic_cloud_off), contentDescription = null)
                        Spacer(Modifier.size(12.dp))
                        Column(Modifier.weight(1f)) {
                            Text("Offline mode", style = MaterialTheme.typography.titleMedium)
                            Text(
                                "Playing only the ${songs(cached.size)} kept on this phone. Turn it off in Settings.",
                                style = MaterialTheme.typography.bodyMedium,
                            )
                        }
                    }
                }
            }
        }

        // The last song played, unless it's playing now.
        val last = history.firstOrNull()
        if (last != null && !actions.isPlaying(last)) {
            item(key = "jump") { JumpBackIn(last) { viewModel.playList(history, 0) } }
        }

        // Up to six songs from what you play and like.
        val picks = (history + liked).distinctBy { "${it.kind}:${it.id}" }.take(6)
        if (picks.size >= 2) {
            item(key = "picks-title") { SectionTitle("Quick picks") }
            item(key = "picks") {
                Column(Modifier.padding(horizontal = 16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    picks.chunked(2).forEachIndexed { row, pair ->
                        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                            pair.forEachIndexed { col, track ->
                                QuickPick(track, actions.isPlaying(track), Modifier.weight(1f)) {
                                    viewModel.playList(picks, row * 2 + col)
                                }
                            }
                            if (pair.size == 1) Spacer(Modifier.weight(1f))
                        }
                    }
                }
            }
        }

        if (history.isNotEmpty()) {
            item(key = "recent-title") { SectionTitle("Recently played", "See all") { onOpen(SongList.History) } }
            item(key = "recent") {
                LazyRow(contentPadding = PaddingValues(horizontal = 16.dp), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                    itemsIndexed(history.take(20), key = { _, t -> "${t.kind}:${t.id}" }) { index, track ->
                        SongCard(
                            track,
                            playing = actions.isPlaying(track),
                            actions = actions,
                            onRemove = { viewModel.library.removeFromHistory(track) },
                        ) { viewModel.playList(history, index) }
                    }
                }
            }
        }

        item(key = "moods-title") { SectionTitle(if (empty) "Start with a mood" else "For this ${momentName(ambience)}") }
        item(key = "moods") {
            FlowRow(
                Modifier.padding(horizontal = 16.dp),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                moodsFor(ambience).forEach { mood ->
                    SuggestionChip(onClick = { onSearch(mood) }, label = { Text(mood) })
                }
            }
        }

        if (!empty) {
            item(key = "mine-title") { SectionTitle("Your music") }
            item(key = "mine") {
                LazyRow(contentPadding = PaddingValues(horizontal = 16.dp), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                    item {
                        CollectionCard(
                            title = "Liked songs",
                            subtitle = songs(liked.size),
                            artwork = liked.firstOrNull()?.artwork,
                            icon = R.drawable.ic_favorite,
                        ) { onOpen(SongList.Liked) }
                    }
                    if (cached.isNotEmpty()) {
                        item {
                            CollectionCard(
                                title = "Kept on this phone",
                                subtitle = songs(cached.size),
                                artwork = cached.firstOrNull()?.artwork,
                                icon = R.drawable.ic_offline_pin,
                            ) { onOpen(SongList.Offline) }
                        }
                    }
                    items(playlists, key = { it.id }) { list ->
                        CollectionCard(
                            title = list.name,
                            subtitle = songs(list.count.toInt()),
                            artwork = list.artwork,
                            icon = R.drawable.ic_queue_music,
                        ) { onOpen(SongList.Playlist(list.id, list.name)) }
                    }
                }
            }
        }

        item(key = "explore-title") { SectionTitle("Explore") }
        item(key = "explore") { ExploreGrid(onSearch) }

        if (liked.isNotEmpty()) {
            item(key = "liked-title") { SectionTitle("From your liked songs", "Shuffle") { viewModel.shuffleList(liked) } }
            itemsIndexed(liked.take(5), key = { _, t -> "liked:${t.kind}:${t.id}" }) { index, track ->
                TrackRow(track, actions, onClick = { viewModel.playList(liked, index) }, showSource = false)
            }
        }

        if (empty) {
            item(key = "start") {
                Box(Modifier.fillMaxWidth().padding(24.dp), contentAlignment = Alignment.Center) {
                    Button(onClick = { onSearch(null) }) { Text("Search for something to play") }
                }
            }
        }
    }
}

/** The greeting over the living sky, which reaches up behind the status bar. */
@Composable
private fun HomeHeader(ambience: Ambience, animated: Boolean, onSettings: () -> Unit) {
    val top = WindowInsets.statusBars.asPaddingValues().calculateTopPadding()
    Box(Modifier.fillMaxWidth().height(232.dp + top)) {
        if (animated) AmbientScene(ambience, Modifier.matchParentSize())
        Surface(
            onClick = onSettings,
            modifier = Modifier.align(Alignment.TopEnd).padding(top = top + 8.dp, end = 12.dp).size(44.dp),
            shape = CircleShape,
            color = MaterialTheme.colorScheme.surfaceContainerHigh.copy(alpha = 0.72f),
        ) {
            Box(contentAlignment = Alignment.Center) {
                Icon(painterResource(R.drawable.ic_settings), contentDescription = "Settings", modifier = Modifier.size(22.dp))
            }
        }
        Column(Modifier.align(Alignment.BottomStart).padding(start = 24.dp, end = 24.dp, bottom = 4.dp)) {
            Text(
                dateLine(ambience),
                style = MaterialTheme.typography.labelLarge,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Text(
                ambience.greeting,
                style = MaterialTheme.typography.displaySmall,
                fontWeight = FontWeight.Bold,
                color = MaterialTheme.colorScheme.onSurface,
            )
            Text(
                ambience.line,
                style = MaterialTheme.typography.bodyLarge,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

private fun dateLine(a: Ambience): String {
    val day = a.date.format(DateTimeFormatter.ofPattern("EEEE, d MMMM", Locale.getDefault()))
    return "$day · ${a.season.label}"
}

private fun momentName(a: Ambience): String = when (a.dayPart) {
    DayPart.DAWN -> "morning"
    DayPart.DAY -> if (a.hour < 12f) "morning" else "afternoon"
    DayPart.DUSK -> "evening"
    DayPart.NIGHT -> "night"
}

/** Searches that suit the moment: the time of day, then the season or special day. */
private fun moodsFor(a: Ambience): List<String> {
    val time = when {
        a.hour >= 5f && a.hour < 12f -> listOf("Morning acoustic", "Coffee jazz", "Upbeat pop", "Morning chill")
        a.hour >= 12f && a.hour < 17f -> listOf("Focus beats", "Feel-good hits", "Indie", "Workout")
        a.hour >= 17f && a.hour < 22f -> listOf("Sunset chill", "Evening jazz", "Retro classics", "Road trip")
        else -> listOf("Late night lo-fi", "Soft piano", "Ambient", "Sleep")
    }
    val extra = when (a.occasion) {
        Occasion.NEW_YEAR -> listOf("Party anthems", "Countdown hits")
        null -> listOf(
            when (a.season) {
                Season.SPRING -> "Spring vibes"
                Season.SUMMER -> "Summer hits"
                Season.AUTUMN -> "Autumn acoustic"
                Season.WINTER -> "Cozy winter"
            },
        )
    }
    return extra + time
}

/** A large card to carry on with the last song. */
@Composable
private fun JumpBackIn(track: TrackInfo, onPlay: () -> Unit) {
    val scheme = MaterialTheme.colorScheme
    val press = remember { MutableInteractionSource() }
    Surface(
        onClick = onPlay,
        interactionSource = press,
        modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp).pressScale(press),
        shape = RoundedCornerShape(28.dp),
        color = Color.Transparent,
    ) {
        Row(
            Modifier
                .fillMaxWidth()
                .background(Brush.linearGradient(listOf(scheme.primaryContainer, scheme.tertiaryContainer)))
                .padding(14.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Artwork(track.artwork, 84)
            Column(Modifier.weight(1f).padding(horizontal = 14.dp)) {
                Text("Jump back in", style = MaterialTheme.typography.labelLarge, color = scheme.onPrimaryContainer.copy(alpha = 0.8f))
                Text(
                    track.title,
                    style = MaterialTheme.typography.titleMedium,
                    fontWeight = FontWeight.SemiBold,
                    color = scheme.onPrimaryContainer,
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis,
                )
                Text(
                    track.artist ?: "",
                    style = MaterialTheme.typography.bodyMedium,
                    color = scheme.onPrimaryContainer.copy(alpha = 0.8f),
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
            FilledIconButton(onClick = onPlay, modifier = Modifier.size(52.dp)) {
                Icon(painterResource(R.drawable.ic_play), contentDescription = "Play")
            }
        }
    }
}

/** A compact tile: small artwork and the title, two to a row. */
@Composable
private fun QuickPick(track: TrackInfo, playing: Boolean, modifier: Modifier, onClick: () -> Unit) {
    val press = remember { MutableInteractionSource() }
    Surface(
        onClick = onClick,
        interactionSource = press,
        modifier = modifier.height(60.dp).pressScale(press),
        shape = RoundedCornerShape(16.dp),
        color = if (playing) MaterialTheme.colorScheme.secondaryContainer else MaterialTheme.colorScheme.surfaceContainerHigh,
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Box {
                Artwork(track.artwork, 60, Modifier.size(60.dp))
                if (playing) PlayingOverlay(Modifier.matchParentSize().clip(RoundedCornerShape(15.dp)))
            }
            Text(
                track.title,
                style = MaterialTheme.typography.labelLarge,
                fontWeight = if (playing) FontWeight.Bold else FontWeight.Medium,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.padding(horizontal = 10.dp),
            )
        }
    }
}

private data class Genre(val name: String, val query: String, val shape: Shape)

/** Genre tiles, each with a slowly turning Material shape. */
@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
private fun ExploreGrid(onSearch: (String?) -> Unit) {
    val genres = listOf(
            Genre("Pop", "pop hits", MaterialShapes.Sunny.toShape()),
            Genre("Hip hop", "hip hop", MaterialShapes.Clover4Leaf.toShape()),
            Genre("Rock", "rock classics", MaterialShapes.SoftBurst.toShape()),
            Genre("Electronic", "electronic dance", MaterialShapes.Gem.toShape()),
            Genre("R&B", "r&b soul", MaterialShapes.Flower.toShape()),
            Genre("Jazz", "jazz classics", MaterialShapes.Cookie9Sided.toShape()),
            Genre("Classical", "classical music", MaterialShapes.Cookie6Sided.toShape()),
            Genre("Lo-fi", "lofi beats", MaterialShapes.Puffy.toShape()),
    )
    val scheme = MaterialTheme.colorScheme
    val colors = listOf(
        scheme.primaryContainer to scheme.onPrimaryContainer,
        scheme.secondaryContainer to scheme.onSecondaryContainer,
        scheme.tertiaryContainer to scheme.onTertiaryContainer,
        scheme.surfaceContainerHighest to scheme.onSurface,
    )
    val spin by rememberInfiniteTransition(label = "explore").animateFloat(
        initialValue = 0f,
        targetValue = 360f,
        animationSpec = infiniteRepeatable(tween(24_000, easing = LinearEasing)),
        label = "spin",
    )
    Column(Modifier.padding(horizontal = 16.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
        genres.chunked(2).forEachIndexed { row, pair ->
            Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                pair.forEachIndexed { col, genre ->
                    val i = row * 2 + col
                    val (container, content) = colors[(i + row) % colors.size]
                    val press = remember { MutableInteractionSource() }
                    Surface(
                        onClick = { onSearch(genre.query) },
                        interactionSource = press,
                        modifier = Modifier.weight(1f).height(92.dp).pressScale(press),
                        shape = RoundedCornerShape(24.dp),
                        color = container,
                    ) {
                        Box {
                            Box(
                                Modifier
                                    .size(84.dp)
                                    .align(Alignment.BottomEnd)
                                    .offset(x = 18.dp, y = 22.dp)
                                    .graphicsLayer { rotationZ = if (i % 2 == 0) spin else -spin }
                                    .clip(genre.shape)
                                    .background(content.copy(alpha = 0.16f)),
                            )
                            Text(
                                genre.name,
                                style = MaterialTheme.typography.titleMedium,
                                fontWeight = FontWeight.Bold,
                                color = content,
                                modifier = Modifier.padding(16.dp),
                            )
                        }
                    }
                }
            }
        }
    }
}

@Composable
fun SectionTitle(title: String, action: String? = null, onAction: () -> Unit = {}) {
    Row(
        Modifier.fillMaxWidth().padding(start = 24.dp, end = 12.dp, top = 20.dp, bottom = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(title, style = MaterialTheme.typography.titleLarge, fontWeight = FontWeight.SemiBold, modifier = Modifier.weight(1f))
        if (action != null) TextButton(onClick = onAction) { Text(action) }
    }
}

@Composable
private fun SongCard(
    track: TrackInfo,
    playing: Boolean,
    actions: TrackActions,
    onRemove: () -> Unit,
    onClick: () -> Unit,
) {
    val press = remember { MutableInteractionSource() }
    var sheet by remember { mutableStateOf(false) }
    if (sheet) {
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
                SongAction("Remove from recently played", REMOVE_ICON, onRemove),
            ),
            onDismiss = { sheet = false },
        )
    }
    Column(
        Modifier
            .width(152.dp)
            .pressScale(press)
            .combinedClickable(
                interactionSource = press,
                indication = LocalIndication.current,
                onLongClickLabel = "More options",
                onLongClick = { sheet = true },
                onClick = onClick,
            ),
    ) {
        Box {
            Artwork(track.artwork, 152)
            if (playing) PlayingOverlay(Modifier.matchParentSize().clip(RoundedCornerShape(28.dp)))
        }
        Text(
            track.title,
            style = MaterialTheme.typography.titleSmall,
            fontWeight = if (playing) FontWeight.Bold else FontWeight.Medium,
            color = if (playing) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurface,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.padding(top = 8.dp),
        )
        Text(
            track.artist ?: "",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
    }
}

@Composable
fun CollectionCard(title: String, subtitle: String, artwork: String?, icon: Int, onClick: () -> Unit) {
    val press = remember { MutableInteractionSource() }
    Column(Modifier.width(152.dp).pressScale(press).clickable(press, LocalIndication.current, onClick = onClick)) {
        if (artwork != null) {
            Artwork(artwork, 152)
        } else {
            Surface(Modifier.size(152.dp), shape = RoundedCornerShape(28.dp), color = MaterialTheme.colorScheme.primaryContainer) {
                Box(contentAlignment = Alignment.Center) {
                    Icon(painterResource(icon), contentDescription = null, modifier = Modifier.size(48.dp))
                }
            }
        }
        Text(title, style = MaterialTheme.typography.titleSmall, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.padding(top = 8.dp))
        Text(subtitle, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
    }
}

/** A dark veil with dancing bars, over the artwork of the song that's on. */
@Composable
fun PlayingOverlay(modifier: Modifier = Modifier) {
    Box(modifier.background(Color.Black.copy(alpha = 0.38f)), contentAlignment = Alignment.Center) {
        PlayingBars(Color.White, Modifier.size(22.dp))
    }
}

/** Three bars bouncing like a level meter. */
@Composable
fun PlayingBars(color: Color, modifier: Modifier = Modifier) {
    val transition = rememberInfiniteTransition(label = "bars")
    val heights = listOf(420, 560, 360).map { ms ->
        transition.animateFloat(
            initialValue = 0.25f,
            targetValue = 1f,
            animationSpec = infiniteRepeatable(tween(ms), RepeatMode.Reverse),
            label = "bar",
        )
    }
    Row(modifier, horizontalArrangement = Arrangement.spacedBy(3.dp), verticalAlignment = Alignment.Bottom) {
        heights.forEach { h ->
            Box(
                Modifier
                    .weight(1f)
                    .fillMaxHeight(h.value)
                    .clip(RoundedCornerShape(2.dp))
                    .background(color),
            )
        }
    }
}

/** Shrinks a little while [source] is pressed, and springs back. */
@Composable
fun Modifier.pressScale(source: MutableInteractionSource): Modifier {
    val pressed by source.collectIsPressedAsState()
    val scale by animateFloatAsState(if (pressed) 0.95f else 1f, label = "press")
    return this.graphicsLayer {
        scaleX = scale
        scaleY = scale
    }
}

fun songs(count: Int) = if (count == 1) "1 song" else "$count songs"
