package com.touchpad.android.model

import org.json.JSONArray
import org.json.JSONObject

enum class ScreenEdge(val label: String, val arrow: String) {
    LEFT("Esquerda", "←"),
    RIGHT("Direita", "→"),
    TOP("Superior", "↑"),
    BOTTOM("Inferior", "↓");

    val opposite: ScreenEdge get() = when (this) {
        LEFT -> RIGHT
        RIGHT -> LEFT
        TOP -> BOTTOM
        BOTTOM -> TOP
    }
}

data class EdgeLink(
    val fromComputerId: String,
    val fromEdge: ScreenEdge,
    val toComputerId: String,
    val toEdge: ScreenEdge
) {
    fun reversed() = EdgeLink(toComputerId, toEdge, fromComputerId, fromEdge)

    fun toJson() = JSONObject()
        .put("from_computer_id", fromComputerId)
        .put("from_edge", fromEdge.name)
        .put("to_computer_id", toComputerId)
        .put("to_edge", toEdge.name)

    companion object {
        fun fromJson(json: JSONObject) = EdgeLink(
            json.getString("from_computer_id"),
            ScreenEdge.valueOf(json.getString("from_edge")),
            json.getString("to_computer_id"),
            ScreenEdge.valueOf(json.getString("to_edge"))
        )
    }
}

data class AndroidSettings(
    val multiPcEnabled: Boolean = false,
    val holdDurationMs: Int = 260,
    val hapticsEnabled: Boolean = true,
    val defaultLandscape: Boolean = true,
    val edgeOvershootPx: Int = 28,
    val localSensitivity: Float = 1f
) {
    companion object {
        fun fromJson(json: JSONObject) = AndroidSettings(
            multiPcEnabled = json.getBoolean("multi_pc_enabled"),
            holdDurationMs = json.getInt("hold_duration_ms"),
            hapticsEnabled = json.getBoolean("haptics_enabled"),
            defaultLandscape = json.getBoolean("default_landscape"),
            edgeOvershootPx = json.getInt("edge_overshoot_px"),
            localSensitivity = json.getDouble("local_sensitivity").toFloat()
        )
    }
}

data class SessionPeer(
    val id: String,
    val name: String,
    val online: Boolean,
    val pairingStatus: String?
)

data class ActiveSession(
    val primaryComputerId: String,
    val activeComputerId: String,
    val activeComputerName: String,
    val multiPcActive: Boolean,
    val edgeLocked: Boolean,
    val connected: Boolean,
    val peers: List<SessionPeer>
) {
    companion object {
        fun fromJson(json: JSONObject) = ActiveSession(
            primaryComputerId = json.getString("primary_computer_id"),
            activeComputerId = json.getString("active_computer_id"),
            activeComputerName = json.getString("active_computer_name"),
            multiPcActive = json.getBoolean("multi_pc_enabled"),
            edgeLocked = json.getBoolean("edge_locked"),
            connected = json.getBoolean("connected"),
            peers = json.getJSONArray("peers").objects().map { peer ->
                SessionPeer(
                    peer.getString("computer_id"),
                    peer.getString("name"),
                    peer.getBoolean("online"),
                    peer.stringOrNull("pairing_status")
                )
            }
        )
    }
}

internal fun JSONArray?.objects(): List<JSONObject> =
    if (this == null) emptyList() else (0 until length()).mapNotNull { optJSONObject(it) }
