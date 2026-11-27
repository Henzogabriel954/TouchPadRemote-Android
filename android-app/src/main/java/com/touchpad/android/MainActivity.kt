package com.touchpad.android

import android.os.Bundle
import android.view.HapticFeedbackConstants
import android.widget.Toast
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.graphics.Color
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.repeatOnLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import com.touchpad.android.ui.ComputerListScreen
import com.touchpad.android.ui.TouchpadFullscreenScreen

private val TouchPadDarkColorScheme = darkColorScheme(
    primary = Color(0xFF38BDF8),
    onPrimary = Color(0xFF041E30),
    primaryContainer = Color(0xFF0C4A6E),
    onPrimaryContainer = Color(0xFFE0F2FE),
    secondary = Color(0xFF818CF8),
    background = Color(0xFF0B1120),
    onBackground = Color(0xFFF8FAFC),
    surface = Color(0xFF131C31),
    onSurface = Color(0xFFF8FAFC),
    surfaceVariant = Color(0xFF1E293B),
    onSurfaceVariant = Color(0xFF94A3B8),
    outline = Color(0xFF334155),
    error = Color(0xFFF87171)
)

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContent {
            MaterialTheme(colorScheme = TouchPadDarkColorScheme) {
                val mainViewModel: MainViewModel = viewModel()
                val state by mainViewModel.uiState.collectAsState()

                LaunchedEffect(mainViewModel) {
                    lifecycle.repeatOnLifecycle(Lifecycle.State.STARTED) {
                        mainViewModel.events.collect { event ->
                            when (event) {
                                is MainUiEvent.ActiveComputerChanged -> {
                                    val latest = mainViewModel.uiState.value
                                    if (latest.settings.hapticsEnabled &&
                                        latest.activeSession?.multiPcActive == true
                                    ) {
                                        window.decorView.performHapticFeedback(
                                            if (event.viaEdge) HapticFeedbackConstants.LONG_PRESS
                                            else HapticFeedbackConstants.CLOCK_TICK
                                        )
                                    }
                                }
                                is MainUiEvent.SessionDisconnected ->
                                    Toast.makeText(this@MainActivity, event.reason, Toast.LENGTH_LONG).show()
                                is MainUiEvent.ToastMessage ->
                                    Toast.makeText(this@MainActivity, event.message, Toast.LENGTH_SHORT).show()
                            }
                        }
                    }
                }

                val session = state.activeSession?.takeIf { it.connected }
                if (session == null) {
                    ComputerListScreen(
                        computers = state.computers,
                        onlineStatus = state.onlineStatus,
                        isScanningNetwork = state.isScanningNetwork,
                        discoveredComputers = state.discoveredComputers,
                        hasScannedNetwork = state.hasScannedNetwork,
                        checkingComputerId = state.checkingComputerId,
                        pairingComputer = state.pairingComputer,
                        offlineAlertComputer = state.offlineAlertComputer,
                        infoErrorMessage = state.infoErrorMessage,
                        settings = state.settings,
                        edgeLinks = state.edgeLinks,
                        coreAvailable = state.coreAvailable,
                        onSelectComputer = mainViewModel::selectComputer,
                        onAddComputer = mainViewModel::addComputer,
                        onRemoveComputer = mainViewModel::removeComputer,
                        onRefreshStatus = mainViewModel::refreshStatus,
                        onDiscoverNetwork = mainViewModel::discoverComputers,
                        onClearDiscovery = mainViewModel::clearDiscovery,
                        onCancelPairing = mainViewModel::cancelPairing,
                        onDismissOfflineAlert = mainViewModel::dismissOfflineAlert,
                        onDismissInfoError = mainViewModel::dismissInfoError,
                        onSaveSettings = mainViewModel::saveSettings,
                        onSaveLayout = mainViewModel::saveLayout
                    )
                } else {
                    val outgoing = state.edgeLinks.filter { it.fromComputerId == session.activeComputerId }
                    val neighborIds = outgoing.mapTo(mutableSetOf()) { it.toComputerId }
                    state.edgeLinks.filter { link ->
                        link.toComputerId == session.activeComputerId &&
                            outgoing.none { it.fromEdge == link.toEdge }
                    }.mapTo(neighborIds) { it.fromComputerId }
                    TouchpadFullscreenScreen(
                        session = session,
                        neighbors = session.peers.filter { it.id in neighborIds },
                        settings = state.settings,
                        onSwitchActiveComputer = mainViewModel::switchActiveComputer,
                        onSetEdgeLock = mainViewModel::setEdgeLock,
                        onExit = mainViewModel::exitTouchpad
                    )
                }
            }
        }
    }
}
