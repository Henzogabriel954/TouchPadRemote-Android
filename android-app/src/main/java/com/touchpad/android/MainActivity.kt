package com.touchpad.android

import android.app.Application
import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.graphics.Color
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import androidx.lifecycle.viewmodel.compose.viewModel
import com.touchpad.android.data.ComputerRepository
import com.touchpad.android.model.ComputerProfile
import com.touchpad.android.network.TouchpadUdpClient
import com.touchpad.android.ui.ComputerListScreen
import com.touchpad.android.ui.TouchpadFullscreenScreen
import com.touchpad.protocol.MouseButton
import com.touchpad.protocol.PairingStatus
import com.touchpad.protocol.TouchpadPacket
import kotlinx.coroutines.Job
import kotlinx.coroutines.async
import kotlinx.coroutines.awaitAll
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch

data class MainUiState(
    val computers: List<ComputerProfile> = emptyList(),
    val selectedComputer: ComputerProfile? = null,
    val onlineStatus: Map<String, Boolean> = emptyMap(),
    val isScanningNetwork: Boolean = false,
    val discoveredComputers: List<ComputerProfile> = emptyList(),
    val hasScannedNetwork: Boolean = false,
    val checkingComputerId: String? = null,
    val pairingComputer: ComputerProfile? = null,
    val offlineAlertComputer: ComputerProfile? = null,
    val infoErrorMessage: String? = null
)

class MainViewModel(application: Application) : AndroidViewModel(application) {

    private val repository = ComputerRepository(application)
    private val udpClient = TouchpadUdpClient()
    private val clientId: String = repository.getOrCreateClientId()
    private val clientName: String =
        Build.MODEL?.takeIf { it.isNotBlank() } ?: "Celular Android"

    private var refreshJob: Job? = null
    private var pairingJob: Job? = null
    private var discoverJob: Job? = null
    private var pairingToken: Long = 0L

    private val _uiState = MutableStateFlow(
        MainUiState(computers = repository.loadAll())
    )
    val uiState: StateFlow<MainUiState> = _uiState.asStateFlow()

    init {
        refreshStatus()
    }

    fun selectComputer(computer: ComputerProfile) {
        pairingJob?.cancel()
        val token = ++pairingToken
        pairingJob = viewModelScope.launch {
            _uiState.update {
                it.copy(
                    checkingComputerId = computer.id,
                    pairingComputer = null,
                    offlineAlertComputer = null,
                    infoErrorMessage = null
                )
            }

            try {
                val status = udpClient.requestPairing(
                    ip = computer.ipAddress,
                    port = computer.port,
                    clientId = clientId,
                    clientName = clientName,
                    timeoutMs = 800
                )

                if (pairingToken != token) return@launch
                _uiState.update { it.copy(checkingComputerId = null) }

                when (status) {
                    null -> {
                        _uiState.update { state ->
                            state.copy(
                                onlineStatus = state.onlineStatus + (computer.id to false),
                                offlineAlertComputer = computer
                            )
                        }
                    }

                    PairingStatus.APPROVED -> {
                        _uiState.update { state ->
                            state.copy(
                                onlineStatus = state.onlineStatus + (computer.id to true),
                                selectedComputer = computer
                            )
                        }
                    }

                    PairingStatus.REJECTED -> {
                        _uiState.update {
                            it.copy(
                                infoErrorMessage = "O pareamento foi recusado em \"${computer.name}\". Clique novamente no quadrado e aceite na janela do Windows."
                            )
                        }
                    }

                    PairingStatus.PENDING -> {
                        _uiState.update { state ->
                            state.copy(
                                onlineStatus = state.onlineStatus + (computer.id to true),
                                pairingComputer = computer
                            )
                        }

                        var consecutiveNulls = 0
                        while (isActive) {
                            delay(900L)
                            val pollStatus = udpClient.requestPairing(
                                ip = computer.ipAddress,
                                port = computer.port,
                                clientId = clientId,
                                clientName = clientName,
                                timeoutMs = 800
                            )
                            if (pairingToken != token) break

                            when (pollStatus) {
                                PairingStatus.APPROVED -> {
                                    _uiState.update { state ->
                                        state.copy(
                                            pairingComputer = null,
                                            onlineStatus = state.onlineStatus + (computer.id to true),
                                            selectedComputer = computer
                                        )
                                    }
                                    break
                                }

                                PairingStatus.REJECTED -> {
                                    _uiState.update {
                                        it.copy(
                                            pairingComputer = null,
                                            infoErrorMessage = "O pareamento foi recusado no computador Windows."
                                        )
                                    }
                                    break
                                }

                                null -> {
                                    consecutiveNulls++
                                    if (consecutiveNulls >= 3) {
                                        _uiState.update { state ->
                                            state.copy(
                                                pairingComputer = null,
                                                onlineStatus = state.onlineStatus + (computer.id to false),
                                                offlineAlertComputer = computer
                                            )
                                        }
                                        break
                                    }
                                }

                                PairingStatus.PENDING -> {
                                    consecutiveNulls = 0
                                }
                            }
                        }
                    }
                }
            } finally {
                if (pairingToken == token) {
                    _uiState.update { it.copy(checkingComputerId = null) }
                }
            }
        }
    }

