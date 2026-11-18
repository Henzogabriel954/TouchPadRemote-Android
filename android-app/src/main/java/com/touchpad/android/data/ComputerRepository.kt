package com.touchpad.android.data

import android.content.Context
import com.google.gson.Gson
import com.google.gson.reflect.TypeToken
import com.touchpad.android.model.ComputerProfile
import com.touchpad.protocol.ConnectionMethod
import com.touchpad.protocol.TouchpadPacket
import java.util.UUID

/**
 * Local persistence layer for registered [ComputerProfile] entries backed by SharedPreferences and Gson.
 */
class ComputerRepository(context: Context) {

    private val prefs = context.applicationContext.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)
    private val gson = Gson()
    private val lock = Any()
    private val listType = object : TypeToken<List<ComputerProfile>>() {}.type

    fun loadAll(): List<ComputerProfile> = synchronized(lock) {
        val raw = prefs.getString(KEY_COMPUTERS, null).orEmpty()
        if (raw.isBlank()) return emptyList()

        return try {
            val parsed: List<ComputerProfile?> = gson.fromJson(raw, listType) ?: emptyList()
            parsed.mapNotNull { item ->
                item?.sanitize()
            }
        } catch (_: Exception) {
            emptyList()
        }
    }

    fun saveAll(computers: List<ComputerProfile>) = synchronized(lock) {
        val sanitized = computers.map { it.sanitize() }
        prefs.edit()
            .putString(KEY_COMPUTERS, gson.toJson(sanitized))
            .apply()
    }

    fun add(computer: ComputerProfile): List<ComputerProfile> = synchronized(lock) {
        val current = loadAll().toMutableList()
        val sanitized = computer.sanitize()
        val existingIndex = current.indexOfFirst {
            it.id == sanitized.id ||
                (it.ipAddress.equals(sanitized.ipAddress, ignoreCase = true) && it.port == sanitized.port)
        }
        if (existingIndex >= 0) {
            val preservedId = current[existingIndex].id
            current[existingIndex] = sanitized.copy(id = preservedId)
        } else {
            current.add(sanitized)
        }
        saveAll(current)
        current.toList()
    }

    fun update(computer: ComputerProfile): List<ComputerProfile> = synchronized(lock) {
        val current = loadAll().toMutableList()
        val sanitized = computer.sanitize()
        val index = current.indexOfFirst { it.id == sanitized.id }
        if (index >= 0) {
            current[index] = sanitized
            saveAll(current)
        }
        current.toList()
    }

    fun remove(id: String): List<ComputerProfile> = synchronized(lock) {
        val updated = loadAll().filterNot { it.id == id }
        saveAll(updated)
        updated
    }

    fun getOrCreateClientId(): String = synchronized(lock) {
        val existing = prefs.getString(KEY_CLIENT_ID, null)?.trim()
        if (!existing.isNullOrBlank()) return@synchronized existing

        val created = UUID.randomUUID().toString()
        prefs.edit()
            .putString(KEY_CLIENT_ID, created)
            .apply()
        created
    }

    @Suppress("UNNECESSARY_SAFE_CALL", "USELESS_ELVIS")
    private fun ComputerProfile.sanitize(): ComputerProfile {
        val safeId = (id as String?)?.takeIf { it.isNotBlank() } ?: UUID.randomUUID().toString()
        val safeName = (name as String?)?.trim().orEmpty().ifBlank { ipAddress.trim() }
        val safeIp = (ipAddress as String?)?.trim().orEmpty()
        val safePort = port.takeIf { it in 1..65535 } ?: TouchpadPacket.DEFAULT_PORT
        val safeMethod = (connectionMethodId as String?)?.takeIf { it.isNotBlank() }
            ?: ConnectionMethod.NETWORK_LAN.id

        return ComputerProfile(
            id = safeId,
            name = safeName,
            ipAddress = safeIp,
            port = safePort,
            connectionMethodId = safeMethod
        )
    }

    private companion object {
        const val PREFS_NAME = "touchpad_remote_prefs"
        const val KEY_COMPUTERS = "saved_computers"
        const val KEY_CLIENT_ID = "client_id"
    }
}