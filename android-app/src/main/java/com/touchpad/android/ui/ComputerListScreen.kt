package com.touchpad.android.ui

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Bluetooth
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Computer
import androidx.compose.material.icons.filled.DeleteOutline
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material.icons.filled.Search
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material.icons.filled.Usb
import androidx.compose.material.icons.filled.Wifi
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.AssistChip
import androidx.compose.material3.AssistChipDefaults
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExtendedFloatingActionButton
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.OutlinedTextFieldDefaults
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalLifecycleOwner
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.window.Dialog
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.repeatOnLifecycle
import com.touchpad.android.model.ComputerProfile
import com.touchpad.android.model.AndroidSettings
import com.touchpad.android.model.ConnectionMethod
import com.touchpad.android.model.DEFAULT_PORT
import com.touchpad.android.model.EdgeLink
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive

private val HomeBackground = Color(0xFF0B1120)
private val CardSurface = Color(0xFF131C31)
private val CardBorder = Color(0xFF263450)
private val AccentBlue = Color(0xFF38BDF8)
private val AccentContainer = Color(0xFF0C4A6E)
private val TextPrimary = Color(0xFFF8FAFC)
private val TextSecondary = Color(0xFF94A3B8)
private val OnlineGreen = Color(0xFF22C55E)
private val OfflineSlate = Color(0xFF64748B)
private val DangerRed = Color(0xFFF87171)