    fun cancelPairing() {
        pairingToken++
        pairingJob?.cancel()
        pairingJob = null
        _uiState.update {
            it.copy(
                checkingComputerId = null,
                pairingComputer = null
            )
        }
    }

    fun dismissOfflineAlert() {
        _uiState.update { it.copy(offlineAlertComputer = null) }
    }

    fun dismissInfoError() {
        _uiState.update { it.copy(infoErrorMessage = null) }
    }

    fun exitTouchpad() {
        val active = _uiState.value.selectedComputer
        _uiState.update { it.copy(selectedComputer = null) }
        if (active != null) {
            viewModelScope.launch {
                udpClient.sendPacket(
                    active.ipAddress,
                    active.port,
                    TouchpadPacket.ButtonState(MouseButton.LEFT, pressed = false)
                )
                udpClient.sendPacket(
                    active.ipAddress,
                    active.port,
                    TouchpadPacket.ButtonState(MouseButton.RIGHT, pressed = false)
                )
            }
        }
        refreshStatus(force = true)
    }

    fun addComputer(computer: ComputerProfile) {
        val updated = repository.add(computer)
        _uiState.update { it.copy(computers = updated) }
        refreshStatus(force = true)
    }

    fun removeComputer(id: String) {
        if (_uiState.value.checkingComputerId == id || _uiState.value.pairingComputer?.id == id) {
            pairingToken++
            pairingJob?.cancel()
            pairingJob = null
        }
        val updated = repository.remove(id)
        _uiState.update { state ->
            state.copy(
                computers = updated,
                selectedComputer = state.selectedComputer?.takeIf { it.id != id },
                checkingComputerId = state.checkingComputerId?.takeIf { it != id },
                pairingComputer = state.pairingComputer?.takeIf { it.id != id },
                offlineAlertComputer = state.offlineAlertComputer?.takeIf { it.id != id },
                onlineStatus = state.onlineStatus - id
            )
        }
    }

    fun refreshStatus(force: Boolean = false) {
        if (!force && refreshJob?.isActive == true) return
        if (force) {
            refreshJob?.cancel()
        }
        val targets = _uiState.value.computers
        if (targets.isEmpty()) {
            _uiState.update { it.copy(onlineStatus = emptyMap()) }
            return
        }

        refreshJob = viewModelScope.launch {
            val statuses = targets.map { computer ->
                async {
                    computer.id to udpClient.ping(computer.ipAddress, computer.port, timeoutMs = 600)
                }
            }.awaitAll().toMap()

            _uiState.update { state ->
                val validIds = state.computers.mapTo(HashSet()) { it.id }
                state.copy(onlineStatus = statuses.filterKeys { it in validIds })
            }
        }
    }

    fun discoverComputers() {
        if (_uiState.value.isScanningNetwork) return

        discoverJob?.cancel()
        discoverJob = viewModelScope.launch {
            _uiState.update {
                it.copy(
                    isScanningNetwork = true,
                    discoveredComputers = emptyList()
                )
            }
            try {
                val found = udpClient.discoverComputers(
                    port = TouchpadPacket.DEFAULT_PORT,
                    timeoutMs = 1200
                )
                _uiState.update {
                    it.copy(
                        discoveredComputers = found,
                        hasScannedNetwork = true
                    )
                }
            } finally {
                _uiState.update { it.copy(isScanningNetwork = false) }
            }
        }
    }

