package org.openmesh.android

import android.Manifest
import android.content.Intent
import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.core.content.ContextCompat
import org.openmesh.android.data.repository.MeshGraph
import org.openmesh.android.ui.navigation.OpenMeshNavHost
import org.openmesh.android.ui.theme.OpenMeshTheme

class MainActivity : ComponentActivity() {

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val repository = MeshGraph.repository()

        val notificationPermission = registerForActivityResult(
            ActivityResultContracts.RequestPermission(),
        ) { /* notifications are optional; the link still works without them */ }

        if (Build.VERSION.SDK_INT >= 33 &&
            ContextCompat.checkSelfPermission(this, Manifest.permission.POST_NOTIFICATIONS) !=
            android.content.pm.PackageManager.PERMISSION_GRANTED
        ) {
            notificationPermission.launch(Manifest.permission.POST_NOTIFICATIONS)
        }

        // Handle openmesh://contact/... and openmesh://channel/... links while the app is alive.
        handleInviteIntent(intent)

        setContent {
            OpenMeshTheme {
                val radio by repository.radio.collectAsState()
                var deepLinkMessage by remember { mutableStateOf<String?>(null) }
                LaunchedEffect(Unit) {
                    repository.uiEvents.collect { event ->
                        when (event) {
                            is org.openmesh.android.data.repository.UiEvent.InviteImported ->
                                deepLinkMessage = "Imported ${if (event.channel) "channel" else "contact"}: ${event.label}"
                            is org.openmesh.android.data.repository.UiEvent.Error ->
                                deepLinkMessage = event.text
                            is org.openmesh.android.data.repository.UiEvent.Snackbar ->
                                deepLinkMessage = event.text
                        }
                    }
                }
                OpenMeshNavHost(
                    repository = repository,
                    radio = radio,
                    snackbarMessage = deepLinkMessage,
                    onSnackbarShown = { deepLinkMessage = null },
                )
            }
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        handleInviteIntent(intent)
    }

    private fun handleInviteIntent(intent: Intent?) {
        val data = intent?.data ?: return
        if (data.scheme == "openmesh") {
            runCatching { MeshGraph.repository().importInvite(data.toString()) }
        }
    }
}