@Composable
fun ComputerListScreen(
    computers: List<ComputerProfile>,
    onlineStatus: Map<String, Boolean>,
    isScanningNetwork: Boolean,
    discoveredComputers: List<ComputerProfile>,
    hasScannedNetwork: Boolean,
    checkingComputerId: String?,
    pairingComputer: ComputerProfile?,
    offlineAlertComputer: ComputerProfile?,
    infoErrorMessage: String?,
    settings: AndroidSettings,
    edgeLinks: List<EdgeLink>,
    coreAvailable: Boolean,
    onSaveSettings: (AndroidSettings) -> Unit,
    onSaveLayout: (List<EdgeLink>, Boolean, Int, String?) -> Unit,
    onSelectComputer: (ComputerProfile) -> Unit,
    onAddComputer: (ComputerProfile) -> Unit,
    onRemoveComputer: (String) -> Unit,
    onRefreshStatus: () -> Unit,
    onDiscoverNetwork: () -> Unit,
    onClearDiscovery: () -> Unit,
    onCancelPairing: () -> Unit,
    onDismissOfflineAlert: () -> Unit,
    onDismissInfoError: () -> Unit,
    modifier: Modifier = Modifier
) {
    var showAddDialog by rememberSaveable { mutableStateOf(false) }
    var autoScanOnOpen by rememberSaveable { mutableStateOf(false) }
    var showMethodsDialog by rememberSaveable { mutableStateOf(false) }
    var showSettings by rememberSaveable { mutableStateOf(false) }
    var showMultiPc by rememberSaveable { mutableStateOf(false) }
    var pendingDelete by remember { mutableStateOf<ComputerProfile?>(null) }

    val lifecycleOwner = LocalLifecycleOwner.current
    val latestOnRefreshStatus by rememberUpdatedState(onRefreshStatus)

    LaunchedEffect(lifecycleOwner, computers.isNotEmpty()) {
        if (computers.isNotEmpty()) {
            lifecycleOwner.lifecycle.repeatOnLifecycle(Lifecycle.State.STARTED) {
                while (isActive) {
                    latestOnRefreshStatus()
                    delay(2500L)
                }
            }
        }
    }

    Scaffold(
        modifier = modifier
            .fillMaxSize()
            .background(HomeBackground),
        containerColor = HomeBackground,
        floatingActionButton = {
            if (computers.isNotEmpty()) {
                ExtendedFloatingActionButton(
                    onClick = {
                        autoScanOnOpen = false
                        onClearDiscovery()
                        showAddDialog = true
                    },
                    modifier = Modifier.navigationBarsPadding(),
                    containerColor = AccentBlue,
                    contentColor = Color(0xFF041E30),
                    icon = {
                        Icon(
                            imageVector = Icons.Default.Add,
                            contentDescription = null
                        )
                    },
                    text = {
                        Text(
                            text = "Cadastrar Computador",
                            fontWeight = FontWeight.SemiBold
                        )
                    }
                )
            }
        }
    ) { innerPadding ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .statusBarsPadding()
                .padding(innerPadding)
        ) {
            HomeTopBar(
                hasComputers = computers.isNotEmpty(),
                onConnectionMethodClick = { showMethodsDialog = true },
                onRefreshClick = onRefreshStatus
            )

            if (coreAvailable) {
                Row(
                    modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp),
                    horizontalArrangement = Arrangement.SpaceBetween,
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    OutlinedButton(onClick = { showMultiPc = true }) { Text("Multi-PC (Bordas)") }
                    IconButton(onClick = { showSettings = true }) {
                        Icon(Icons.Default.Settings, contentDescription = "Configurações")
                    }
                }
            }

            if (!coreAvailable) {
                Text(
                    "Núcleo do TouchPad Remote indisponível.",
                    modifier = Modifier.padding(16.dp),
                    color = MaterialTheme.colorScheme.error
                )
            } else if (computers.isEmpty()) {
                EmptyComputersView(
                    onOpenAddManual = {
                        autoScanOnOpen = false
                        onClearDiscovery()
                        showAddDialog = true
                    },
                    onOpenScanNetwork = {
                        autoScanOnOpen = true
                        onClearDiscovery()
                        showAddDialog = true
                    },
                    modifier = Modifier.weight(1f)
                )
            } else {
                LazyVerticalGrid(
                    columns = GridCells.Adaptive(minSize = 156.dp),
                    modifier = Modifier
                        .fillMaxWidth()
                        .weight(1f),
                    contentPadding = PaddingValues(
                        start = 16.dp,
                        end = 16.dp,
                        top = 8.dp,
                        bottom = 96.dp
                    ),
                    horizontalArrangement = Arrangement.spacedBy(12.dp),
                    verticalArrangement = Arrangement.spacedBy(12.dp)
                ) {
                    items(
                        items = computers,
                        key = { it.id }
                    ) { computer ->
                        ComputerSquareCard(
                            computer = computer,
                            isOnline = onlineStatus[computer.id] == true,
                            isChecking = checkingComputerId == computer.id,
                            onClick = { onSelectComputer(computer) },
                            onDeleteClick = { pendingDelete = computer }
                        )
                    }
                }
            }
        }
    }

    if (showSettings && coreAvailable) {
        SettingsDialog(settings, onDismiss = { showSettings = false }) {
            onSaveSettings(it)
            showSettings = false
        }
    }
    if (showMultiPc && coreAvailable) {
        MultiPcLayoutDialog(computers, edgeLinks, settings, onDismiss = { showMultiPc = false }) {
            links, enabled, overshoot, primaryId ->
            onSaveLayout(links, enabled, overshoot, primaryId)
            showMultiPc = false
        }
    }
    if (showAddDialog && coreAvailable) {
        AddComputerDialog(
            autoScanOnOpen = autoScanOnOpen,
            isScanning = isScanningNetwork,
            discoveredComputers = discoveredComputers,
            hasScannedOnce = hasScannedNetwork,
            onDiscoverClick = onDiscoverNetwork,
            onDismiss = {
                showAddDialog = false
                autoScanOnOpen = false
                onClearDiscovery()
            },
            onSave = { profile ->
                onAddComputer(profile)
                showAddDialog = false
                autoScanOnOpen = false
                onClearDiscovery()
            }
        )
    }

    if (showMethodsDialog) {
        ConnectionMethodsDialog(
            onDismiss = { showMethodsDialog = false }
        )
    }

    pendingDelete?.let { target ->
        AlertDialog(
            onDismissRequest = { pendingDelete = null },
            containerColor = CardSurface,
            titleContentColor = TextPrimary,
            textContentColor = TextSecondary,
            title = {
                Text(
                    text = "Remover computador?",
                    fontWeight = FontWeight.SemiBold
                )
            },
            text = {
                Text("Deseja remover \"${target.name}\" (${target.ipAddress}:${target.port}) da lista?")
            },
            confirmButton = {
                TextButton(
                    onClick = {
                        onRemoveComputer(target.id)
                        pendingDelete = null
                    },
                    colors = ButtonDefaults.textButtonColors(contentColor = DangerRed)
                ) {
                    Text("Remover", fontWeight = FontWeight.SemiBold)
                }
            },
            dismissButton = {
                TextButton(
                    onClick = { pendingDelete = null },
                    colors = ButtonDefaults.textButtonColors(contentColor = TextSecondary)
                ) {
                    Text("Cancelar")
                }
            }
        )
    }

    offlineAlertComputer?.let { pc ->
        AlertDialog(
            onDismissRequest = onDismissOfflineAlert,
            containerColor = CardSurface,
            titleContentColor = TextPrimary,
            textContentColor = TextSecondary,
            title = {
                Text(
                    text = "Computador Offline",
                    fontWeight = FontWeight.Bold
                )
            },
            text = {
                Text(
                    text = "O computador \"${pc.name}\" (${pc.ipAddress}:${pc.port}) está offline ou não respondeu.\n\nAbra o aplicativo TouchPad Remote no computador e confira se ambos estão no mesmo Wi-Fi.",
                    lineHeight = 20.sp
                )
            },
            confirmButton = {
                Button(
                    onClick = {
                        onDismissOfflineAlert()
                        onSelectComputer(pc)
                    },
                    colors = ButtonDefaults.buttonColors(
                        containerColor = AccentBlue,
                        contentColor = Color(0xFF041E30)
                    )
                ) {
                    Text("Tentar novamente", fontWeight = FontWeight.SemiBold)
                }
            },
            dismissButton = {
                TextButton(
                    onClick = onDismissOfflineAlert,
                    colors = ButtonDefaults.textButtonColors(contentColor = TextSecondary)
                ) {
                    Text("Fechar")
                }
            }
        )
    }

    pairingComputer?.let { pc ->
        AlertDialog(
            onDismissRequest = onCancelPairing,
            containerColor = CardSurface,
            titleContentColor = TextPrimary,
            textContentColor = TextSecondary,
            title = {
                Text(
                    text = "Aceite o pareamento no computador",
                    fontWeight = FontWeight.Bold
                )
            },
            text = {
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(16.dp)
                ) {
                    CircularProgressIndicator(
                        modifier = Modifier.size(28.dp),
                        strokeWidth = 3.dp,
                        color = AccentBlue
                    )
                    Text(
                        text = "Olhe para a janela do TouchPad Remote no computador \"${pc.name}\" e clique no botão verde \"Aceitar Pareamento\".",
                        lineHeight = 20.sp,
                        modifier = Modifier.weight(1f)
                    )
                }
            },
            confirmButton = {
                TextButton(
                    onClick = onCancelPairing,
                    colors = ButtonDefaults.textButtonColors(contentColor = TextSecondary)
                ) {
                    Text("Cancelar", fontWeight = FontWeight.SemiBold)
                }
            }
        )
    }

    infoErrorMessage?.let { message ->
        AlertDialog(
            onDismissRequest = onDismissInfoError,
            containerColor = CardSurface,
            titleContentColor = TextPrimary,
            textContentColor = TextSecondary,
            title = {
                Text(
                    text = "Aviso de Pareamento",
                    fontWeight = FontWeight.Bold
                )
            },
            text = {
                Text(
                    text = message,
                    lineHeight = 20.sp
                )
            },
            confirmButton = {
                TextButton(onClick = onDismissInfoError) {
                    Text("OK", color = AccentBlue, fontWeight = FontWeight.SemiBold)
                }
            }
        )
    }
}

