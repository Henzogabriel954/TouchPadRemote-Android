package com.touchpad.android

import android.app.Application
import android.os.Build
import android.util.Log
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import com.touchpad.android.model.ActiveSession
import com.touchpad.android.model.AndroidSettings
import com.touchpad.android.model.ComputerProfile
import com.touchpad.android.model.EdgeLink
import com.touchpad.android.model.objects
import com.touchpad.android.model.stringOrNull
import com.touchpad.android.nativecore.RustTouchpadBridge
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.channels.BufferOverflow
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.receiveAsFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import org.json.JSONArray
import org.json.JSONException
import org.json.JSONObject

data class MainUiState(
    val computers: List<ComputerProfile> = emptyList(),
    val onlineStatus: Map<String, Boolean> = emptyMap(),
    val isScanningNetwork: Boolean = false,
    val discoveredComputers: List<ComputerProfile> = emptyList(),
    val hasScannedNetwork: Boolean = false,
    val checkingComputerId: String? = null,
    val pairingComputer: ComputerProfile? = null,
    val offlineAlertComputer: ComputerProfile? = null,
    val infoErrorMessage: String? = null,
    val edgeLinks: List<EdgeLink> = emptyList(),
    val settings: AndroidSettings = AndroidSettings(),
    val activeSession: ActiveSession? = null,
    val coreAvailable: Boolean = false
)

sealed interface MainUiEvent {
    data class ActiveComputerChanged(val viaEdge: Boolean) : MainUiEvent
    data class SessionDisconnected(val reason: String) : MainUiEvent
    data class ToastMessage(val message: String) : MainUiEvent
}

class MainViewModel(application: Application) : AndroidViewModel(application) {
    private val core = RustTouchpadBridge
    private val commands = Mutex()
    private val mutableUiState = MutableStateFlow(MainUiState())
    val uiState = mutableUiState.asStateFlow()
    private val eventChannel = Channel<MainUiEvent>(capacity = 32, onBufferOverflow = BufferOverflow.DROP_OLDEST)
    val events = eventChannel.receiveAsFlow()
    private val initialized = core.isLoaded && try {
        core.nativeInit(application.filesDir.absolutePath, Build.MODEL ?: "Android")
    } catch (error: LinkageError) {
        Log.e(TAG, "Unable to initialize the Rust core", error)
        false
    }

    init {
        if (initialized) {
            viewModelScope.launch(Dispatchers.IO) {
                commands.withLock {
                    importLegacyComputers(application)
                    publishSnapshot()
                    core.nativeRefreshStatus()
                }
                while (isActive) {
                    val json = core.nativeWaitEventsJson(1_000)
                    if (json == null) {
                        delay(100)
                        continue
                    }
                    try {
                        val batch = JSONArray(json)
                        if (batch.length() == 0) continue
                        publishSnapshot()
                        batch.objects().forEach { handleEvent(it) }
                    } catch (error: JSONException) {
                        Log.e(TAG, "Invalid native event or snapshot", error)
                    }
                }
            }
        } else {
            mutableUiState.update {
                it.copy(infoErrorMessage = "Não foi possível iniciar o núcleo do TouchPad Remote.")
            }
        }
    }

    private fun publishSnapshot() {
        val json = JSONObject(core.nativeGetStateJson() ?: return)
        val hostInfo = json.getJSONObject("host_info_map")
        val computers = json.getJSONArray("computers").objects().map { saved ->
            val profile = ComputerProfile.fromJson(saved)
            val host = hostInfo.optJSONObject(profile.id)
            profile.copy(
                lastOs = host?.stringOrNull("os") ?: profile.lastOs,
                lastPrivilege = host?.stringOrNull("privilege") ?: profile.lastPrivilege
            )
        }
        val online = json.getJSONObject("online_status")
        mutableUiState.update { current ->
            current.copy(
                computers = computers,
                onlineStatus = online.keys().asSequence().associateWith { online.getBoolean(it) },
                isScanningNetwork = json.getBoolean("is_scanning_network"),
                discoveredComputers = json.getJSONArray("discovered_computers").objects()
                    .map(ComputerProfile::fromJson),
                hasScannedNetwork = json.getBoolean("has_scanned_network"),
                checkingComputerId = json.stringOrNull("checking_computer_id"),
                pairingComputer = computers.find { it.id == json.stringOrNull("pairing_computer_id") },
                edgeLinks = json.getJSONArray("edge_links").objects().map(EdgeLink::fromJson),
                settings = AndroidSettings.fromJson(json.getJSONObject("settings")),
                activeSession = json.optJSONObject("active_session")?.let(ActiveSession::fromJson),
                coreAvailable = true
            )
        }
    }

