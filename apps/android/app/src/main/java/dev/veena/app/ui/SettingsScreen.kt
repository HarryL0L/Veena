package dev.veena.app.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LargeFlexibleTopAppBar
import androidx.compose.material3.LinearWavyProgressIndicator
import androidx.compose.material3.ListItem
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.Surface
import androidx.compose.material3.Switch
import androidx.compose.material3.SwitchDefaults
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.material3.rememberTopAppBarState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.input.nestedscroll.nestedScroll
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import dev.veena.app.BuildConfig
import dev.veena.app.PlayerViewModel
import dev.veena.app.R
import dev.veena.app.Settings
import dev.veena.app.ThemeMode

/**
 * Settings in the Material You style of Android's own Settings app: a large
 * title that shrinks as you scroll, and grouped rows on rounded cards, each
 * with a round tinted icon.
 */
@OptIn(ExperimentalMaterial3Api::class, ExperimentalMaterial3ExpressiveApi::class, ExperimentalLayoutApi::class)
@Composable
fun SettingsScreen(viewModel: PlayerViewModel, onBack: () -> Unit, onLogs: () -> Unit, onOpenSaved: () -> Unit) {
    val settings by viewModel.settings.collectAsStateWithLifecycle()
    val used by viewModel.library.cacheBytes.collectAsStateWithLifecycle()
    val saved by viewModel.library.cached.collectAsStateWithLifecycle()
    var confirmClear by remember { mutableStateOf(false) }
    val bricks = remember { viewModel.bricks() }
    val scroll = TopAppBarDefaults.exitUntilCollapsedScrollBehavior(rememberTopAppBarState())

    Column(Modifier.fillMaxSize().nestedScroll(scroll.nestedScrollConnection)) {
        LargeFlexibleTopAppBar(
            title = { Text("Settings") },
            navigationIcon = {
                IconButton(onClick = onBack) { Icon(painterResource(R.drawable.ic_arrow_back), contentDescription = "Back") }
            },
            // The page already sits below the status bar.
            windowInsets = WindowInsets(0),
            colors = TopAppBarDefaults.topAppBarColors(
                containerColor = MaterialTheme.colorScheme.surface,
                scrolledContainerColor = MaterialTheme.colorScheme.surfaceContainer,
            ),
            scrollBehavior = scroll,
        )

        LazyColumn(
            modifier = Modifier.fillMaxSize(),
            contentPadding = PaddingValues(start = 16.dp, end = 16.dp, bottom = 24.dp + LocalMiniPlayerSpace.current),
        ) {
            group("Offline listening") {
                listOf<@Composable () -> Unit>(
                    {
                        SwitchItem(
                            index = 0, count = 3,
                            icon = R.drawable.ic_offline_pin,
                            title = "Keep songs for offline listening",
                            subtitle = "Songs you play in full stay inside Veena and play without the internet. " +
                                "They aren't downloads: they can't be opened or shared outside the app.",
                            checked = settings.cacheEnabled,
                            onChange = { on -> viewModel.updateSettings { it.copy(cacheEnabled = on) } },
                        )
                    },
                    {
                        SwitchItem(
                            index = 1, count = 3,
                            icon = R.drawable.ic_cloud_off,
                            title = "Offline mode",
                            subtitle = "Only play and search songs kept on this phone.",
                            checked = settings.offline,
                            onChange = { on -> viewModel.updateSettings { it.copy(offline = on) } },
                        )
                    },
                    {
                        Item(
                            index = 2, count = 3,
                            icon = R.drawable.ic_library_music,
                            title = "Kept songs",
                            subtitle = songs(saved.size),
                            onClick = onOpenSaved,
                        )
                    },
                )
            }

            group("Storage") {
                listOf<@Composable () -> Unit>(
                    {
                        Card(index = 0, count = 1) {
                            Row(verticalAlignment = Alignment.CenterVertically) {
                                IconBadge(R.drawable.ic_storage)
                                Column(Modifier.padding(start = 16.dp).weight(1f)) {
                                    Text("Space for kept songs", style = MaterialTheme.typography.titleMedium)
                                    val limit = settings.cacheLimitMb.toLong() * 1024 * 1024
                                    Text(
                                        "${size(used)} of ${size(limit)} used",
                                        style = MaterialTheme.typography.bodyMedium,
                                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                                    )
                                }
                            }
                            val limit = settings.cacheLimitMb.toLong() * 1024 * 1024
                            LinearWavyProgressIndicator(
                                progress = { if (limit > 0) (used.toFloat() / limit).coerceIn(0f, 1f) else 0f },
                                modifier = Modifier.fillMaxWidth().padding(top = 16.dp),
                            )
                            FlowRow(
                                Modifier.padding(top = 12.dp),
                                horizontalArrangement = Arrangement.spacedBy(8.dp),
                            ) {
                                Settings.CACHE_SIZES.forEach { mb ->
                                    FilterChip(
                                        selected = settings.cacheLimitMb == mb,
                                        onClick = { viewModel.updateSettings { it.copy(cacheLimitMb = mb) } },
                                        label = { Text(size(mb.toLong() * 1024 * 1024)) },
                                    )
                                }
                            }
                            Text(
                                "When it's full, the songs played longest ago make room.",
                                style = MaterialTheme.typography.bodySmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                                modifier = Modifier.padding(top = 4.dp),
                            )
                            Row(Modifier.padding(top = 12.dp), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                                FilledTonalButton(onClick = onOpenSaved) { Text("See kept songs") }
                                OutlinedButton(onClick = { confirmClear = true }, enabled = saved.isNotEmpty()) { Text("Clear") }
                            }
                        }
                    },
                )
            }

            group("Appearance") {
                listOf<@Composable () -> Unit>(
                    {
                        Card(index = 0, count = 1) {
                            Row(verticalAlignment = Alignment.CenterVertically) {
                                IconBadge(R.drawable.ic_palette)
                                Column(Modifier.padding(start = 16.dp)) {
                                    Text("Theme", style = MaterialTheme.typography.titleMedium)
                                    Text(
                                        "Colours come from your wallpaper, and the player from each song's artwork.",
                                        style = MaterialTheme.typography.bodyMedium,
                                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                                    )
                                }
                            }
                            SingleChoiceSegmentedButtonRow(Modifier.fillMaxWidth().padding(top = 16.dp)) {
                                ThemeMode.entries.forEachIndexed { i, mode ->
                                    SegmentedButton(
                                        selected = settings.theme == mode,
                                        onClick = { viewModel.updateSettings { it.copy(theme = mode) } },
                                        shape = SegmentedButtonDefaults.itemShape(i, ThemeMode.entries.size),
                                    ) {
                                        Text(
                                            when (mode) {
                                                ThemeMode.SYSTEM -> "System"
                                                ThemeMode.LIGHT -> "Light"
                                                ThemeMode.DARK -> "Dark"
                                            },
                                        )
                                    }
                                }
                            }
                        }
                    },
                )
            }

            group("Bricks", "The parts that fetch music and lyrics. They update on their own, without a new app.") {
                bricks.mapIndexed { i, brick ->
                    @Composable {
                        Item(
                            index = i, count = bricks.size,
                            icon = when {
                                brick.error != null -> R.drawable.ic_bug
                                brick.audioLinks -> R.drawable.ic_music_note
                                brick.search -> R.drawable.ic_search
                                else -> R.drawable.ic_queue_music
                            },
                            title = brick.name.ifEmpty { brick.id },
                            subtitle = brick.error ?: listOfNotNull(
                                "Search".takeIf { brick.search },
                                "Audio links".takeIf { brick.audioLinks },
                            ).joinToString(" · ").ifEmpty { "Lyrics" },
                            trailing = { Text(brick.version, style = MaterialTheme.typography.labelLarge) },
                        )
                    }
                }
            }

            group("About") {
                listOf<@Composable () -> Unit>(
                    { Item(index = 0, count = 2, icon = R.drawable.ic_info, title = "Veena", subtitle = "Version ${BuildConfig.VERSION_NAME}") },
                    {
                        Item(
                            index = 1, count = 2,
                            icon = R.drawable.ic_bug,
                            title = "Logs",
                            subtitle = "What Veena did recently, to copy when something goes wrong",
                            onClick = onLogs,
                        )
                    },
                )
            }
        }
    }

    if (confirmClear) {
        AlertDialog(
            onDismissRequest = { confirmClear = false },
            icon = { Icon(painterResource(R.drawable.ic_delete), contentDescription = null) },
            title = { Text("Clear kept songs?") },
            text = { Text("${songs(saved.size)} will be removed from this phone. They'll be kept again next time you play them.") },
            confirmButton = {
                TextButton(onClick = {
                    confirmClear = false
                    viewModel.library.clearCache()
                }) { Text("Clear") }
            },
            dismissButton = { TextButton(onClick = { confirmClear = false }) { Text("Cancel") } },
        )
    }
}

