package org.openmesh.android.ui.screens

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import org.openmesh.android.data.help.AudienceFilter
import org.openmesh.android.data.help.HelpKeys
import org.openmesh.android.data.help.HelpTopicMeta
import org.openmesh.android.data.model.AppSettings
import org.openmesh.android.data.repository.MeshRepository
import org.openmesh.android.ui.theme.MeshOnSurfaceVariant
import org.openmesh.android.ui.theme.MeshPrimary
import org.openmesh.android.ui.theme.MeshSurfaceHigh

/** Bundled Help Center: search, audience filter and a small offline markdown reader. */
@Composable
fun HelpScreen(
    repository: MeshRepository,
    settings: AppSettings,
    onBack: () -> Unit,
    initialTopic: String? = null,
) {
    var query by remember { mutableStateOf("") }
    var audience by remember { mutableStateOf(if (settings.uiMode == "advanced") AudienceFilter.ADVANCED else AudienceFilter.BEGINNER) }
    var openSlug by remember { mutableStateOf(initialTopic) }
    val topics = remember(query, audience, settings.uiMode) {
        runCatching { repository.helpTopics }.getOrDefault(emptyList())
            .filter { meta ->
                val q = query.trim().lowercase()
                val audienceOk = when (audience) {
                    AudienceFilter.ALL -> true
                    AudienceFilter.BEGINNER -> meta.audience == "beginner" || meta.audience == "both"
                    AudienceFilter.ADVANCED -> meta.audience == "advanced" || meta.audience == "both"
                }
                audienceOk && (q.isEmpty() || meta.title.lowercase().contains(q) || meta.summary.lowercase().contains(q))
            }
    }

    openSlug?.let { slug ->
        AlertDialog(
            onDismissRequest = { openSlug = null },
            confirmButton = { TextButton(onClick = { openSlug = null }) { Text("Close") } },
            title = { Text(repository.helpTopic(slug)?.meta?.title ?: slug) },
            text = {
                Column(Modifier.verticalScroll(rememberScrollState())) {
                    MarkdownText(repository.helpTopic(slug)?.markdown ?: "Topic not bundled.")
                }
            },
        )
        return
    }

    Column(Modifier.fillMaxSize()) {
        Row(Modifier.fillMaxWidth().padding(horizontal = 4.dp, vertical = 6.dp), verticalAlignment = androidx.compose.ui.Alignment.CenterVertically) {
            IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back") }
            Column {
                Text("Help", style = MaterialTheme.typography.headlineSmall)
                Text("Works offline · ${topics.size} topics", color = MeshOnSurfaceVariant)
            }
        }
        OutlinedTextField(
            value = query,
            onValueChange = { query = it },
            placeholder = { Text("Search help…") },
            modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp),
            singleLine = true,
        )
        Row(Modifier.padding(horizontal = 12.dp, vertical = 6.dp), horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            AudienceFilter.entries.forEach { filter ->
                FilterChip(selected = audience == filter, onClick = { audience = filter }, label = { Text(filter.label) })
            }
        }
        LazyColumn(
            contentPadding = PaddingValues(horizontal = 16.dp, vertical = 8.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            items(topics.map { it.slug }) { slug ->
                val meta: HelpTopicMeta = topics.first { it.slug == slug }
                Card(
                    onClick = { openSlug = meta.slug },
                    colors = CardDefaults.cardColors(containerColor = MeshSurfaceHigh),
                ) {
                    Column(Modifier.padding(14.dp), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                        Text(meta.title, fontWeight = FontWeight.SemiBold)
                        Text(meta.summary, style = MaterialTheme.typography.bodySmall, color = MeshOnSurfaceVariant)
                        Text("${meta.audience} · ${meta.minutes} min", style = MaterialTheme.typography.labelSmall, color = MeshPrimary)
                    }
                }
            }
            if (topics.isEmpty()) {
                item { Text("No help topics matched.", color = MeshOnSurfaceVariant) }
            }
        }
    }
}

/** Minimal markdown renderer for the bundled corpus (headings, bullets, bold, paragraphs). */
@Composable
fun MarkdownText(markdown: String) {
    val lines = remember(markdown) { markdown.lines() }
    Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
        lines.forEach { raw ->
            val line = raw.trimEnd()
            when {
                line.startsWith("### ") -> Text(line.removePrefix("### "), style = MaterialTheme.typography.titleSmall, fontWeight = FontWeight.SemiBold)
                line.startsWith("## ") -> Text(line.removePrefix("## "), style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold)
                line.startsWith("# ") -> Text(line.removePrefix("# "), style = MaterialTheme.typography.titleLarge, fontWeight = FontWeight.Bold)
                line.startsWith("- ") || line.startsWith("* ") -> Text("• ${line.drop(2)}", style = MaterialTheme.typography.bodyMedium)
                line.startsWith("```") -> Unit
                line.isBlank() -> androidx.compose.foundation.layout.Spacer(Modifier.padding(2.dp))
                else -> Text(line, style = MaterialTheme.typography.bodyMedium)
            }
        }
    }
}

/** Standard inline help affordance: a small "?" that opens the matching help topic. */
@Composable
fun HelpTip(topic: String, onOpenHelp: (String) -> Unit) {
    TextButton(onClick = { onOpenHelp(topic) }) {
        Text("?", style = MaterialTheme.typography.labelLarge, color = MeshPrimary)
    }
}

/** Convenience for screens that link to a help key. */
fun helpTopicFor(key: String): String = HelpKeys.slugFor(key) ?: HelpKeys.GETTING_STARTED
