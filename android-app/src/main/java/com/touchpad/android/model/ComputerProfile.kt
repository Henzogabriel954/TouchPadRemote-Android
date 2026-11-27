package com.touchpad.android.model

import org.json.JSONObject

const val DEFAULT_PORT = 45454

enum class ConnectionMethod {
    NETWORK_LAN, BLUETOOTH, USB_ADB;

    val id: String get() = name
    val isAvailableNow: Boolean get() = this == NETWORK_LAN

    companion object {
        fun fromId(id: String) = entries.firstOrNull { it.id.equals(id, ignoreCase = true) }
    }
}

data class ComputerProfile(
    val id: String = "",
    val name: String,
    val ipAddress: String,
    val port: Int = DEFAULT_PORT,
    val connectionMethodId: String = ConnectionMethod.NETWORK_LAN.id,
    val lastOs: String? = null,
    val lastPrivilege: String? = null
) {
    fun toJson() = JSONObject()
        .put("id", id.takeIf { it.isNotBlank() })
        .put("name", name)
        .put("ip_address", ipAddress)
        .put("port", port)
        .put("connection_method_id", connectionMethodId)
        .put("last_os", lastOs)
        .put("last_privilege", lastPrivilege)

    companion object {
        fun fromJson(json: JSONObject): ComputerProfile {
            val host = json.optString("ip_address", json.optString("ipAddress"))
            val port = json.optInt("port", DEFAULT_PORT).takeIf { it in 1..65535 } ?: DEFAULT_PORT
            return ComputerProfile(
                id = json.stringOrNull("id") ?: "$host:$port",
                name = json.stringOrNull("name") ?: host,
                ipAddress = host,
                port = port,
                connectionMethodId = json.optString(
                    "connection_method_id", json.optString("connectionMethodId", ConnectionMethod.NETWORK_LAN.id)
                ),
                lastOs = json.stringOrNull("last_os"),
                lastPrivilege = json.stringOrNull("last_privilege")
            )
        }
    }
}

internal fun JSONObject.stringOrNull(key: String): String? =
    if (isNull(key)) null else optString(key).takeIf(String::isNotBlank)