/** A titled group of rows, joined into one rounded card with small gaps. */
private fun LazyListScope.group(title: String, note: String? = null, rows: () -> List<@Composable () -> Unit>) {
    item(key = "group:$title") {
        Column(Modifier.padding(start = 8.dp, top = 24.dp, bottom = 8.dp)) {
            Text(title, style = MaterialTheme.typography.titleSmall, color = MaterialTheme.colorScheme.primary)
            if (note != null) {
                Text(
                    note,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(top = 2.dp, end = 8.dp),
                )
            }
        }
    }
    rows().forEachIndexed { i, row ->
        item(key = "group:$title:$i") {
            if (i > 0) Spacer(Modifier.height(ListItemDefaults.SegmentedGap))
            row()
        }
    }
}

/** A round, tinted icon, like the ones in Android's Settings. */
@Composable
private fun IconBadge(icon: Int) {
    Surface(Modifier.size(40.dp), shape = CircleShape, color = MaterialTheme.colorScheme.secondaryContainer) {
        Box(contentAlignment = Alignment.Center) {
            Icon(
                painterResource(icon),
                contentDescription = null,
                tint = MaterialTheme.colorScheme.onSecondaryContainer,
                modifier = Modifier.size(22.dp),
            )
        }
    }
}

@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
private fun Item(
    index: Int,
    count: Int,
    icon: Int,
    title: String,
    subtitle: String,
    onClick: (() -> Unit)? = null,
    trailing: (@Composable () -> Unit)? = null,
) {
    val shapes = ListItemDefaults.segmentedShapes(index, count)
    val colors = ListItemDefaults.segmentedColors(containerColor = MaterialTheme.colorScheme.surfaceContainer)
    val leading: @Composable () -> Unit = { IconBadge(icon) }
    val supporting: @Composable () -> Unit = { Text(subtitle) }
    if (onClick != null) {
        ListItem(
            onClick = onClick,
            shapes = shapes,
            colors = colors,
            leadingContent = leading,
            supportingContent = supporting,
            trailingContent = trailing,
        ) { Text(title) }
    } else {
        ListItem(
            shapes = shapes,
            colors = colors,
            leadingContent = leading,
            supportingContent = supporting,
            trailingContent = trailing,
        ) { Text(title) }
    }
}

