package org.openmesh.android.ui.navigation

import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.ChatBubbleOutline
import androidx.compose.material.icons.outlined.Contacts
import androidx.compose.material.icons.outlined.Hub
import androidx.compose.material.icons.outlined.Map
import androidx.compose.material.icons.outlined.Settings
import androidx.compose.material3.Icon
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.unit.dp
import androidx.navigation.NavGraph.Companion.findStartDestination
import androidx.navigation.NavType
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.currentBackStackEntryAsState
import androidx.navigation.compose.rememberNavController
import androidx.navigation.navArgument
import org.openmesh.android.data.model.RadioStatus
import org.openmesh.android.data.repository.MeshRepository
import org.openmesh.android.ui.screens.AboutScreen
import org.openmesh.android.ui.screens.ChannelsScreen
import org.openmesh.android.ui.screens.ChatListScreen
import org.openmesh.android.ui.screens.ChatThreadScreen
import org.openmesh.android.ui.screens.ContactsScreen
import org.openmesh.android.ui.screens.HelpScreen
import org.openmesh.android.ui.screens.MapScreen
import org.openmesh.android.ui.screens.NodesScreen
import org.openmesh.android.ui.screens.QrScanScreen
import org.openmesh.android.ui.screens.RadioScreen
import org.openmesh.android.ui.screens.SettingsScreen

object Routes {
    const val CHATS = "chats"
    const val MAP = "map"
    const val NODES = "nodes"
    const val CONTACTS = "contacts"
    const val SETTINGS = "settings"
    const val QR = "qr"
    const val CHANNELS = "channels"
    const val RADIO = "radio"
    const val HELP = "help?topic={topic}"
    const val ABOUT = "about"
    const val THREAD = "thread/{peerId}?channel={channel}"
    fun thread(peerId: String, channel: Boolean) = "thread/$peerId?channel=$channel"
    fun help(topic: String? = null) = if (topic.isNullOrBlank()) "help" else "help?topic=$topic"
}

private data class TopLevel(val route: String, val label: String, val icon: ImageVector)

private val topLevel = listOf(
    TopLevel(Routes.CHATS, "Chats", Icons.Outlined.ChatBubbleOutline),
    TopLevel(Routes.MAP, "Map", Icons.Outlined.Map),
    TopLevel(Routes.NODES, "Nodes", Icons.Outlined.Hub),
    TopLevel(Routes.CONTACTS, "Contacts", Icons.Outlined.Contacts),
    TopLevel(Routes.SETTINGS, "Settings", Icons.Outlined.Settings),
)

