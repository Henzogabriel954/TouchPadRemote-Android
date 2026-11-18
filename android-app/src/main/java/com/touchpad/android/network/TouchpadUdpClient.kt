package com.touchpad.android.network

import android.os.Build
import com.touchpad.android.model.ComputerProfile
import com.touchpad.protocol.PairingStatus
import com.touchpad.protocol.TouchpadPacket
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.withContext
import java.net.DatagramPacket
import java.net.DatagramSocket
import java.net.InetAddress
import java.net.NetworkInterface
import java.net.SocketTimeoutException
import java.util.concurrent.ConcurrentHashMap
import kotlin.coroutines.coroutineContext

/**
 * Coroutine-safe UDP transport client for streaming touchpad packets, pinging hosts,
 * and discovering active desktop servers on the local network.
 */
class TouchpadUdpClient {

    private val socketLock = Any()
    private val addressCache = ConcurrentHashMap<String, InetAddress>()
    private val activeEphemeralSockets = ConcurrentHashMap.newKeySet<DatagramSocket>()

    @Volatile
    private var sendSocket: DatagramSocket? = null

    @Volatile
    private var closed = false

    suspend fun sendPacket(ip: String, port: Int, packet: TouchpadPacket) {
        if (closed || ip.isBlank() || port !in 1..65535) return

        withContext(Dispatchers.IO) {
            var activeSocket: DatagramSocket? = null
            try {
                val payload = packet.toByteArray()
                val address = resolveAddress(ip)
                val datagram = DatagramPacket(payload, payload.size, address, port)
                val socket = getOrCreateSendSocket() ?: return@withContext
                activeSocket = socket
                socket.send(datagram)
            } catch (e: CancellationException) {
                throw e
            } catch (_: Exception) {
                addressCache.remove(ip.trim())
                invalidateSendSocket(activeSocket)
            }
        }
    }

    suspend fun ping(ip: String, port: Int, timeoutMs: Int = 800): Boolean {
        val timestamp = System.currentTimeMillis()
        return sendAndAwaitReply(
            ip = ip,
            port = port,
            packet = TouchpadPacket.Ping(timestamp),
            timeoutMs = timeoutMs
        ) { response ->
            if (response is TouchpadPacket.Pong && response.timestamp == timestamp) true else null
        } ?: false
    }

    suspend fun requestPairing(
        ip: String,
        port: Int,
        clientId: String,
        clientName: String,
        timeoutMs: Int = 800
    ): PairingStatus? {
        return sendAndAwaitReply(
            ip = ip,
            port = port,
            packet = TouchpadPacket.PairRequest(clientId = clientId, clientName = clientName),
            timeoutMs = timeoutMs
        ) { response ->
            (response as? TouchpadPacket.PairResponse)?.status
        }
    }

    private suspend fun <T> sendAndAwaitReply(
        ip: String,
        port: Int,
        packet: TouchpadPacket,
        timeoutMs: Int,
        matcher: (TouchpadPacket) -> T?
    ): T? {
        if (closed || ip.isBlank() || port !in 1..65535) return null

        return withContext(Dispatchers.IO) {
            var socket: DatagramSocket? = null
            var cancelHandle: kotlinx.coroutines.DisposableHandle? = null
            try {
                val createdSocket = synchronized(socketLock) {
                    if (closed) null else DatagramSocket().also { activeEphemeralSockets.add(it) }
                } ?: return@withContext null
                socket = createdSocket
                cancelHandle = coroutineContext[Job]?.invokeOnCompletion {
                    runCatching { createdSocket.close() }
                }

                val payload = packet.toByteArray()
                val address = resolveAddress(ip)
                createdSocket.send(DatagramPacket(payload, payload.size, address, port))

                val buffer = ByteArray(RECEIVE_BUFFER_SIZE)
                val timeoutNanos = timeoutMs.coerceAtLeast(100) * 1_000_000L
                val deadlineNanos = System.nanoTime() + timeoutNanos

                while (true) {
                    val remainingMs = ((deadlineNanos - System.nanoTime()) / 1_000_000L).toInt()
                    if (remainingMs <= 0) return@withContext null

                    createdSocket.soTimeout = remainingMs
                    val receivePacket = DatagramPacket(buffer, buffer.size)
                    try {
                        createdSocket.receive(receivePacket)
                    } catch (_: SocketTimeoutException) {
                        return@withContext null
                    }

                    if (receivePacket.port != port) continue
                    val senderAddress = receivePacket.address
                    if (senderAddress != address && senderAddress?.hostAddress != address.hostAddress) {
                        continue
                    }

                    val response = decodePacketSafely(receivePacket.data, receivePacket.length)
                    val matched = response?.let(matcher)
                    if (matched != null) {
                        return@withContext matched
                    }
                }

                @Suppress("UNREACHABLE_CODE")
                null
            } catch (e: CancellationException) {
                throw e
            } catch (_: Exception) {
                null
            } finally {
                cancelHandle?.dispose()
                socket?.let {
                    synchronized(socketLock) {
                        activeEphemeralSockets.remove(it)
                    }
                    runCatching { it.close() }
                }
            }
        }
    }

