package com.touchpad.android.ui

import android.app.Activity
import android.content.Context
import android.content.ContextWrapper
import android.content.pm.ActivityInfo
import android.os.Build
import android.view.HapticFeedbackConstants
import android.view.WindowManager
import androidx.activity.compose.BackHandler
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.tween
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.displayCutoutPadding
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Lock
import androidx.compose.material.icons.filled.LockOpen
import androidx.compose.material.icons.filled.ScreenRotation
import androidx.compose.material3.Icon
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.input.pointer.PointerId
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalLifecycleOwner
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import com.touchpad.android.model.ActiveSession
import com.touchpad.android.model.AndroidSettings
import com.touchpad.android.model.SessionPeer
import com.touchpad.android.nativecore.RustTouchpadBridge
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlin.math.abs

private enum class MouseButton { LEFT, RIGHT }

private data class TapFlashEvent(
    val id: Long,
    val position: Offset,
    val isRightSide: Boolean
)

@Composable
fun TouchpadFullscreenScreen(
    session: ActiveSession,
    neighbors: List<SessionPeer>,
    settings: AndroidSettings,
    onSwitchActiveComputer: (String) -> Unit,
    onSetEdgeLock: (Boolean) -> Unit,
    onExit: () -> Unit,
    modifier: Modifier = Modifier
) {
    val context = LocalContext.current
    val view = LocalView.current
    val lifecycleOwner = LocalLifecycleOwner.current
    val activity = remember(context) { context.findActivity() }
    val density = LocalDensity.current

    var isLandscape by rememberSaveable { mutableStateOf(settings.defaultLandscape) }
    var heldMouseButton by remember { mutableStateOf<MouseButton?>(null) }
    var tapFlash by remember { mutableStateOf<TapFlashEvent?>(null) }
    val flashAlpha = remember { Animatable(0f) }
    val gestureScope = rememberCoroutineScope()
    var pendingHold by remember { mutableStateOf<Job?>(null) }
    var inputGeneration by remember { mutableStateOf(0) }
    val latestOnExit by rememberUpdatedState(onExit)
    val latestHaptics by rememberUpdatedState(settings.hapticsEnabled)
    val core = RustTouchpadBridge

    fun releaseHeldButton() {
        heldMouseButton?.let { button ->
            core.nativeSendButtonState(button.ordinal, false)
            heldMouseButton = null
        }
    }

    fun cancelInput() {
        inputGeneration++
        pendingHold?.cancel()
        releaseHeldButton()
    }

    val releaseHoldAndExit: () -> Unit = {
        cancelInput()
        latestOnExit()
    }

    BackHandler(onBack = releaseHoldAndExit)

    // Animate visual tap feedback on the left or right half of the touchpad.
    LaunchedEffect(tapFlash?.id) {
        if (tapFlash != null) {
            flashAlpha.snapTo(1f)
            flashAlpha.animateTo(0f, animationSpec = tween(durationMillis = 220))
            tapFlash = null
        }
    }

    // Apply orientation independently so toggling portrait/landscape does not flicker system bars.
    LaunchedEffect(activity, isLandscape) {
        activity?.requestedOrientation = if (isLandscape) {
            ActivityInfo.SCREEN_ORIENTATION_SENSOR_LANDSCAPE
        } else {
            ActivityInfo.SCREEN_ORIENTATION_PORTRAIT
        }
    }

    // Enter immersive fullscreen and keep screen awake while the touchpad is active.
    DisposableEffect(activity, view, lifecycleOwner) {
        val window = activity?.window
        val insetsController = window?.let { WindowCompat.getInsetsController(it, view) }
        val previousCutoutMode = if (window != null && Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
            window.attributes.layoutInDisplayCutoutMode
        } else {
            0
        }

        fun enterImmersive() {
            if (window == null || insetsController == null) return
            WindowCompat.setDecorFitsSystemWindows(window, false)
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
                window.attributes = window.attributes.apply {
                    layoutInDisplayCutoutMode =
                        WindowManager.LayoutParams.LAYOUT_IN_DISPLAY_CUTOUT_MODE_SHORT_EDGES
                }
            }
            window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
            insetsController.systemBarsBehavior =
                WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
            insetsController.hide(WindowInsetsCompat.Type.systemBars())
        }

        enterImmersive()

        val observer = LifecycleEventObserver { _, event ->
            when (event) {
                Lifecycle.Event.ON_RESUME -> enterImmersive()
                Lifecycle.Event.ON_PAUSE,
                Lifecycle.Event.ON_STOP -> cancelInput()
                else -> Unit
            }
        }
        lifecycleOwner.lifecycle.addObserver(observer)

        onDispose {
            cancelInput()
            lifecycleOwner.lifecycle.removeObserver(observer)
            if (window != null && insetsController != null) {
                window.clearFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
                    window.attributes = window.attributes.apply {
                        layoutInDisplayCutoutMode = previousCutoutMode
                    }
                }
                insetsController.show(WindowInsetsCompat.Type.systemBars())
                WindowCompat.setDecorFitsSystemWindows(window, true)
            }
            activity?.requestedOrientation = ActivityInfo.SCREEN_ORIENTATION_UNSPECIFIED
        }
    }

    val tapSlopPx = remember(density) { with(density) { 7.dp.toPx() }.coerceIn(14f, 28f) }

    Box(
        modifier = modifier
            .fillMaxSize()
            .background(Color.Black)
    ) {
        // Fullscreen pure black touchpad surface with center dividing line and zero-lag multi-touch.
        Box(
            modifier = Modifier
                .fillMaxSize()
                .pointerInput(session.primaryComputerId, settings.holdDurationMs, tapSlopPx) {
                    awaitEachGesture {
                        val firstDown = awaitFirstDown(requireUnconsumed = false)
                        if (!lifecycleOwner.lifecycle.currentState.isAtLeast(Lifecycle.State.RESUMED)) {
                            return@awaitEachGesture
                        }
                        val generation = inputGeneration
                        val downPos = firstDown.position
                        val onRight = downPos.x >= size.width / 2f
                        val button = if (onRight) MouseButton.RIGHT else MouseButton.LEFT
                        val holdDuration = settings.holdDurationMs.toLong()
                        var pointerId = firstDown.id
                        var lastPosition = downPos
                        var moved = false
                        var multitouch = false
                        var holdTriggered = false
                        var scrollIds: Pair<PointerId, PointerId>? = null
                        var previousScrollY = 0f

                        fun pressAndHold() {
                            holdTriggered = true
                            heldMouseButton = button
                            core.nativeSendButtonState(button.ordinal, true)
                            if (latestHaptics) view.performHapticFeedback(HapticFeedbackConstants.LONG_PRESS)
                        }

                        val holdJob = gestureScope.launch {
                            delay(holdDuration)
                            if (generation == inputGeneration) pressAndHold()
                        }
                        pendingHold = holdJob
                        try {
                            while (true) {
                                val event = awaitPointerEvent()
                                if (generation != inputGeneration) break

                                val pressed = event.changes.filter { it.pressed }
                                if (pressed.isEmpty()) {
                                    if (!multitouch && !moved && !holdTriggered) {
                                        val up = event.changes.firstOrNull { it.id == pointerId }
                                        if (up != null && (up.position - downPos).getDistance() <= tapSlopPx) {
                                            if (up.uptimeMillis - firstDown.uptimeMillis < holdDuration) {
                                                core.nativeSendClick(button.ordinal)
                                                if (latestHaptics) {
                                                    view.performHapticFeedback(HapticFeedbackConstants.KEYBOARD_TAP)
                                                }
                                                tapFlash = TapFlashEvent(up.uptimeMillis, downPos, onRight)
                                            } else {
                                                pressAndHold()
                                            }
                                        }
                                    }
                                    break
                                }

                                if (pressed.size >= 2) {
                                    multitouch = true
                                    holdJob.cancel()
                                    releaseHeldButton()
                                    val first = pressed.firstOrNull { it.id == scrollIds?.first } ?: pressed[0]
                                    val second = pressed.firstOrNull { it.id == scrollIds?.second && it.id != first.id }
                                        ?: pressed.first { it.id != first.id }
                                    val pair = first.id to second.id
                                    val y = (first.position.y + second.position.y) / 2f
                                    if (pair == scrollIds && abs(y - previousScrollY) > 0.5f) {
                                        core.nativeSendScroll(y - previousScrollY)
                                    }
                                    scrollIds = pair
                                    previousScrollY = y
                                } else {
                                    val pointer = pressed.single()
                                    if (scrollIds != null || pointer.id != pointerId) {
                                        holdJob.cancel()
                                        multitouch = true
                                        scrollIds = null
                                        pointerId = pointer.id
                                        lastPosition = pointer.position
                                    } else {
                                        val delta = pointer.position - lastPosition
                                        if (moved || multitouch || holdTriggered || delta.getDistance() > tapSlopPx) {
                                            moved = true
                                            holdJob.cancel()
                                            if (delta != Offset.Zero) core.nativeSendMove(delta.x, delta.y)
                                            lastPosition = pointer.position
                                        }
                                    }
                                }
                                event.changes.forEach { it.consume() }
                            }
                        } finally {
                            holdJob.cancel()
                            if (pendingHold === holdJob) pendingHold = null
                            releaseHeldButton()
                        }
                    }
                }
        ) {
            Canvas(modifier = Modifier.fillMaxSize()) {
                val centerX = size.width / 2f
                val activeFlash = tapFlash
                val alpha = flashAlpha.value

                heldMouseButton?.let { btn ->
                    val sideStartX = if (btn == MouseButton.RIGHT) centerX else 0f
                    drawRect(
                        color = Color(0xFF38BDF8).copy(alpha = 0.08f),
                        topLeft = Offset(sideStartX, 0f),
                        size = Size(centerX, size.height)
                    )
                }

                if (activeFlash != null && alpha > 0f) {
                    val sideStartX = if (activeFlash.isRightSide) centerX else 0f
                    drawRect(
                        color = Color.White.copy(alpha = 0.055f * alpha),
                        topLeft = Offset(sideStartX, 0f),
                        size = Size(centerX, size.height)
                    )
                    val radius = (18.dp.toPx() + 34.dp.toPx() * (1f - alpha))
                    drawCircle(
                        color = Color.White.copy(alpha = 0.35f * alpha),
                        radius = radius,
                        center = activeFlash.position,
                        style = Stroke(width = 2.dp.toPx())
                    )
                }

                // Clear vertical center line separating left-click and right-click sides.
                drawLine(
                    color = if (heldMouseButton != null) {
                        Color(0xFF38BDF8).copy(alpha = 0.7f)
                    } else {
                        Color.White.copy(alpha = 0.38f)
                    },
                    start = Offset(x = centerX, y = 0f),
                    end = Offset(x = centerX, y = size.height),
                    strokeWidth = 1.5.dp.toPx()
                )
            }

            // Ultra-subtle corner labels indicating left/right tap zones without cluttering black surface.
            Text(
                text = "ESQ",
                color = Color.White.copy(alpha = 0.16f),
                fontSize = 11.sp,
                fontWeight = FontWeight.Medium,
                modifier = Modifier
                    .align(Alignment.BottomStart)
                    .padding(start = 18.dp, bottom = 14.dp)
            )
            Text(
                text = "DIR",
                color = Color.White.copy(alpha = 0.16f),
                fontSize = 11.sp,
                fontWeight = FontWeight.Medium,
                modifier = Modifier
                    .align(Alignment.BottomEnd)
                    .padding(end = 18.dp, bottom = 14.dp)
            )
        }

        heldMouseButton?.let { btn ->
            val labelText = if (btn == MouseButton.RIGHT) {
                "Segurando Clique Direito"
            } else {
                "Segurando Clique Esquerdo"
            }
            Surface(
                modifier = Modifier
                    .align(Alignment.BottomCenter)
                    .displayCutoutPadding()
                    .padding(bottom = 28.dp),
                color = Color(0xFF0C4A6E).copy(alpha = 0.85f),
                shape = RoundedCornerShape(999.dp),
                border = BorderStroke(1.dp, Color(0xFF38BDF8).copy(alpha = 0.65f))
            ) {
                Row(
                    modifier = Modifier.padding(horizontal = 12.dp, vertical = 8.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(8.dp)
                ) {
                    Box(
                        modifier = Modifier
                            .size(8.dp)
                            .clip(CircleShape)
                            .background(Color(0xFF38BDF8))
                    )
                    Text(
                        text = labelText,
                        color = Color(0xFFE0F2FE),
                        fontSize = 12.sp,
                        fontWeight = FontWeight.Medium
                    )
                }
            }
        }

        Column(
            modifier = Modifier.align(Alignment.TopEnd).displayCutoutPadding().padding(top = 12.dp, end = 12.dp),
            horizontalAlignment = Alignment.End,
            verticalArrangement = Arrangement.spacedBy(6.dp)
        ) {
            Surface(
                color = Color(0xFF0F172A).copy(alpha = 0.55f),
                shape = RoundedCornerShape(999.dp),
                border = BorderStroke(1.dp, Color.White.copy(alpha = 0.14f))
            ) {
                Row(
                    modifier = Modifier.padding(start = 12.dp, end = 4.dp, top = 2.dp, bottom = 2.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(4.dp)
                ) {
                    Box(
                        modifier = Modifier
                            .size(7.dp)
                            .clip(CircleShape)
                            .background(
                                if (session.connected) Color(0xFF22C55E) else Color(0xFF64748B)
                            )
                    )
                    Spacer(modifier = Modifier.width(2.dp))
                    Text(
                        text = session.activeComputerName,
                        color = Color.White.copy(alpha = 0.78f),
                        fontSize = 12.sp,
                        fontWeight = FontWeight.Medium,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.widthIn(max = if (session.multiPcActive) 96.dp else 136.dp)
                    )

                    if (session.multiPcActive) {
                        Box(
                            modifier = Modifier.size(48.dp).clip(CircleShape)
                                .toggleable(
                                    value = session.edgeLocked,
                                    role = Role.Switch,
                                    onValueChange = onSetEdgeLock
                                )
                                .semantics {
                                    contentDescription = if (session.edgeLocked) "Destravar PC" else "Travar PC"
                                },
                            contentAlignment = Alignment.Center
                        ) {
                            Icon(
                                if (session.edgeLocked) Icons.Default.Lock else Icons.Default.LockOpen,
                                contentDescription = null,
                                tint = if (session.edgeLocked) Color(0xFF38BDF8) else Color.White.copy(alpha = 0.7f),
                                modifier = Modifier.size(16.dp)
                            )
                        }
                    }

                    Box(
                        modifier = Modifier
                            .size(48.dp)
                            .clip(CircleShape)
                            .clickable(
                                role = Role.Button,
                                onClick = { isLandscape = !isLandscape }
                            )
                            .semantics {
                                contentDescription = "Alternar orientação da tela"
                            },
                        contentAlignment = Alignment.Center
                    ) {
                        Icon(
                            imageVector = Icons.Default.ScreenRotation,
                            contentDescription = null,
                            tint = Color.White.copy(alpha = 0.7f),
                            modifier = Modifier.size(16.dp)
                        )
                    }

                    Row(
                        modifier = Modifier
                            .clip(RoundedCornerShape(999.dp))
                            .clickable(
                                role = Role.Button,
                                onClick = releaseHoldAndExit
                            )
                            .padding(horizontal = 10.dp, vertical = 10.dp)
                            .semantics {
                                contentDescription = "Sair do modo touchpad"
                            },
                        verticalAlignment = Alignment.CenterVertically
                    ) {
                        Icon(
                            imageVector = Icons.Default.Close,
                            contentDescription = null,
                            tint = Color.White.copy(alpha = 0.85f),
                            modifier = Modifier.size(16.dp)
                        )
                        Spacer(modifier = Modifier.width(4.dp))
                        Text(
                            text = "Sair",
                            color = Color.White.copy(alpha = 0.85f),
                            fontSize = 12.sp,
                            fontWeight = FontWeight.Medium
                        )
                    }
                }
            }
            if (session.multiPcActive && neighbors.isNotEmpty()) {
                Row(
                    modifier = Modifier.widthIn(max = 300.dp).horizontalScroll(rememberScrollState()),
                    horizontalArrangement = Arrangement.spacedBy(6.dp)
                ) {
                    neighbors.forEach { neighbor ->
                        val available = neighbor.online && neighbor.pairingStatus == "APPROVED"
                        Surface(
                            onClick = {
                                cancelInput()
                                onSwitchActiveComputer(neighbor.id)
                            },
                            enabled = available,
                            shape = CircleShape,
                            color = Color(0xFF0F172A),
                            contentColor = Color.White.copy(alpha = if (available) 0.86f else 0.45f)
                        ) {
                            Text(
                                text = neighbor.name,
                                modifier = Modifier.padding(horizontal = 10.dp, vertical = 10.dp).widthIn(max = 112.dp),
                                fontSize = 11.sp,
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis
                            )
                        }
                    }
                }
            }
        }
    }
}

private tailrec fun Context.findActivity(): Activity? = when (this) {
    is Activity -> this
    is ContextWrapper -> baseContext.findActivity()
    else -> null
}