    private fun handleEvent(event: JSONObject) {
        when (event.getString("type")) {
            "ActiveComputerChanged" -> eventChannel.trySend(
                MainUiEvent.ActiveComputerChanged(viaEdge = !event.isNull("via_edge"))
            )
            "SessionDisconnected" -> eventChannel.trySend(
                MainUiEvent.SessionDisconnected(event.getString("reason"))
            )
            "ToastMessage" -> eventChannel.trySend(MainUiEvent.ToastMessage(event.getString("message")))
            "PairingRejected" -> mutableUiState.update {
                it.copy(infoErrorMessage = "O pareamento foi recusado em ${event.getString("computer_name")}.")
            }
            "ComputerOffline" -> mutableUiState.update { state ->
                state.copy(offlineAlertComputer = state.computers.find {
                    it.id == event.getString("computer_id")
                })
            }
        }
    }

    private fun importLegacyComputers(application: Application) {
        val prefs = application.getSharedPreferences("touchpad_remote_prefs", Application.MODE_PRIVATE)
        val saved = prefs.getString("saved_computers", null) ?: return
        val previous = try {
            JSONArray(saved).objects().map(ComputerProfile::fromJson).filter { it.ipAddress.isNotBlank() }
        } catch (error: JSONException) {
            Log.w(TAG, "Unable to read previous computer profiles", error)
            return
        }
        val imported = previous.all { profile ->
            val result = core.nativeAddOrUpdateComputerJson(profile.toJson().toString())
            JSONObject(result ?: "{}").optJSONArray("computers").objects().any {
                val current = ComputerProfile.fromJson(it)
                current.ipAddress.equals(profile.ipAddress, ignoreCase = true) && current.port == profile.port
            }
        }
        if (imported) prefs.edit().remove("saved_computers").apply()
    }

    private fun command(action: () -> Unit) {
        if (!initialized) return
        viewModelScope.launch {
            commands.withLock { withContext(Dispatchers.IO) { action() } }
        }
    }

    fun selectComputer(computer: ComputerProfile) = command {
        val current = uiState.value
        val multiPcEnabled = current.settings.multiPcEnabled && current.edgeLinks.any {
            it.fromComputerId == computer.id || it.toComputerId == computer.id
        }
        core.nativeStartSession(computer.id, multiPcEnabled)
    }
    fun cancelPairing() = command(core::nativeCancelPairing)
    fun exitTouchpad() = command(core::nativeExitSession)
    fun refreshStatus() = command(core::nativeRefreshStatus)
    fun discoverComputers() = command { core.nativeDiscoverComputers(1_200) }
    fun clearDiscovery() = command(core::nativeClearDiscovery)
    fun removeComputer(id: String) = command { core.nativeRemoveComputer(id) }
    fun addComputer(computer: ComputerProfile) = command {
        core.nativeAddOrUpdateComputerJson(computer.toJson().toString())
        core.nativeRefreshStatus()
    }

    fun dismissOfflineAlert() = mutableUiState.update { it.copy(offlineAlertComputer = null) }
    fun dismissInfoError() = mutableUiState.update { it.copy(infoErrorMessage = null) }

    fun switchActiveComputer(id: String) = command {
        if (!core.nativeSwitchActiveComputer(id)) {
            eventChannel.trySend(MainUiEvent.ToastMessage("Este computador está indisponível."))
        }
    }

    fun setEdgeLock(locked: Boolean) = command { core.nativeSetEdgeLock(locked) }

    fun saveSettings(settings: AndroidSettings) = command {
        core.nativeSaveSettingsJson(
            JSONObject()
                .put("local_sensitivity", settings.localSensitivity.toDouble())
                .put("hold_duration_ms", settings.holdDurationMs)
                .put("haptics_enabled", settings.hapticsEnabled)
                .put("default_landscape", settings.defaultLandscape)
                .toString()
        )
    }

    fun saveLayout(links: List<EdgeLink>, enabled: Boolean, overshoot: Int, primaryId: String?) = command {
        val savedLinks = core.nativeSaveEdgeLinksJson(JSONArray(links.map(EdgeLink::toJson)).toString())
        val savedSettings = core.nativeSaveSettingsJson(
            JSONObject().put("multi_pc_enabled", enabled).put("edge_overshoot_px", overshoot).toString()
        )
        if (primaryId != null && enabled &&
            savedLinks != null && savedSettings != null
        ) {
            val persisted = JSONObject(savedSettings)
            val settings = persisted.getJSONObject("settings")
            val actualLinks = persisted.getJSONArray("edge_links").objects().map(EdgeLink::fromJson)
            if (settings.getBoolean("multi_pc_enabled") &&
                settings.getInt("edge_overshoot_px") == overshoot &&
                actualLinks.toSet() == links.toSet()
            ) {
                core.nativeStartSession(primaryId, true)
            }
        }
    }

    override fun onCleared() {
        if (initialized) core.nativeShutdown()
        super.onCleared()
    }

    private companion object {
        const val TAG = "MainViewModel"
    }
}