    suspend fun discoverComputers(
        port: Int = TouchpadPacket.DEFAULT_PORT,
        timeoutMs: Int = 1200
    ): List<ComputerProfile> {
        if (closed || port !in 1..65535) return emptyList()

        return withContext(Dispatchers.IO) {
            var socket: DatagramSocket? = null
            var cancelHandle: kotlinx.coroutines.DisposableHandle? = null
            val discovered = LinkedHashMap<String, ComputerProfile>()

            try {
                val createdSocket = synchronized(socketLock) {
                    if (closed) {
                        null
                    } else {
                        DatagramSocket().apply {
                            broadcast = true
                        }.also { activeEphemeralSockets.add(it) }
                    }
                } ?: return@withContext emptyList()
                socket = createdSocket
                cancelHandle = coroutineContext[Job]?.invokeOnCompletion {
                    runCatching { createdSocket.close() }
                }

                val clientName = Build.MODEL?.takeIf { it.isNotBlank() } ?: "Android"
                val requestBytes = TouchpadPacket.DiscoverRequest(clientName).toByteArray()
                val broadcastTargets = collectBroadcastAddresses()

                sendDiscoveryBurst(createdSocket, requestBytes, broadcastTargets, port)

                val buffer = ByteArray(RECEIVE_BUFFER_SIZE)
                val totalTimeoutMs = timeoutMs.coerceAtLeast(250).toLong()
                val retryDelayMs = (totalTimeoutMs / 3L).coerceIn(120L, 350L)
                val startNanos = System.nanoTime()
                val deadlineNanos = startNanos + totalTimeoutMs * 1_000_000L
                val retryAtNanos = startNanos + retryDelayMs * 1_000_000L
                var retrySent = false

                while (true) {
                    val nowNanos = System.nanoTime()
                    val remainingMs = ((deadlineNanos - nowNanos) / 1_000_000L).toInt()
                    if (remainingMs <= 0) break

                    val sliceTimeoutMs = if (!retrySent) {
                        val untilRetryMs = ((retryAtNanos - nowNanos) / 1_000_000L).toInt()
                        if (untilRetryMs <= 0) {
                            retrySent = true
                            if (remainingMs > 40) {
                                sendDiscoveryBurst(createdSocket, requestBytes, broadcastTargets, port)
                            }
                            remainingMs
                        } else {
                            minOf(untilRetryMs, remainingMs).coerceAtLeast(1)
                        }
                    } else {
                        remainingMs
                    }

                    createdSocket.soTimeout = sliceTimeoutMs
                    val receivePacket = DatagramPacket(buffer, buffer.size)
                    try {
                        createdSocket.receive(receivePacket)
                        val packet = decodePacketSafely(receivePacket.data, receivePacket.length)
                        if (packet is TouchpadPacket.DiscoverResponse) {
                            val senderIp = receivePacket.address?.hostAddress
                                ?.takeIf { it.isNotBlank() && it != "0.0.0.0" }
                            val resolvedIp = senderIp ?: packet.ipAddress.trim()
                            val resolvedPort = packet.port.takeIf { it in 1..65535 } ?: port
                            if (resolvedIp.isNotBlank()) {
                                val key = "$resolvedIp:$resolvedPort"
                                val displayName = packet.computerName.trim().ifBlank { resolvedIp }
                                discovered[key] = ComputerProfile(
                                    name = displayName,
                                    ipAddress = resolvedIp,
                                    port = resolvedPort,
                                    connectionMethodId = packet.method.id
                                )
                            }
                        }
                    } catch (_: SocketTimeoutException) {
                        if (!retrySent) {
                            retrySent = true
                            val leftMs = ((deadlineNanos - System.nanoTime()) / 1_000_000L).toInt()
                            if (leftMs > 40) {
                                sendDiscoveryBurst(createdSocket, requestBytes, broadcastTargets, port)
                            }
                        }
                    }
                }

                discovered.values.toList()
            } catch (e: CancellationException) {
                throw e
            } catch (_: Exception) {
                discovered.values.toList()
            } finally {
                cancelHandle?.dispose()
                socket?.let {
                    synchronized(socketLock) {
                        activeEphemeralSockets.remove(it)
                    }
                    runCatching { it.close() }
                }
            }
        }
    }