@Composable
private fun HomeTopBar(
    hasComputers: Boolean,
    onConnectionMethodClick: () -> Unit,
    onRefreshClick: () -> Unit
) {
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .padding(horizontal = 16.dp, vertical = 14.dp)
    ) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.SpaceBetween
        ) {
            Column(modifier = Modifier.weight(1f)) {
                Text(
                    text = "TouchPad Remote",
                    style = MaterialTheme.typography.headlineSmall,
                    color = TextPrimary,
                    fontWeight = FontWeight.Bold
                )
                Spacer(modifier = Modifier.height(2.dp))
                Text(
                    text = "Toque no quadrado do computador para conectar",
                    style = MaterialTheme.typography.bodySmall,
                    color = TextSecondary
                )
            }

            if (hasComputers) {
                IconButton(
                    onClick = onRefreshClick,
                    modifier = Modifier.size(48.dp)
                ) {
                    Icon(
                        imageVector = Icons.Default.Refresh,
                        contentDescription = "Atualizar status dos computadores",
                        tint = TextSecondary
                    )
                }
            }
        }

        Spacer(modifier = Modifier.height(10.dp))

        AssistChip(
            onClick = onConnectionMethodClick,
            label = {
                Text(
                    text = "Via Rede (Wi-Fi / LAN)",
                    fontSize = 13.sp,
                    fontWeight = FontWeight.Medium
                )
            },
            leadingIcon = {
                Icon(
                    imageVector = Icons.Default.Wifi,
                    contentDescription = null,
                    modifier = Modifier.size(16.dp),
                    tint = AccentBlue
                )
            },
            colors = AssistChipDefaults.assistChipColors(
                containerColor = CardSurface,
                labelColor = TextPrimary
            ),
            border = BorderStroke(1.dp, CardBorder),
            shape = RoundedCornerShape(999.dp)
        )
    }
}

