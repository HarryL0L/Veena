package dev.veena.app.ui

import androidx.compose.foundation.clickable
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
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Button
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import dev.veena.app.PlayerViewModel
import dev.veena.app.R
import java.time.LocalTime
import uniffi.veena_ffi.TrackInfo

/** Recently played, liked songs and playlists at a glance. */
@Composable
fun HomeScreen(
    viewModel: PlayerViewModel,
    actions: TrackActions,
    onOpen: (SongList) -> Unit,
    onSearch: () -> Unit,
    onSettings: () -> Unit,
) {
    val settings by viewModel.settings.collectAsStateWithLifecycle()
    val cached by viewModel.library.cached.collectAsStateWithLifecycle()
    val history by viewModel.library.history.collectAsStateWithLifecycle()
    val liked by viewModel.library.liked.collectAsStateWithLifecycle()
    val playlists by viewModel.library.playlists.collectAsStateWithLifecycle()

    LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(bottom = 24.dp + LocalMiniPlayerSpace.current)) {
        item {
            Row(Modifier.fillMaxWidth().padding(start = 24.dp, end = 8.dp, top = 16.dp, bottom = 8.dp), verticalAlignment = Alignment.CenterVertically) {
                Text(greeting(), style = MaterialTheme.typography.headlineMedium, fontWeight = FontWeight.Bold, modifier = Modifier.weight(1f))
                IconButton(onClick = onSettings) { Icon(painterResource(R.drawable.ic_settings), contentDescription = "Settings") }
            }
        }

        if (settings.offline) {
            item {
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

        if (history.isEmpty() && liked.isEmpty() && playlists.isEmpty()) {
            item {
                Column(
                    Modifier.fillMaxWidth().padding(32.dp),
                    horizontalAlignment = Alignment.CenterHorizontally,
                ) {
                    Icon(
                        painterResource(R.drawable.ic_music_note),
                        contentDescription = null,
                        modifier = Modifier.size(56.dp),
                        tint = MaterialTheme.colorScheme.primary,
                    )
                    Spacer(Modifier.size(12.dp))
                    Text("Songs you play and like will show up here.", style = MaterialTheme.typography.bodyLarge)
                    Spacer(Modifier.size(16.dp))
                    Button(onClick = onSearch) { Text("Find something to play") }
                }
            }
            return@LazyColumn
        }

        if (history.isNotEmpty()) {
            item { SectionTitle("Recently played", "See all") { onOpen(SongList.History) } }
            item {
                LazyRow(contentPadding = PaddingValues(horizontal = 16.dp), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                    itemsIndexed(history.take(20), key = { _, t -> "${t.kind}:${t.id}" }) { index, track ->
                        SongCard(track, playing = actions.isPlaying(track)) { viewModel.playList(history, index) }
                    }
                }
            }
        }

        item { SectionTitle("Your music") }
        item {
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

        if (liked.isNotEmpty()) {
            item { SectionTitle("From your liked songs", "Shuffle") { viewModel.shuffleList(liked) } }
            itemsIndexed(liked.take(5), key = { _, t -> "liked:${t.kind}:${t.id}" }) { index, track ->
                TrackRow(track, actions, onClick = { viewModel.playList(liked, index) }, showSource = false)
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
private fun SongCard(track: TrackInfo, playing: Boolean, onClick: () -> Unit) {
    Column(Modifier.width(152.dp).clickable(onClick = onClick)) {
        Artwork(track.artwork, 152)
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
    Column(Modifier.width(152.dp).clickable(onClick = onClick)) {
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

fun songs(count: Int) = if (count == 1) "1 song" else "$count songs"

private fun greeting(): String = when (LocalTime.now().hour) {
    in 5..11 -> "Good morning"
    in 12..16 -> "Good afternoon"
    else -> "Good evening"
}
