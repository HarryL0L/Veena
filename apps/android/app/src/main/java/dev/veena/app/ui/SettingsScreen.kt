package dev.veena.app.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.selection.selectable
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import dev.veena.app.BuildConfig
import dev.veena.app.PlayerViewModel
import dev.veena.app.R
import dev.veena.app.Settings
import dev.veena.app.ThemeMode

/** Offline listening, storage, appearance and app details. */
@OptIn(ExperimentalLayoutApi::class)
@Composable
fun SettingsScreen(viewModel: PlayerViewModel, onBack: () -> Unit, onLogs: () -> Unit, onOpenSaved: () -> Unit) {
    val settings by viewModel.settings.collectAsStateWithLifecycle()
    val used by viewModel.library.cacheBytes.collectAsStateWithLifecycle()
    val saved by viewModel.library.cached.collectAsStateWithLifecycle()
    var confirmClear by remember { mutableStateOf(false) }
    val bricks = remember { viewModel.bricks() }

    LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(bottom = 24.dp + LocalMiniPlayerSpace.current)) {
        item {
            Row(Modifier.fillMaxWidth().padding(8.dp), verticalAlignment = Alignment.CenterVertically) {
                IconButton(onClick = onBack) { Icon(painterResource(R.drawable.ic_arrow_back), contentDescription = "Back") }
                Text("Settings", style = MaterialTheme.typography.headlineSmall, fontWeight = FontWeight.Bold)
            }
        }

        item { Group("Offline listening") }
        item {
            SwitchRow(
                title = "Keep songs for offline listening",
                subtitle = "Songs you play in full are kept privately inside Veena, so they play again without the " +
                    "internet. They are not downloads: they can't be opened or shared outside the app.",
                checked = settings.cacheEnabled,
                onChange = { on -> viewModel.updateSettings { it.copy(cacheEnabled = on) } },
            )
        }
        item {
            SwitchRow(
                title = "Offline mode",
                subtitle = "Only play and search songs kept on this phone. Nothing is fetched from the internet.",
                checked = settings.offline,
                onChange = { on -> viewModel.updateSettings { it.copy(offline = on) } },
            )
        }
        item {
            Column(Modifier.padding(horizontal = 24.dp, vertical = 12.dp)) {
                Text("Space for kept songs", style = MaterialTheme.typography.titleMedium)
                val limit = settings.cacheLimitMb.toLong() * 1024 * 1024
                Text(
                    "${songs(saved.size)} · ${size(used)} of ${size(limit)}",
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(top = 2.dp, bottom = 8.dp),
                )
                LinearProgressIndicator(
                    progress = { if (limit > 0) (used.toFloat() / limit).coerceIn(0f, 1f) else 0f },
                    modifier = Modifier.fillMaxWidth(),
                )
                FlowRow(Modifier.padding(top = 12.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
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
                    OutlinedButton(onClick = onOpenSaved) { Text("See kept songs") }
                    OutlinedButton(onClick = { confirmClear = true }, enabled = saved.isNotEmpty()) { Text("Clear") }
                }
            }
        }

        item { Group("Appearance") }
        item {
            Column(Modifier.padding(horizontal = 16.dp)) {
                ThemeMode.entries.forEach { mode ->
                    Row(
                        Modifier
                            .fillMaxWidth()
                            .selectable(
                                selected = settings.theme == mode,
                                onClick = { viewModel.updateSettings { it.copy(theme = mode) } },
                                role = Role.RadioButton,
                            )
                            .padding(vertical = 4.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        RadioButton(selected = settings.theme == mode, onClick = null)
                        Spacer(Modifier.width(12.dp))
                        Text(mode.label, style = MaterialTheme.typography.bodyLarge)
                    }
                }
            }
        }

        item { Group("Bricks") }
        item {
            Text(
                "The parts that fetch music and lyrics. They update on their own, without a new app.",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(horizontal = 24.dp, vertical = 4.dp),
            )
        }
        bricks.forEach { brick ->
            item(key = "brick:${brick.id}") {
                ListItem(
                    supportingContent = {
                        Text(
                            brick.error ?: listOfNotNull(
                                "search".takeIf { brick.search },
                                "audio links".takeIf { brick.audioLinks },
                            ).joinToString(" · ").ifEmpty { "lyrics" },
                        )
                    },
                    trailingContent = { Text(brick.version, style = MaterialTheme.typography.labelMedium) },
                ) {
                    Text(brick.name.ifEmpty { brick.id })
                }
            }
        }

        item { Group("About") }
        item {
            ListItem(supportingContent = { Text("Version ${BuildConfig.VERSION_NAME}") }) { Text("Veena") }
        }
        item {
            ListItem(onClick = onLogs, supportingContent = { Text("What Veena did recently, to copy when something goes wrong") }) {
                Text("Logs")
            }
        }
    }

    if (confirmClear) {
        AlertDialog(
            onDismissRequest = { confirmClear = false },
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

@Composable
private fun Group(title: String) {
    Text(
        title,
        style = MaterialTheme.typography.titleSmall,
        color = MaterialTheme.colorScheme.primary,
        modifier = Modifier.padding(start = 24.dp, end = 24.dp, top = 24.dp, bottom = 4.dp),
    )
}

@Composable
private fun SwitchRow(title: String, subtitle: String, checked: Boolean, onChange: (Boolean) -> Unit) {
    ListItem(
        onClick = { onChange(!checked) },
        supportingContent = { Text(subtitle) },
        trailingContent = { Switch(checked = checked, onCheckedChange = onChange) },
    ) {
        Text(title)
    }
}

private fun size(bytes: Long): String = when {
    bytes >= 1024L * 1024 * 1024 -> "%.1f GB".format(bytes / (1024.0 * 1024 * 1024)).replace(".0 ", " ")
    else -> "${bytes / (1024 * 1024)} MB"
}