@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
private fun SwitchItem(
    index: Int,
    count: Int,
    icon: Int,
    title: String,
    subtitle: String,
    checked: Boolean,
    onChange: (Boolean) -> Unit,
) {
    ListItem(
        onClick = { onChange(!checked) },
        shapes = ListItemDefaults.segmentedShapes(index, count),
        colors = ListItemDefaults.segmentedColors(containerColor = MaterialTheme.colorScheme.surfaceContainer),
        leadingContent = { IconBadge(icon) },
        supportingContent = { Text(subtitle) },
        trailingContent = {
            Switch(
                checked = checked,
                onCheckedChange = onChange,
                thumbContent = if (checked) {
                    { Icon(painterResource(R.drawable.ic_check), contentDescription = null, modifier = Modifier.size(SwitchDefaults.IconSize)) }
                } else {
                    null
                },
            )
        },
    ) { Text(title) }
}

/** A card-shaped group row with free-form content (sliders, chips, buttons). */
@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
private fun Card(index: Int, count: Int, content: @Composable () -> Unit) {
    val shape = ListItemDefaults.segmentedShapes(index, count).shape
    Surface(
        modifier = Modifier.fillMaxWidth(),
        shape = shape,
        color = MaterialTheme.colorScheme.surfaceContainer,
    ) {
        Column(Modifier.padding(16.dp)) { content() }
    }
}

private fun size(bytes: Long): String = when {
    bytes >= 1024L * 1024 * 1024 -> "%.1f GB".format(bytes / (1024.0 * 1024 * 1024)).replace(".0 ", " ")
    else -> "${bytes / (1024 * 1024)} MB"
}