    fun clearDiscovery() {
        discoverJob?.cancel()
        discoverJob = null
        _uiState.update {
            it.copy(
                isScanningNetwork = false,
                discoveredComputers = emptyList(),
                hasScannedNetwork = false
            )
        }
    }

    suspend fun sendPacket(computer: ComputerProfile, packet: TouchpadPacket) {
        udpClient.sendPacket(computer.ipAddress, computer.port, packet)
    }

    suspend fun pingComputer(computer: ComputerProfile): Boolean {
        val status = udpClient.requestPairing(
            ip = computer.ipAddress,
            port = computer.port,
            clientId = clientId,
            clientName = clientName,
            timeoutMs = 800
        )
        if (status == PairingStatus.REJECTED || status == PairingStatus.PENDING) {
            _uiState.update { state ->
                val isCurrentActive = state.selectedComputer?.id == computer.id
                state.copy(
                    selectedComputer = if (isCurrentActive) null else state.selectedComputer,
                    infoErrorMessage = if (isCurrentActive) {
                        "A sessão com \"${computer.name}\" foi desconectada no computador Windows."
                    } else {
                        state.infoErrorMessage
                    }
                )
            }
            viewModelScope.launch {
                udpClient.sendPacket(
                    computer.ipAddress,
                    computer.port,
                    TouchpadPacket.ButtonState(MouseButton.LEFT, pressed = false)
                )
                udpClient.sendPacket(
                    computer.ipAddress,
                    computer.port,
                    TouchpadPacket.ButtonState(MouseButton.RIGHT, pressed = false)
                )
            }
            return false
        }
        return status == PairingStatus.APPROVED
    }

    override fun onCleared() {
        refreshJob?.cancel()
        pairingJob?.cancel()
        discoverJob?.cancel()
        udpClient.close()
        super.onCleared()
    }
}

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

@Composable
private fun TouchPadRemoteTheme(content: @Composable () -> Unit) {
    MaterialTheme(
        colorScheme = TouchPadDarkColorScheme,
        content = content
    )
}

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContent {
            TouchPadRemoteTheme {
                val mainViewModel: MainViewModel = viewModel()
                val uiState by mainViewModel.uiState.collectAsState()

                val activeComputer = uiState.selectedComputer
                if (activeComputer == null) {
                    ComputerListScreen(
                        computers = uiState.computers,
                        onlineStatus = uiState.onlineStatus,
                        isScanningNetwork = uiState.isScanningNetwork,
                        discoveredComputers = uiState.discoveredComputers,
                        hasScannedNetwork = uiState.hasScannedNetwork,
                        checkingComputerId = uiState.checkingComputerId,
                        pairingComputer = uiState.pairingComputer,
                        offlineAlertComputer = uiState.offlineAlertComputer,
                        infoErrorMessage = uiState.infoErrorMessage,
                        onSelectComputer = mainViewModel::selectComputer,
                        onAddComputer = mainViewModel::addComputer,
                        onRemoveComputer = mainViewModel::removeComputer,
                        onRefreshStatus = mainViewModel::refreshStatus,
                        onDiscoverNetwork = mainViewModel::discoverComputers,
                        onClearDiscovery = mainViewModel::clearDiscovery,
                        onCancelPairing = mainViewModel::cancelPairing,
                        onDismissOfflineAlert = mainViewModel::dismissOfflineAlert,
                        onDismissInfoError = mainViewModel::dismissInfoError
                    )
                } else {
                    TouchpadFullscreenScreen(
                        computer = activeComputer,
                        onSendPacket = { packet ->
                            mainViewModel.sendPacket(activeComputer, packet)
                        },
                        onPingComputer = {
                            mainViewModel.pingComputer(activeComputer)
                        },
                        onExit = mainViewModel::exitTouchpad
                    )
                }
            }
        }
    }
}