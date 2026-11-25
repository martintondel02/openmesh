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
import androidx.compose.runtime.saveable.rememberSaveable
import org.openmesh.android.util.MeshLog
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.core.content.ContextCompat
import org.openmesh.android.data.notify.Notifications
import org.openmesh.android.data.repository.MeshGraph
import org.openmesh.android.service.MeshService
import org.openmesh.android.ui.navigation.OpenMeshNavHost
import org.openmesh.android.ui.screens.OnboardingScreen
import org.openmesh.android.ui.theme.OpenMeshTheme

class MainActivity : ComponentActivity() {
    private var pendingThread = mutableStateOf<Pair<String, Boolean>?>(null)

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val repository = MeshGraph.repository()

        val notificationPermission = registerForActivityResult(
            ActivityResultContracts.RequestPermission(),
        ) { /* notifications are optional; the mesh keeps running without them */ }

        if (Build.VERSION.SDK_INT >= 33 &&
            ContextCompat.checkSelfPermission(this, Manifest.permission.POST_NOTIFICATIONS) !=
            android.content.pm.PackageManager.PERMISSION_GRANTED
        ) {
            notificationPermission.launch(Manifest.permission.POST_NOTIFICATIONS)
        }

        handleInviteIntent(intent)

        setContent {
            OpenMeshTheme {
                val settings by repository.settings.collectAsState()
                val initialized by repository.initialized.collectAsState()
                val radio by repository.radio.collectAsState()
                var showOnboarding by rememberSaveable { mutableStateOf<Boolean?>(null) }
                LaunchedEffect(initialized, settings.setupCompleted) {
                    if (initialized && showOnboarding == null) {
                        showOnboarding = !settings.setupCompleted
                    }
                }
                var snackbar by remember { mutableStateOf<String?>(null) }

                // Start keeping the mesh alive once onboarding has completed.
                LaunchedEffect(settings.setupCompleted) {
                    if (settings.setupCompleted) {
                        MeshService.start(this@MainActivity, MeshService.ACTION_START)
                    }
                }
                LaunchedEffect(Unit) {
                    repository.uiEvents.collect { event ->
                        when (event) {
                            is org.openmesh.android.data.repository.UiEvent.InviteImported ->
                                snackbar = "Imported ${if (event.channel) "channel" else "contact"}: ${event.label}"
                            is org.openmesh.android.data.repository.UiEvent.Error -> snackbar = event.text
                            is org.openmesh.android.data.repository.UiEvent.Snackbar -> snackbar = event.text
                        }
                    }
                }
                // Notification tap → conversation; onNewIntent updates the same state.
                LaunchedEffect(pendingThread.value) { /* recompose when the intent changes */ }

                val onboarding = showOnboarding
                if (onboarding == true) {
                    OnboardingScreen(repository = repository) {
                        MeshLog.i("onboarding: finished by user; entering app")
                        showOnboarding = false
                        pendingThread.value = null
                    }
                } else if (onboarding == false) {
                    OpenMeshNavHost(
                        repository = repository,
                        radio = radio,
                        snackbarMessage = snackbar,
                        onSnackbarShown = { snackbar = null },
                        initialThread = pendingThread.value,
                    )
                }
            }
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        handleInviteIntent(intent)
    }

    private fun handleInviteIntent(intent: Intent?) {
        Notifications.peerFromIntent(intent)?.let { peer ->
            MeshLog.i("notification tap: peer=$peer channel=${Notifications.channelFromIntent(intent)}")
            pendingThread.value = peer to Notifications.channelFromIntent(intent)
        }
        val data = intent?.data ?: return
        if (data.scheme == "openmesh") {
            runCatching { MeshGraph.repository().importInvite(data.toString()) }
        }
    }
}