@Composable
private fun ComputerSquareCard(
    computer: ComputerProfile,
    isOnline: Boolean,
    isChecking: Boolean,
    onClick: () -> Unit,
    onDeleteClick: () -> Unit
) {
    val methodLabel = remember(computer.connectionMethodId) {
        val resolved: ConnectionMethod? = ConnectionMethod.fromId(computer.connectionMethodId)
        when (resolved) {
            ConnectionMethod.BLUETOOTH -> "Bluetooth"
            ConnectionMethod.USB_ADB -> "Cabo USB"
            ConnectionMethod.NETWORK_LAN, null -> "Via Rede"
        }
    }

    Surface(
        modifier = Modifier
            .fillMaxWidth()
            .aspectRatio(1f)
            .clip(RoundedCornerShape(20.dp))
            .clickable(
                role = Role.Button,
                onClickLabel = "Abrir touchpad para ${computer.name}",
                onClick = onClick
            )
            .semantics {
                val statusText = if (isOnline) "Online" else "Offline"
                contentDescription = "${computer.name}, $statusText, ${computer.ipAddress} porta ${computer.port}"
            },
        shape = RoundedCornerShape(20.dp),
        color = CardSurface,
        border = BorderStroke(1.dp, CardBorder)
    ) {
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(12.dp),
            verticalArrangement = Arrangement.SpaceBetween
        ) {
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.Top
            ) {
                Box(
                    modifier = Modifier.size(44.dp),
                    contentAlignment = Alignment.Center
                ) {
                    Box(
                        modifier = Modifier
                            .size(42.dp)
                            .clip(RoundedCornerShape(12.dp))
                            .background(AccentContainer.copy(alpha = 0.7f)),
                        contentAlignment = Alignment.Center
                    ) {
                        Icon(
                            imageVector = Icons.Default.Computer,
                            contentDescription = null,
                            tint = AccentBlue,
                            modifier = Modifier.size(22.dp)
                        )
                    }
                    Box(
                        modifier = Modifier
                            .align(Alignment.TopEnd)
                            .size(10.dp)
                            .clip(CircleShape)
                            .background(if (isOnline) OnlineGreen else OfflineSlate)
                    )
                }

                IconButton(
                    onClick = onDeleteClick,
                    modifier = Modifier.size(48.dp)
                ) {
                    Icon(
                        imageVector = Icons.Default.DeleteOutline,
                        contentDescription = "Remover ${computer.name}",
                        tint = TextSecondary,
                        modifier = Modifier.size(20.dp)
                    )
                }
            }

            Column(modifier = Modifier.fillMaxWidth()) {
                Text(
                    text = computer.name,
                    color = TextPrimary,
                    fontSize = 16.sp,
                    fontWeight = FontWeight.Bold,
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis,
                    lineHeight = 20.sp
                )
                Spacer(modifier = Modifier.height(4.dp))
                Text(
                    text = "${computer.ipAddress}:${computer.port}",
                    color = TextSecondary,
                    fontSize = 12.sp,
                    fontFamily = FontFamily.Monospace,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis
                )
                val os = when (computer.lastOs?.uppercase()) {
                    "WINDOWS" -> "Windows"
                    "LINUX" -> "Linux"
                    else -> null
                }
                val privilege = when (computer.lastPrivilege?.uppercase()) {
                    "USER" -> if (os == "Windows") "UAC" else null
                    "ADMIN" -> "Admin"
                    "SYSTEM" -> "Sistema"
                    "UINPUT" -> "uinput"
                    "XTEST" -> "XTest"
                    else -> null
                }
                val hostLabel = listOfNotNull(os, privilege).joinToString(" · ")
                if (hostLabel.isNotEmpty()) {
                    Text(
                        text = hostLabel,
                        color = TextSecondary,
                        fontSize = 10.sp,
                        lineHeight = 12.sp,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis
                    )
                }
                Spacer(modifier = Modifier.height(4.dp))
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(6.dp)
                ) {
                    Surface(
                        color = Color(0xFF1E293B),
                        shape = RoundedCornerShape(8.dp)
                    ) {
                        Text(
                            text = methodLabel,
                            color = AccentBlue,
                            fontSize = 11.sp,
                            fontWeight = FontWeight.Medium,
                            maxLines = 1,
                            modifier = Modifier.padding(horizontal = 8.dp, vertical = 3.dp)
                        )
                    }
                    if (isChecking) {
                        CircularProgressIndicator(
                            modifier = Modifier.size(14.dp),
                            strokeWidth = 2.dp,
                            color = AccentBlue
                        )
                    } else {
                        Surface(
                            color = if (isOnline) {
                                OnlineGreen.copy(alpha = 0.14f)
                            } else {
                                OfflineSlate.copy(alpha = 0.16f)
                            },
                            shape = RoundedCornerShape(8.dp)
                        ) {
                            Text(
                                text = if (isOnline) "Online" else "Offline",
                                color = if (isOnline) OnlineGreen else TextSecondary,
                                fontSize = 11.sp,
                                fontWeight = FontWeight.SemiBold,
                                maxLines = 1,
                                modifier = Modifier.padding(horizontal = 7.dp, vertical = 3.dp)
                            )
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun EmptyComputersView(
    onOpenAddManual: () -> Unit,
    onOpenScanNetwork: () -> Unit,
    modifier: Modifier = Modifier
) {
    Column(
        modifier = modifier
            .fillMaxWidth()
            .padding(horizontal = 24.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center
    ) {
        Box(
            modifier = Modifier
                .size(84.dp)
                .clip(RoundedCornerShape(24.dp))
                .background(CardSurface),
            contentAlignment = Alignment.Center
        ) {
            Icon(
                imageVector = Icons.Default.Computer,
                contentDescription = null,
                tint = AccentBlue,
                modifier = Modifier.size(42.dp)
            )
        }

        Spacer(modifier = Modifier.height(20.dp))

        Text(
            text = "Nenhum computador cadastrado",
            color = TextPrimary,
            fontSize = 20.sp,
            fontWeight = FontWeight.Bold,
            textAlign = TextAlign.Center
        )

        Spacer(modifier = Modifier.height(8.dp))

        Text(
            text = "Cadastre seu PC pelo endereço IP ou busque automaticamente na rede Wi-Fi para usar o touchpad em tela cheia.",
            color = TextSecondary,
            fontSize = 14.sp,
            textAlign = TextAlign.Center,
            lineHeight = 20.sp
        )

        Spacer(modifier = Modifier.height(24.dp))

        Button(
            onClick = onOpenAddManual,
            modifier = Modifier
                .fillMaxWidth()
                .heightIn(min = 50.dp),
            colors = ButtonDefaults.buttonColors(
                containerColor = AccentBlue,
                contentColor = Color(0xFF041E30)
            ),
            shape = RoundedCornerShape(14.dp)
        ) {
            Icon(
                imageVector = Icons.Default.Add,
                contentDescription = null,
                modifier = Modifier.size(18.dp)
            )
            Spacer(modifier = Modifier.width(8.dp))
            Text(
                text = "Cadastrar Computador",
                fontWeight = FontWeight.SemiBold,
                fontSize = 15.sp
            )
        }

        Spacer(modifier = Modifier.height(10.dp))

        OutlinedButton(
            onClick = onOpenScanNetwork,
            modifier = Modifier
                .fillMaxWidth()
                .heightIn(min = 50.dp),
            border = BorderStroke(1.dp, CardBorder),
            colors = ButtonDefaults.outlinedButtonColors(contentColor = TextPrimary),
            shape = RoundedCornerShape(14.dp)
        ) {
            Icon(
                imageVector = Icons.Default.Search,
                contentDescription = null,
                tint = AccentBlue,
                modifier = Modifier.size(18.dp)
            )
            Spacer(modifier = Modifier.width(8.dp))
            Text(
                text = "Buscar na Rede",
                fontWeight = FontWeight.Medium,
                fontSize = 15.sp
            )
        }
    }
}

@Composable
private fun AddComputerDialog(
    autoScanOnOpen: Boolean,
    isScanning: Boolean,
    discoveredComputers: List<ComputerProfile>,
    hasScannedOnce: Boolean,
    onDiscoverClick: () -> Unit,
    onDismiss: () -> Unit,
    onSave: (ComputerProfile) -> Unit
) {
    var name by rememberSaveable { mutableStateOf("") }
    var ipAddress by rememberSaveable { mutableStateOf("") }
    var portText by rememberSaveable { mutableStateOf(DEFAULT_PORT.toString()) }

    var nameError by remember { mutableStateOf<String?>(null) }
    var ipError by remember { mutableStateOf<String?>(null) }
    var portError by remember { mutableStateOf<String?>(null) }

    LaunchedEffect(autoScanOnOpen) {
        if (autoScanOnOpen && !hasScannedOnce && !isScanning) {
            onDiscoverClick()
        }
    }

    Dialog(onDismissRequest = onDismiss) {
        Surface(
            modifier = Modifier.fillMaxWidth(),
            shape = RoundedCornerShape(24.dp),
            color = CardSurface,
            border = BorderStroke(1.dp, CardBorder)
        ) {
            Column(
                modifier = Modifier
                    .fillMaxWidth()
                    .verticalScroll(rememberScrollState())
                    .padding(20.dp)
            ) {
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.SpaceBetween,
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    Text(
                        text = "Cadastrar Computador",
                        color = TextPrimary,
                        fontSize = 19.sp,
                        fontWeight = FontWeight.Bold
                    )
                    IconButton(
                        onClick = onDismiss,
                        modifier = Modifier.size(48.dp)
                    ) {
                        Icon(
                            imageVector = Icons.Default.Close,
                            contentDescription = "Fechar",
                            tint = TextSecondary
                        )
                    }
                }

                Spacer(modifier = Modifier.height(6.dp))

                FilledTonalButton(
                    onClick = onDiscoverClick,
                    enabled = !isScanning,
                    modifier = Modifier
                        .fillMaxWidth()
                        .heightIn(min = 48.dp),
                    colors = ButtonDefaults.filledTonalButtonColors(
                        containerColor = AccentContainer,
                        contentColor = TextPrimary
                    ),
                    shape = RoundedCornerShape(12.dp)
                ) {
                    if (isScanning) {
                        CircularProgressIndicator(
                            modifier = Modifier.size(18.dp),
                            strokeWidth = 2.dp,
                            color = AccentBlue
                        )
                        Spacer(modifier = Modifier.width(10.dp))
                        Text("Buscando na rede...", fontWeight = FontWeight.Medium)
                    } else {
                        Icon(
                            imageVector = Icons.Default.Search,
                            contentDescription = null,
                            tint = AccentBlue,
                            modifier = Modifier.size(18.dp)
                        )
                        Spacer(modifier = Modifier.width(8.dp))
                        Text("Buscar na Rede", fontWeight = FontWeight.SemiBold)
                    }
                }

                if (discoveredComputers.isNotEmpty()) {
                    Spacer(modifier = Modifier.height(10.dp))
                    Text(
                        text = "Computadores encontrados (toque para preencher):",
                        color = AccentBlue,
                        fontSize = 12.sp,
                        fontWeight = FontWeight.Medium
                    )
                    Spacer(modifier = Modifier.height(6.dp))
                    Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
                        discoveredComputers.forEach { found ->
                            Surface(
                                modifier = Modifier
                                    .fillMaxWidth()
                                    .clip(RoundedCornerShape(12.dp))
                                    .clickable {
                                        name = found.name
                                        ipAddress = found.ipAddress
                                        portText = found.port.toString()
                                        nameError = null
                                        ipError = null
                                        portError = null
                                    },
                                color = Color(0xFF1E293B),
                                shape = RoundedCornerShape(12.dp),
                                border = BorderStroke(1.dp, CardBorder)
                            ) {
                                Row(
                                    modifier = Modifier
                                        .fillMaxWidth()
                                        .padding(horizontal = 12.dp, vertical = 10.dp),
                                    verticalAlignment = Alignment.CenterVertically,
                                    horizontalArrangement = Arrangement.SpaceBetween
                                ) {
                                    Column(modifier = Modifier.weight(1f)) {
                                        Text(
                                            text = found.name,
                                            color = TextPrimary,
                                            fontSize = 14.sp,
                                            fontWeight = FontWeight.SemiBold,
                                            maxLines = 1,
                                            overflow = TextOverflow.Ellipsis
                                        )
                                        Text(
                                            text = "${found.ipAddress}:${found.port}",
                                            color = TextSecondary,
                                            fontSize = 12.sp,
                                            fontFamily = FontFamily.Monospace
                                        )
                                    }
                                    Text(
                                        text = "Selecionar",
                                        color = AccentBlue,
                                        fontSize = 12.sp,
                                        fontWeight = FontWeight.SemiBold
                                    )
                                }
                            }
                        }
                    }
                } else if (hasScannedOnce && !isScanning) {
                    Spacer(modifier = Modifier.height(8.dp))
                    Text(
                        text = "Nenhum PC encontrado automaticamente. Verifique se o TouchPad Remote está aberto no computador na mesma rede Wi-Fi ou digite o IP manualmente abaixo.",
                        color = TextSecondary,
                        fontSize = 12.sp,
                        lineHeight = 16.sp
                    )
                }

                Spacer(modifier = Modifier.height(14.dp))
                HorizontalDivider(color = CardBorder)
                Spacer(modifier = Modifier.height(14.dp))

                val fieldColors = OutlinedTextFieldDefaults.colors(
                    focusedBorderColor = AccentBlue,
                    unfocusedBorderColor = CardBorder,
                    focusedLabelColor = AccentBlue,
                    unfocusedLabelColor = TextSecondary,
                    focusedTextColor = TextPrimary,
                    unfocusedTextColor = TextPrimary,
                    cursorColor = AccentBlue,
                    errorBorderColor = DangerRed,
                    errorLabelColor = DangerRed
                )

                OutlinedTextField(
                    value = name,
                    onValueChange = {
                        name = it
                        nameError = null
                    },
                    label = { Text("Nome do Computador") },
                    placeholder = { Text("Ex: Notebook Sala", color = OfflineSlate) },
                    singleLine = true,
                    isError = nameError != null,
                    supportingText = nameError?.let { msg -> { Text(msg, color = DangerRed) } },
                    keyboardOptions = KeyboardOptions(imeAction = ImeAction.Next),
                    colors = fieldColors,
                    modifier = Modifier.fillMaxWidth()
                )

                Spacer(modifier = Modifier.height(8.dp))

                OutlinedTextField(
                    value = ipAddress,
                    onValueChange = {
                        ipAddress = it.trim()
                        ipError = null
                    },
                    label = { Text("Endereço IP") },
                    placeholder = { Text("Ex: 192.168.1.100", color = OfflineSlate) },
                    singleLine = true,
                    isError = ipError != null,
                    supportingText = ipError?.let { msg -> { Text(msg, color = DangerRed) } },
                    keyboardOptions = KeyboardOptions(
                        keyboardType = KeyboardType.Uri,
                        imeAction = ImeAction.Next
                    ),
                    colors = fieldColors,
                    modifier = Modifier.fillMaxWidth()
                )

                Spacer(modifier = Modifier.height(8.dp))

                OutlinedTextField(
                    value = portText,
                    onValueChange = { input ->
                        portText = input.filter { it.isDigit() }
                        portError = null
                    },
                    label = { Text("Porta") },
                    placeholder = { Text("45454", color = OfflineSlate) },
                    singleLine = true,
                    isError = portError != null,
                    supportingText = portError?.let { msg -> { Text(msg, color = DangerRed) } },
                    keyboardOptions = KeyboardOptions(
                        keyboardType = KeyboardType.Number,
                        imeAction = ImeAction.Done
                    ),
                    colors = fieldColors,
                    modifier = Modifier.fillMaxWidth()
                )

                Spacer(modifier = Modifier.height(16.dp))

                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.End,
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    TextButton(
                        onClick = onDismiss,
                        modifier = Modifier.heightIn(min = 48.dp),
                        colors = ButtonDefaults.textButtonColors(contentColor = TextSecondary)
                    ) {
                        Text("Cancelar")
                    }
                    Spacer(modifier = Modifier.width(8.dp))
                    Button(
                        onClick = {
                            val cleanName = name.trim()
                            val cleanIp = ipAddress.trim()
                            val parsedPort = portText.trim().toIntOrNull()

                            var valid = true
                            if (cleanName.isBlank()) {
                                nameError = "Informe o nome do computador"
                                valid = false
                            }
                            if (!isValidHostOrIp(cleanIp)) {
                                ipError = "Informe um endereço IP válido (ex: 192.168.1.100)"
                                valid = false
                            }
                            if (parsedPort == null || parsedPort !in 1..65535) {
                                portError = "Porta inválida (1 a 65535)"
                                valid = false
                            }

                            if (valid && parsedPort != null) {
                                onSave(
                                    ComputerProfile(
                                        name = cleanName,
                                        ipAddress = cleanIp,
                                        port = parsedPort,
                                        connectionMethodId = ConnectionMethod.NETWORK_LAN.id
                                    )
                                )
                            }
                        },
                        modifier = Modifier.heightIn(min = 48.dp),
                        colors = ButtonDefaults.buttonColors(
                            containerColor = AccentBlue,
                            contentColor = Color(0xFF041E30)
                        ),
                        shape = RoundedCornerShape(12.dp)
                    ) {
                        Text("Salvar", fontWeight = FontWeight.SemiBold)
                    }
                }
            }
        }
    }
}

@Composable
private fun ConnectionMethodsDialog(onDismiss: () -> Unit) {
    AlertDialog(
        onDismissRequest = onDismiss,
        containerColor = CardSurface,
        titleContentColor = TextPrimary,
        textContentColor = TextSecondary,
        title = {
            Text(
                text = "Métodos de Conexão",
                fontWeight = FontWeight.Bold
            )
        },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
                ConnectionMethod.entries.forEach { method ->
                    val icon = when (method) {
                        ConnectionMethod.NETWORK_LAN -> Icons.Default.Wifi
                        ConnectionMethod.BLUETOOTH -> Icons.Default.Bluetooth
                        ConnectionMethod.USB_ADB -> Icons.Default.Usb
                    }
                    val title = when (method) {
                        ConnectionMethod.NETWORK_LAN -> "Via Rede (Wi-Fi / LAN)"
                        ConnectionMethod.BLUETOOTH -> "Bluetooth"
                        ConnectionMethod.USB_ADB -> "Cabo USB (ADB)"
                    }
                    val description = when (method) {
                        ConnectionMethod.NETWORK_LAN -> "Conecte pelo Wi-Fi local com baixa latência."
                        ConnectionMethod.BLUETOOTH -> "Conexão direta sem depender de rede Wi-Fi."
                        ConnectionMethod.USB_ADB -> "Conexão via cabo USB com túnel ADB."
                    }

                    Surface(
                        color = Color(0xFF1E293B),
                        shape = RoundedCornerShape(12.dp),
                        border = BorderStroke(
                            width = 1.dp,
                            color = if (method.isAvailableNow) AccentBlue.copy(alpha = 0.5f) else CardBorder
                        )
                    ) {
                        Row(
                            modifier = Modifier
                                .fillMaxWidth()
                                .padding(12.dp),
                            verticalAlignment = Alignment.CenterVertically
                        ) {
                            Icon(
                                imageVector = icon,
                                contentDescription = null,
                                tint = if (method.isAvailableNow) AccentBlue else OfflineSlate,
                                modifier = Modifier.size(22.dp)
                            )
                            Spacer(modifier = Modifier.width(12.dp))
                            Column(modifier = Modifier.weight(1f)) {
                                Text(
                                    text = title,
                                    color = TextPrimary,
                                    fontSize = 14.sp,
                                    fontWeight = FontWeight.SemiBold
                                )
                                Text(
                                    text = description,
                                    color = TextSecondary,
                                    fontSize = 12.sp
                                )
                            }
                            Spacer(modifier = Modifier.width(8.dp))
                            Text(
                                text = if (method.isAvailableNow) "Ativo" else "Em breve",
                                color = if (method.isAvailableNow) OnlineGreen else OfflineSlate,
                                fontSize = 11.sp,
                                fontWeight = FontWeight.SemiBold
                            )
                        }
                    }
                }
            }
        },
        confirmButton = {
            TextButton(onClick = onDismiss) {
                Text("Fechar", color = AccentBlue, fontWeight = FontWeight.SemiBold)
            }
        }
    )
}

private fun isValidHostOrIp(raw: String): Boolean {
    val value = raw.trim()
    if (value.length !in 1..253 || value.any { it.isWhitespace() }) return false
    if (!value.any { it.isLetterOrDigit() }) return false
    if (value.startsWith('.') || value.endsWith('.') || value.startsWith('-') || value.endsWith('-')) {
        return false
    }

    if (value.all { it.isDigit() || it == '.' }) {
        val ipv4Regex = Regex("""^(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})$""")
        val match = ipv4Regex.matchEntire(value) ?: return false
        return match.groupValues.drop(1).all { octet ->
            octet.toIntOrNull()?.let { it in 0..255 } == true
        }
    }

    return value.all { it.isLetterOrDigit() || it == '.' || it == '-' || it == ':' }
}
