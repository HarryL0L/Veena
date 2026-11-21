package dev.veena.app.ui

import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.ClipEntry
import androidx.compose.ui.platform.LocalClipboard
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.launch
import uniffi.veena_ffi.recentLogs

/** Recent log lines, to copy and share when something doesn't play. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun LogsSheet(onDismiss: () -> Unit) {
    var lines by remember { mutableStateOf(recentLogs()) }
    val clipboard = LocalClipboard.current
    val scope = rememberCoroutineScope()
    val scroll = rememberScrollState()
    LaunchedEffect(lines) { scroll.scrollTo(scroll.maxValue) }

    ModalBottomSheet(onDismissRequest = onDismiss) {
        Column(Modifier.padding(horizontal = 16.dp).navigationBarsPadding()) {
            Text("Logs", style = MaterialTheme.typography.titleLarge)
            Text(
                "What Veena did recently. Copy this when something doesn't play.",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(top = 4.dp, bottom = 12.dp),
            )
            Surface(
                color = MaterialTheme.colorScheme.surfaceContainerHighest,
                shape = MaterialTheme.shapes.large,
                modifier = Modifier.fillMaxWidth().heightIn(max = 420.dp),
            ) {
                SelectionContainer {
                    Text(
                        lines.joinToString("\n").ifEmpty { "Nothing yet." },
                        fontFamily = FontFamily.Monospace,
                        fontSize = 11.sp,
                        lineHeight = 15.sp,
                        modifier = Modifier
                            .verticalScroll(scroll)
                            .horizontalScroll(rememberScrollState())
                            .padding(12.dp),
                    )
                }
            }
            Row(
                Modifier.fillMaxWidth().padding(vertical = 16.dp),
                horizontalArrangement = Arrangement.spacedBy(12.dp, androidx.compose.ui.Alignment.End),
            ) {
                OutlinedButton(onClick = { lines = recentLogs() }) { Text("Refresh") }
                Button(onClick = {
                    scope.launch {
                        clipboard.setClipEntry(
                            ClipEntry(android.content.ClipData.newPlainText("Veena logs", lines.joinToString("\n"))),
                        )
                    }
                }) { Text("Copy") }
            }
        }
    }
}