    fun close() {
        val socketsToClose: List<DatagramSocket>
        synchronized(socketLock) {
            closed = true
            socketsToClose = buildList {
                sendSocket?.let { add(it) }
                addAll(activeEphemeralSockets)
            }
            sendSocket = null
            activeEphemeralSockets.clear()
        }
        for (socket in socketsToClose) {
            runCatching { socket.close() }
        }
        addressCache.clear()
    }

    private fun invalidateSendSocket(brokenSocket: DatagramSocket?) {
        val toClose: DatagramSocket?
        synchronized(socketLock) {
            val current = sendSocket
            if (brokenSocket == null || current === brokenSocket) {
                toClose = current
                sendSocket = null
            } else {
                toClose = brokenSocket
            }
        }
        runCatching { toClose?.close() }
    }

    private fun getOrCreateSendSocket(): DatagramSocket? {
        if (closed) return null
        val current = sendSocket
        if (current != null && !current.isClosed) return current

        return synchronized(socketLock) {
            if (closed) return@synchronized null
            val existing = sendSocket
            if (existing != null && !existing.isClosed) {
                existing
            } else {
                DatagramSocket().also { sendSocket = it }
            }
        }
    }

    private fun resolveAddress(ip: String): InetAddress {
        val trimmed = ip.trim()
        return addressCache[trimmed] ?: InetAddress.getByName(trimmed).also {
            addressCache[trimmed] = it
        }
    }

    private fun collectBroadcastAddresses(): Set<InetAddress> {
        val addresses = LinkedHashSet<InetAddress>()
        runCatching {
            addresses.add(InetAddress.getByName("255.255.255.255"))
        }

        runCatching {
            val interfaces = NetworkInterface.getNetworkInterfaces() ?: return@runCatching
            while (interfaces.hasMoreElements()) {
                val networkInterface = interfaces.nextElement()
                if (!networkInterface.isUp || networkInterface.isLoopback) continue
                for (interfaceAddress in networkInterface.interfaceAddresses) {
                    interfaceAddress.broadcast?.let { addresses.add(it) }
                }
            }
        }
        return addresses
    }

    private fun sendDiscoveryBurst(
        socket: DatagramSocket,
        payload: ByteArray,
        targets: Set<InetAddress>,
        port: Int
    ) {
        for (target in targets) {
            runCatching {
                socket.send(DatagramPacket(payload, payload.size, target, port))
            }
        }
    }

    private fun decodePacketSafely(bytes: ByteArray, length: Int): TouchpadPacket? {
        return try {
            TouchpadPacket.fromByteArray(bytes, length)
        } catch (_: Exception) {
            null
        }
    }

    private companion object {
        const val RECEIVE_BUFFER_SIZE = 2048
    }
}