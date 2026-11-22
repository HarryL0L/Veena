package dev.veena.app.ui

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
            actions.forEachIndexed { i, action ->
                if (i > 0) Spacer(Modifier.height(2.dp))
                // Joined rows: rounder at the ends of the group, like Settings.
                val top = if (i == 0) 20.dp else 4.dp
                val bottom = if (i == actions.lastIndex) 20.dp else 4.dp
                Surface(
                    onClick = {
                        scope.launch { state.hide() }.invokeOnCompletion {
                            onDismiss()
                            action.run()
                        }
                    },
                    shape = RoundedCornerShape(topStart = top, topEnd = top, bottomStart = bottom, bottomEnd = bottom),
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
