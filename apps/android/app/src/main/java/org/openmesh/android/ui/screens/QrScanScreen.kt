package org.openmesh.android.ui.screens

import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.QrCodeScanner
import androidx.compose.material3.Button
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.getValue
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import com.journeyapps.barcodescanner.ScanContract
import com.journeyapps.barcodescanner.ScanOptions
import org.openmesh.android.data.repository.MeshRepository
import org.openmesh.android.ui.components.MeshCard
import org.openmesh.android.ui.theme.MeshOnSurfaceVariant
import org.openmesh.android.ui.theme.MeshPrimary

/**
 * QR scanner for `openmesh://contact` / `openmesh://channel` invites (zxing-android-embedded).
 *
 * The scanner is an Activity provided by the library; the manifest permission is CAMERA.
 * A manual paste field is included because screenshots and copied links are common in the
 * field and scanning is not always possible in sunlight/gloves.
 */
@Composable
fun QrScanScreen(repository: MeshRepository, onBack: () -> Unit) {
    var manual by remember { mutableStateOf("") }
    var message by remember { mutableStateOf<String?>(null) }
    val scanner = rememberLauncherForActivityResult(ScanContract()) { result ->
        val contents = result.contents
        if (contents.isNullOrBlank()) {
            message = "Scan cancelled"
        } else {
            repository.importInvite(contents)
                .onSuccess { message = "Imported ${if (it.kind.name == "CHANNEL") "channel" else "contact"}: ${it.name}" }
                .onFailure { message = it.message ?: "Invalid OpenMesh QR code" }
        }
    }

    Column(Modifier.fillMaxSize()) {
        androidx.compose.foundation.layout.Row(
            Modifier.fillMaxWidth().padding(horizontal = 4.dp, vertical = 6.dp),
            verticalAlignment = androidx.compose.ui.Alignment.CenterVertically,
        ) {
            IconButton(onClick = onBack) {
                Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back")
            }
            Text("Scan invite", style = MaterialTheme.typography.titleLarge)
        }
        Column(
            Modifier.fillMaxSize().padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            MeshCard {
                Column(Modifier.padding(16.dp)) {
                    Text("Scan an OpenMesh QR code", style = MaterialTheme.typography.titleMedium)
                    Spacer(Modifier.height(6.dp))
                    Text(
                        "Contact invites pin a node id and public key; channel invites carry the channel key, " +
                            "profile and region. Nothing is uploaded anywhere.",
                        style = MaterialTheme.typography.bodyMedium,
                        color = MeshOnSurfaceVariant,
                    )
                    Spacer(Modifier.height(12.dp))
                    Button(onClick = {
                        val options = ScanOptions().apply {
                            setDesiredBarcodeFormats(ScanOptions.QR_CODE)
                            setPrompt("Scan an OpenMesh invite")
                            setBeepEnabled(false)
                            setOrientationLocked(false)
                        }
                        scanner.launch(options)
                    }) {
                        Icon(Icons.Filled.QrCodeScanner, contentDescription = null)
                        Spacer(Modifier.padding(4.dp))
                        Text("Open camera")
                    }
                }
            }

            Text("Or paste a link", style = MaterialTheme.typography.titleMedium)
            OutlinedTextField(
                value = manual,
                onValueChange = { manual = it },
                placeholder = { Text("openmesh://contact?…") },
                modifier = Modifier.fillMaxWidth(),
                maxLines = 3,
            )
            TextButton(
                onClick = {
                    repository.importInvite(manual)
                        .onSuccess { message = "Imported: ${it.name}" }
                        .onFailure { message = it.message ?: "Invalid invite" }
                },
                enabled = manual.isNotBlank(),
            ) { Text("Import link") }

            message?.let {
                Text(it, color = MeshPrimary, style = MaterialTheme.typography.bodyMedium)
            }
        }
    }
}
