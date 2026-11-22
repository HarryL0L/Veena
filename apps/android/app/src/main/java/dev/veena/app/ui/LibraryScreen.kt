package dev.veena.app.ui

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
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ExtendedFloatingActionButton
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import dev.veena.app.PlayerViewModel
import dev.veena.app.R
import kotlinx.coroutines.launch
import uniffi.veena_ffi.PlaylistInfo
import uniffi.veena_ffi.TrackInfo

/** A list of songs the app can open: liked songs, history or a playlist. */
sealed interface SongList {
    val title: String

    data object Liked : SongList {
        override val title = "Liked songs"
    }

    data object History : SongList {
        override val title = "Recently played"
    }

    data object Offline : SongList {
        override val title = "Kept on this phone"
    }

    data class Playlist(val id: String, override val title: String) : SongList
}

/** Liked songs and playlists. */
@Composable
fun LibraryScreen(viewModel: PlayerViewModel, onOpen: (SongList) -> Unit) {
    val liked by viewModel.library.liked.collectAsStateWithLifecycle()
    val history by viewModel.library.history.collectAsStateWithLifecycle()
    val playlists by viewModel.library.playlists.collectAsStateWithLifecycle()
    val cached by viewModel.library.cached.collectAsStateWithLifecycle()
    var creating by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()

    Box(Modifier.fillMaxSize()) {
        LazyColumn(contentPadding = PaddingValues(bottom = 96.dp + LocalMiniPlayerSpace.current)) {
            item {
                Text(
                    "Library",
                    style = MaterialTheme.typography.headlineMedium,
                    fontWeight = FontWeight.Bold,
                    modifier = Modifier.padding(start = 24.dp, top = 16.dp, bottom = 8.dp),
                )
            }
            item {
                LibraryRow("Liked songs", songs(liked.size), liked.firstOrNull()?.artwork, R.drawable.ic_favorite) {
                    onOpen(SongList.Liked)
                }
            }
            item {
                LibraryRow("Recently played", songs(history.size), history.firstOrNull()?.artwork, R.drawable.ic_bedtime) {
                    onOpen(SongList.History)
                }
            }
            item {
                LibraryRow("Kept on this phone", songs(cached.size), cached.firstOrNull()?.artwork, R.drawable.ic_offline_pin) {
                    onOpen(SongList.Offline)
                }
            }
            if (playlists.isNotEmpty()) item { SectionTitle("Playlists") }
            items(playlists, key = { it.id }) { list ->
                LibraryRow(list.name, songs(list.count.toInt()), list.artwork, R.drawable.ic_queue_music) {
                    onOpen(SongList.Playlist(list.id, list.name))
                }
            }
        }
        ExtendedFloatingActionButton(
            onClick = { creating = true },
            icon = { Icon(painterResource(R.drawable.ic_add), contentDescription = null) },
            text = { Text("New playlist") },
            modifier = Modifier.align(Alignment.BottomEnd).padding(16.dp).padding(bottom = LocalMiniPlayerSpace.current),
        )
    }

    if (creating) {
        NameDialog(
            title = "New playlist",
            confirm = "Create",
            onConfirm = { name ->
                creating = false
                scope.launch {
                    val id = viewModel.library.createPlaylist(name)
                    onOpen(SongList.Playlist(id, name.ifBlank { "New playlist" }))
                }
            },
            onDismiss = { creating = false },
        )
    }
}

@Composable
private fun LibraryRow(title: String, subtitle: String, artwork: String?, icon: Int, onClick: () -> Unit) {
    ListItem(
        onClick = onClick,
        modifier = Modifier.padding(horizontal = 8.dp),
        supportingContent = { Text(subtitle) },
        leadingContent = {
            if (artwork != null) {
                Artwork(artwork, 56)
            } else {
                Surface(Modifier.size(56.dp), shape = RoundedCornerShape(14.dp), color = MaterialTheme.colorScheme.primaryContainer) {
                    Box(contentAlignment = Alignment.Center) { Icon(painterResource(icon), contentDescription = null) }
                }
            }
        },
    ) {
        Text(title, maxLines = 1, overflow = TextOverflow.Ellipsis)
    }
}

