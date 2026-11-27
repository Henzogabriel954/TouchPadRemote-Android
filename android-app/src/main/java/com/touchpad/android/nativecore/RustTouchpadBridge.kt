package com.touchpad.android.nativecore

import android.util.Log

object RustTouchpadBridge {
    val isLoaded: Boolean

    init {
        isLoaded = try {
            System.loadLibrary("touchpad_android_core")
            true
        } catch (error: LinkageError) {
            Log.e("RustTouchpadBridge", "Unable to load the Rust core", error)
            false
        } catch (error: SecurityException) {
            Log.e("RustTouchpadBridge", "Unable to load the Rust core", error)
            false
        }
    }

    @JvmStatic external fun nativeInit(filesDir: String, deviceName: String): Boolean
    @JvmStatic external fun nativeShutdown()
    @JvmStatic external fun nativeGetStateJson(): String?
    @JvmStatic external fun nativeWaitEventsJson(timeoutMs: Int): String?
    @JvmStatic external fun nativeAddOrUpdateComputerJson(json: String): String?
    @JvmStatic external fun nativeRemoveComputer(computerId: String): String?
    @JvmStatic external fun nativeSaveEdgeLinksJson(json: String): String?
    @JvmStatic external fun nativeSaveSettingsJson(json: String): String?
    @JvmStatic external fun nativeRefreshStatus()
    @JvmStatic external fun nativeDiscoverComputers(timeoutMs: Int)
    @JvmStatic external fun nativeClearDiscovery()
    @JvmStatic external fun nativeStartSession(computerId: String, multiPcEnabled: Boolean)
    @JvmStatic external fun nativeCancelPairing()
    @JvmStatic external fun nativeExitSession()
    @JvmStatic external fun nativeSwitchActiveComputer(targetComputerId: String): Boolean
    @JvmStatic external fun nativeSetEdgeLock(locked: Boolean)
    @JvmStatic external fun nativeSendMove(dx: Float, dy: Float)
    @JvmStatic external fun nativeSendClick(buttonOrdinal: Int)
    @JvmStatic external fun nativeSendButtonState(buttonOrdinal: Int, pressed: Boolean)
    @JvmStatic external fun nativeSendScroll(deltaY: Float)
}
