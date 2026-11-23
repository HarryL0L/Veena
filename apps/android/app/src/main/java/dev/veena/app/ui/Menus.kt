package dev.veena.app.ui

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.DpOffset
import androidx.compose.ui.unit.dp
import dev.veena.app.R
import kotlinx.coroutines.launch
import uniffi.veena_ffi.TrackInfo
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.FilledIconButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.OutlinedTextFieldDefaults
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.runtime.LaunchedEffect
import uniffi.veena_ffi.PlaylistInfo

/** One action in a song's sheet. */
data class SongAction(val label: String, val icon: Int, val run: () -> Unit)

/**
 * What can be done with a song, in a bottom sheet: the song at the top so
 * it's clear which one, then large rounded rows. The sheet slides away
 * before the action runs.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SongSheet(track: TrackInfo, actions: List<SongAction>, onDismiss: () -> Unit) {
    val state = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    val scope = rememberCoroutineScope()
    ModalBottomSheet(
        onDismissRequest = onDismiss,
        sheetState = state,
        containerColor = MaterialTheme.colorScheme.surfaceContainerLow,
    ) {
        Column(Modifier.navigationBarsPadding().padding(horizontal = 16.dp).padding(bottom = 12.dp)) {
            SheetHeader(track)
            actions.forEachIndexed { i, action ->
                if (i > 0) Spacer(Modifier.height(2.dp))
                Surface(
                    onClick = {
                        scope.launch { state.hide() }.invokeOnCompletion {
                            onDismiss()
                            action.run()
                        }
                    },
                    shape = joinedShape(i, actions.size),
                    color = MaterialTheme.colorScheme.surfaceContainerHigh,
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Row(Modifier.padding(horizontal = 20.dp, vertical = 16.dp), verticalAlignment = Alignment.CenterVertically) {
                        Icon(
                            painterResource(action.icon),
                            contentDescription = null,
                            tint = MaterialTheme.colorScheme.primary,
                            modifier = Modifier.size(24.dp),
                        )
                        Text(action.label, style = MaterialTheme.typography.bodyLarge, modifier = Modifier.padding(start = 20.dp))
                    }
                }
            }
        }
    }
}

/** The song a sheet is about, so it's clear which one. */
@Composable
private fun SheetHeader(track: TrackInfo) {
    Row(Modifier.padding(start = 8.dp, bottom = 16.dp), verticalAlignment = Alignment.CenterVertically) {
        Artwork(track.artwork, 56)
        Column(Modifier.padding(start = 16.dp).weight(1f)) {
            Text(
                track.title,
                style = MaterialTheme.typography.titleMedium,
                fontWeight = FontWeight.SemiBold,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
            )
            Text(
                listOfNotNull(track.artist, track.source.ifEmpty { null }).joinToString(" · "),
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
    }
}

/** Joined rows: rounder at the ends of the group, like Settings. */
private fun joinedShape(index: Int, count: Int): RoundedCornerShape {
    val top = if (index == 0) 20.dp else 4.dp
    val bottom = if (index == count - 1) 20.dp else 4.dp
    return RoundedCornerShape(topStart = top, topEnd = top, bottomStart = bottom, bottomEnd = bottom)
}

/**
 * Pick a playlist for a song, in the same kind of sheet as the song's
 * actions: "New playlist" first (named right here), then each playlist with
 * its cover. With no playlists yet, it starts on naming one.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun AddToPlaylistSheet(
    track: TrackInfo,
    playlists: List<PlaylistInfo>,
    onPick: (id: String?, newName: String) -> Unit,
    onDismiss: () -> Unit,
) {
    val state = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    val scope = rememberCoroutineScope()
    var naming by remember { mutableStateOf(playlists.isEmpty()) }
    var name by remember { mutableStateOf("") }
    // The sheet slides away before the song is added.
    fun finish(id: String?, newName: String) {
        scope.launch { state.hide() }.invokeOnCompletion { onPick(id, newName) }
    }
    ModalBottomSheet(
        onDismissRequest = onDismiss,
        sheetState = state,
        containerColor = MaterialTheme.colorScheme.surfaceContainerLow,
    ) {
        Column(Modifier.navigationBarsPadding().padding(horizontal = 16.dp).padding(bottom = 12.dp)) {
            SheetHeader(track)
            Text(
                "Add to playlist",
                style = MaterialTheme.typography.titleSmall,
                color = MaterialTheme.colorScheme.primary,
                modifier = Modifier.padding(start = 8.dp, bottom = 8.dp),
            )
            val count = playlists.size + 1
            LazyColumn(Modifier.heightIn(max = 420.dp)) {
                item(key = "new") {
                    Surface(
                        onClick = { naming = true },
                        enabled = !naming,
                        shape = joinedShape(0, count),
                        color = MaterialTheme.colorScheme.surfaceContainerHigh,
                        modifier = Modifier.fillMaxWidth(),
                    ) {
                        if (naming) {
                            val focus = remember { FocusRequester() }
                            LaunchedEffect(Unit) { focus.requestFocus() }
                            Row(Modifier.padding(horizontal = 12.dp, vertical = 10.dp), verticalAlignment = Alignment.CenterVertically) {
                                OutlinedTextField(
                                    value = name,
                                    onValueChange = { name = it.take(100) },
                                    placeholder = { Text("New playlist name") },
                                    singleLine = true,
                                    shape = RoundedCornerShape(16.dp),
                                    colors = OutlinedTextFieldDefaults.colors(
                                        unfocusedContainerColor = MaterialTheme.colorScheme.surfaceContainerHighest,
                                        focusedContainerColor = MaterialTheme.colorScheme.surfaceContainerHighest,
                                    ),
                                    keyboardOptions = KeyboardOptions(imeAction = ImeAction.Done),
                                    keyboardActions = KeyboardActions(onDone = { if (name.isNotBlank()) finish(null, name.trim()) }),
                                    modifier = Modifier.weight(1f).focusRequester(focus),
                                )
                                FilledIconButton(
                                    onClick = { finish(null, name.trim()) },
                                    enabled = name.isNotBlank(),
                                    shape = CircleShape,
                                    modifier = Modifier.padding(start = 10.dp).size(48.dp),
                                ) {
                                    Icon(painterResource(R.drawable.ic_check), contentDescription = "Create")
                                }
                            }
                        } else {
                            Row(Modifier.padding(horizontal = 20.dp, vertical = 16.dp), verticalAlignment = Alignment.CenterVertically) {
                                Icon(
                                    painterResource(R.drawable.ic_add),
                                    contentDescription = null,
                                    tint = MaterialTheme.colorScheme.primary,
                                    modifier = Modifier.size(24.dp),
                                )
                                Text("New playlist", style = MaterialTheme.typography.bodyLarge, modifier = Modifier.padding(start = 20.dp))
                            }
                        }
                    }
                }
                itemsIndexed(playlists, key = { _, list -> list.id }) { i, list ->
                    Spacer(Modifier.height(2.dp))
                    Surface(
                        onClick = { finish(list.id, "") },
                        shape = joinedShape(i + 1, count),
                        color = MaterialTheme.colorScheme.surfaceContainerHigh,
                        modifier = Modifier.fillMaxWidth(),
                    ) {
                        Row(Modifier.padding(horizontal = 12.dp, vertical = 10.dp), verticalAlignment = Alignment.CenterVertically) {
                            if (list.artwork != null) {
                                Artwork(list.artwork, 44)
                            } else {
                                Surface(Modifier.size(44.dp), shape = RoundedCornerShape(11.dp), color = MaterialTheme.colorScheme.secondaryContainer) {
                                    Box(contentAlignment = Alignment.Center) {
                                        Icon(
                                            painterResource(R.drawable.ic_queue_music),
                                            contentDescription = null,
                                            tint = MaterialTheme.colorScheme.onSecondaryContainer,
                                        )
                                    }
                                }
                            }
                            Column(Modifier.padding(start = 16.dp).weight(1f)) {
                                Text(list.name, style = MaterialTheme.typography.bodyLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
                                Text(
                                    songs(list.count.toInt()),
                                    style = MaterialTheme.typography.bodyMedium,
                                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                                )
                            }
                        }
                    }
                }
            }
        }
    }
}

/**
 * A small menu attached to a button, in the app's style: rounded corners and
 * the same tinted surface as the cards.
 */
@Composable
fun VeenaMenu(
    expanded: Boolean,
    onDismiss: () -> Unit,
    offset: DpOffset = DpOffset(0.dp, 0.dp),
    content: @Composable ColumnScope.() -> Unit,
) {
    DropdownMenu(
        expanded = expanded,
        onDismissRequest = onDismiss,
        offset = offset,
        shape = RoundedCornerShape(20.dp),
        containerColor = MaterialTheme.colorScheme.surfaceContainerHigh,
        tonalElevation = 0.dp,
        shadowElevation = 6.dp,
        content = content,
    )
}

/** A row in a [VeenaMenu], with an optional icon. */
@Composable
fun VeenaMenuItem(label: String, icon: Int? = null, onClick: () -> Unit) {
    DropdownMenuItem(
        text = { Text(label, style = MaterialTheme.typography.bodyLarge) },
        leadingIcon = icon?.let { { Icon(painterResource(it), contentDescription = null, tint = MaterialTheme.colorScheme.primary) } },
        onClick = onClick,
        modifier = Modifier.padding(horizontal = 4.dp),
    )
}

/** The delete icon, for remove-style actions passed in from one list. */
val REMOVE_ICON = R.drawable.ic_delete
