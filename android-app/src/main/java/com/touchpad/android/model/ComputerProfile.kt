package com.touchpad.android.model

import com.google.gson.annotations.SerializedName
import com.touchpad.protocol.ConnectionMethod
import com.touchpad.protocol.TouchpadPacket
import java.util.UUID

/**
 * Saved target computer profile shown in the home grid and used for touchpad sessions.
 */
data class ComputerProfile(
    @SerializedName("id")
    val id: String = UUID.randomUUID().toString(),
    @SerializedName("name")
    val name: String,
    @SerializedName("ipAddress")
    val ipAddress: String,
    @SerializedName("port")
    val port: Int = TouchpadPacket.DEFAULT_PORT,
    @SerializedName("connectionMethodId")
    val connectionMethodId: String = ConnectionMethod.NETWORK_LAN.id
)