@Composable
fun OpenMeshNavHost(
    repository: MeshRepository,
    radio: RadioStatus,
    snackbarMessage: String?,
    onSnackbarShown: () -> Unit,
    initialThread: Pair<String, Boolean>? = null,
    regulatoryOverrideActive: Boolean = false,
) {
    val navController = rememberNavController()
    val snackbarHostState = remember { SnackbarHostState() }
    val backStackEntry by navController.currentBackStackEntryAsState()
    val currentRoute = backStackEntry?.destination?.route
    val showBottomBar = topLevel.any { it.route == currentRoute }

    LaunchedEffect(snackbarMessage) {
        if (!snackbarMessage.isNullOrBlank()) {
            snackbarHostState.showSnackbar(snackbarMessage)
            onSnackbarShown()
        }
    }
    LaunchedEffect(initialThread) {
        initialThread?.let { (peer, channel) -> navController.navigate(Routes.thread(peer, channel)) }
    }

    Scaffold(
        snackbarHost = { SnackbarHost(snackbarHostState) },
        topBar = {
            if (regulatoryOverrideActive) {
                androidx.compose.material3.Surface(
                    color = androidx.compose.ui.graphics.Color(0xFFB3261E),
                    contentColor = androidx.compose.ui.graphics.Color.White,
                ) {
                    Text(
                        org.openmesh.android.data.regulatory.RegulatoryOverride.BANNER,
                        modifier = Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 8.dp),
                        style = MaterialTheme.typography.labelMedium,
                    )
                }
            }
        },
        bottomBar = {
            if (showBottomBar) {
                NavigationBar {
                    topLevel.forEach { entry ->
                        NavigationBarItem(
                            selected = currentRoute == entry.route,
                            onClick = {
                                navController.navigate(entry.route) {
                                    popUpTo(navController.graph.findStartDestination().id) { saveState = true }
                                    launchSingleTop = true
                                    restoreState = true
                                }
                            },
                            icon = { Icon(entry.icon, contentDescription = entry.label) },
                            label = { Text(entry.label) },
                        )
                    }
                }
            }
        },
    ) { innerPadding ->
        NavHost(
            navController = navController,
            startDestination = Routes.CHATS,
            modifier = Modifier.padding(innerPadding),
        ) {
            composable(Routes.CHATS) {
                ChatListScreen(
                    repository = repository,
                    radio = radio,
                    onOpenThread = { peerId, channel -> navController.navigate(Routes.thread(peerId, channel)) },
                    onOpenSettings = { navController.navigate(Routes.SETTINGS) },
                    onOpenChannels = { navController.navigate(Routes.CHANNELS) },
                    onOpenHelp = { navController.navigate(Routes.help()) },
                )
            }
            composable(
                route = Routes.THREAD,
                arguments = listOf(
                    navArgument("peerId") { type = NavType.StringType },
                    navArgument("channel") { type = NavType.BoolType; defaultValue = false },
                ),
            ) { entry ->
                val peerId = entry.arguments?.getString("peerId").orEmpty()
                val channel = entry.arguments?.getBoolean("channel") ?: false
                ChatThreadScreen(
                    repository = repository,
                    peerId = peerId,
                    isChannel = channel,
                    onBack = { navController.popBackStack() },
                    onOpenHelp = { slug -> navController.navigate(Routes.help(slug)) },
                )
            }
            composable(Routes.MAP) { MapScreen(repository = repository) }
            composable(Routes.NODES) { NodesScreen(repository = repository) }
            composable(Routes.CONTACTS) {
                ContactsScreen(
                    repository = repository,
                    onScan = { navController.navigate(Routes.QR) },
                    onOpenHelp = { slug -> navController.navigate(Routes.help(slug)) },
                )
            }
            composable(Routes.SETTINGS) {
                SettingsScreen(
                    repository = repository,
                    radio = radio,
                    onBack = { navController.popBackStack() },
                    onOpenHelp = { slug -> navController.navigate(Routes.help(slug)) },
                    onOpenRadio = { navController.navigate(Routes.RADIO) },
                    onOpenAbout = { navController.navigate(Routes.ABOUT) },
                )
            }
            composable(
                route = Routes.HELP,
                arguments = listOf(navArgument("topic") { type = NavType.StringType; nullable = true; defaultValue = null }),
            ) { entry ->
                val settings = repository.settings.collectAsState().value
                HelpScreen(
                    repository = repository,
                    settings = settings,
                    onBack = { navController.popBackStack() },
                    initialTopic = entry.arguments?.getString("topic"),
                )
            }
            composable(Routes.CHANNELS) {
                ChannelsScreen(
                    repository = repository,
                    onBack = { navController.popBackStack() },
                    onOpenThread = { channelId -> navController.navigate(Routes.thread(channelId, true)) },
                    onScan = { navController.navigate(Routes.QR) },
                )
            }
            composable(Routes.RADIO) {
                val settings = repository.settings.collectAsState().value
                RadioScreen(repository = repository, radio = radio, settings = settings, onBack = { navController.popBackStack() })
            }
            composable(Routes.ABOUT) {
                AboutScreen(
                    repository = repository,
                    onBack = { navController.popBackStack() },
                    onOpenHelp = { slug -> navController.navigate(Routes.help(slug)) },
                )
            }
            composable(Routes.QR) {
                QrScanScreen(
                    repository = repository,
                    onBack = { navController.popBackStack() },
                )
            }
        }
    }
}