/** The songs in one collection, with Play and Shuffle. */
@Composable
fun CollectionScreen(collection: SongList, viewModel: PlayerViewModel, actions: TrackActions, onBack: () -> Unit) {
    val liked by viewModel.library.liked.collectAsStateWithLifecycle()
    val history by viewModel.library.history.collectAsStateWithLifecycle()
    val playlists by viewModel.library.playlists.collectAsStateWithLifecycle()
    val cached by viewModel.library.cached.collectAsStateWithLifecycle()
    var playlistTracks by remember(collection) { mutableStateOf<List<TrackInfo>>(emptyList()) }
    if (collection is SongList.Playlist) {
        // Reload when the playlist changes (songs added or removed).
        LaunchedEffect(collection, playlists) { playlistTracks = viewModel.library.playlistTracks(collection.id) }
    }
    val info = (collection as? SongList.Playlist)?.let { c -> playlists.firstOrNull { it.id == c.id } }
    val title = info?.name ?: collection.title
    val tracks = when (collection) {
        SongList.Liked -> liked
        SongList.History -> history
        SongList.Offline -> cached
        is SongList.Playlist -> playlistTracks
    }
    var renaming by remember { mutableStateOf(false) }
    var menu by remember { mutableStateOf(false) }

    LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(bottom = 24.dp + LocalMiniPlayerSpace.current)) {
        item {
            Row(Modifier.fillMaxWidth().padding(8.dp), verticalAlignment = Alignment.CenterVertically) {
                IconButton(onClick = onBack) { Icon(painterResource(R.drawable.ic_arrow_back), contentDescription = "Back") }
                Spacer(Modifier.weight(1f))
                Box {
                    IconButton(onClick = { menu = true }) { Icon(painterResource(R.drawable.ic_more_vert), contentDescription = "More options") }
                    VeenaMenu(expanded = menu, onDismiss = { menu = false }) {
                        when (collection) {
                            is SongList.Playlist -> {
                                VeenaMenuItem("Rename", R.drawable.ic_edit) { menu = false; renaming = true }
                                VeenaMenuItem("Delete playlist", R.drawable.ic_delete) {
                                    menu = false
                                    viewModel.library.deletePlaylist(collection.id)
                                    onBack()
                                }
                            }
                            SongList.History -> VeenaMenuItem("Clear history", R.drawable.ic_delete) {
                                menu = false
                                viewModel.library.clearHistory()
                            }
                            SongList.Liked -> VeenaMenuItem("Add all to queue", R.drawable.ic_queue_music) {
                                menu = false
                                tracks.forEach(actions.onAddToQueue)
                            }
                            SongList.Offline -> VeenaMenuItem("Clear all", R.drawable.ic_delete) {
                                menu = false
                                viewModel.library.clearCache()
                            }
                        }
                    }
                }
            }
        }
        item {
            Column(Modifier.fillMaxWidth().padding(horizontal = 24.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                val art = tracks.firstOrNull()?.artwork
                if (art != null) {
                    Artwork(art, 220)
                } else {
                    Surface(Modifier.size(220.dp), shape = RoundedCornerShape(28.dp), color = MaterialTheme.colorScheme.primaryContainer) {
                        Box(contentAlignment = Alignment.Center) {
                            Icon(
                                painterResource(
                                    when (collection) {
                                        SongList.Liked -> R.drawable.ic_favorite
                                        SongList.Offline -> R.drawable.ic_offline_pin
                                        else -> R.drawable.ic_queue_music
                                    },
                                ),
                                contentDescription = null,
                                modifier = Modifier.size(72.dp),
                            )
                        }
                    }
                }
                Text(title, style = MaterialTheme.typography.headlineSmall, fontWeight = FontWeight.Bold, modifier = Modifier.padding(top = 16.dp))
                Text(songs(tracks.size), style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                Row(Modifier.padding(vertical = 16.dp), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                    Button(onClick = { viewModel.playList(tracks) }, enabled = tracks.isNotEmpty()) {
                        Icon(painterResource(R.drawable.ic_play), contentDescription = null)
                        Spacer(Modifier.size(8.dp))
                        Text("Play")
                    }
                    FilledTonalButton(onClick = { viewModel.shuffleList(tracks) }, enabled = tracks.isNotEmpty()) {
                        Icon(painterResource(R.drawable.ic_shuffle), contentDescription = null)
                        Spacer(Modifier.size(8.dp))
                        Text("Shuffle")
                    }
                }
            }
        }
        if (tracks.isEmpty()) {
            item {
                Text(
                    when (collection) {
                        SongList.Liked -> "Tap ♡ on a song to like it."
                        SongList.History -> "Nothing played yet."
                        SongList.Offline -> "Songs you play in full are kept here, so they play without the internet. " +
                            "They stay inside Veena; they aren't downloads."
                        is SongList.Playlist -> "Add songs with “Add to playlist…” in a song's menu."
                    },
                    style = MaterialTheme.typography.bodyLarge,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.fillMaxWidth().padding(32.dp),
                )
            }
        }
        itemsIndexed(tracks, key = { i, t -> "$i:${t.kind}:${t.id}" }) { index, track ->
            val extra = when (collection) {
                is SongList.Playlist -> listOf("Remove from playlist" to { viewModel.library.removeFromPlaylist(collection.id, index) })
                SongList.Offline -> listOf("Remove from this phone" to { viewModel.library.removeFromCache(track) })
                else -> emptyList()
            }
            TrackRow(track, actions, onClick = { viewModel.playList(tracks, index) }, showSource = false, extra = extra)
        }
    }

    if (renaming && collection is SongList.Playlist) {
        NameDialog(
            title = "Rename playlist",
            confirm = "Rename",
            initial = title,
            onConfirm = { name ->
                renaming = false
                viewModel.library.renamePlaylist(collection.id, name)
            },
            onDismiss = { renaming = false },
        )
    }
}

/** Pick a playlist for a song, or make a new one. */
@Composable
fun AddToPlaylistDialog(playlists: List<PlaylistInfo>, onPick: (id: String?, newName: String) -> Unit, onDismiss: () -> Unit) {
    var creating by remember { mutableStateOf(playlists.isEmpty()) }
    if (creating) {
        NameDialog(title = "New playlist", confirm = "Create", onConfirm = { onPick(null, it) }, onDismiss = onDismiss)
        return
    }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Add to playlist") },
        text = {
            LazyColumn {
                item {
                    ListItem(
                        onClick = { creating = true },
                        leadingContent = { Icon(painterResource(R.drawable.ic_add), contentDescription = null) },
                    ) {
                        Text("New playlist")
                    }
                }
                items(playlists, key = { it.id }) { list ->
                    ListItem(
                        onClick = { onPick(list.id, "") },
                        supportingContent = { Text(songs(list.count.toInt())) },
                        leadingContent = { Icon(painterResource(R.drawable.ic_queue_music), contentDescription = null) },
                    ) {
                        Text(list.name, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    }
                }
            }
        },
        confirmButton = {},
        dismissButton = { TextButton(onClick = onDismiss) { Text("Cancel") } },
    )
}

@Composable
private fun NameDialog(title: String, confirm: String, initial: String = "", onConfirm: (String) -> Unit, onDismiss: () -> Unit) {
    var name by remember { mutableStateOf(initial) }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(title) },
        text = {
            OutlinedTextField(
                value = name,
                onValueChange = { name = it.take(100) },
                placeholder = { Text("Playlist name") },
                singleLine = true,
                keyboardOptions = KeyboardOptions(imeAction = ImeAction.Done),
                keyboardActions = KeyboardActions(onDone = { onConfirm(name) }),
            )
        },
        confirmButton = { TextButton(onClick = { onConfirm(name) }) { Text(confirm) } },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Cancel") } },
    )
}
