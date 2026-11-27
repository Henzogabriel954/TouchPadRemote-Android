package com.touchpad.android.ui

import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ArrowDropDown
import androidx.compose.material.icons.filled.DeleteOutline
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Slider
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.touchpad.android.model.AndroidSettings
import com.touchpad.android.model.ComputerProfile
import com.touchpad.android.model.EdgeLink
import com.touchpad.android.model.ScreenEdge
import java.util.Locale
import kotlin.math.roundToInt

@Composable
internal fun SettingsDialog(
    settings: AndroidSettings,
    onDismiss: () -> Unit,
    onSave: (AndroidSettings) -> Unit
) {
    var draft by remember { mutableStateOf(settings.copy(holdDurationMs = settings.holdDurationMs.coerceIn(120, 600))) }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Configurações") },
        text = {
            Column(
                modifier = Modifier.verticalScroll(rememberScrollState()),
                verticalArrangement = Arrangement.spacedBy(8.dp)
            ) {
                val sensitivity = String.format(Locale.forLanguageTag("pt-BR"), "%.1f", draft.localSensitivity)
                Text("Sensibilidade local · ${sensitivity}×")
                Slider(
                    value = draft.localSensitivity,
                    onValueChange = { draft = draft.copy(localSensitivity = it) },
                    valueRange = 0.3f..3f,
                    steps = 26,
                    modifier = Modifier.semantics { contentDescription = "Sensibilidade local" }
                )
                Text("Tempo para segurar clique · ${draft.holdDurationMs} ms")
                Slider(
                    value = draft.holdDurationMs.toFloat(),
                    onValueChange = { draft = draft.copy(holdDurationMs = it.roundToInt()) },
                    valueRange = 120f..600f,
                    modifier = Modifier.semantics { contentDescription = "Tempo para segurar clique" }
                )
                ToggleRow("Vibração háptica", draft.hapticsEnabled) {
                    draft = draft.copy(hapticsEnabled = it)
                }
                ToggleRow("Abrir em paisagem por padrão", draft.defaultLandscape) {
                    draft = draft.copy(defaultLandscape = it)
                }
            }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Cancelar") } },
        confirmButton = { TextButton(onClick = { onSave(draft) }) { Text("Salvar") } }
    )
}

@Composable
internal fun MultiPcLayoutDialog(
    computers: List<ComputerProfile>,
    links: List<EdgeLink>,
    settings: AndroidSettings,
    onDismiss: () -> Unit,
    onSave: (List<EdgeLink>, Boolean, Int, String?) -> Unit
) {
    var draft by remember { mutableStateOf(links) }
    var enabled by remember { mutableStateOf(settings.multiPcEnabled) }
    var overshoot by remember { mutableStateOf(settings.edgeOvershootPx) }
    var fromId by remember { mutableStateOf(computers.firstOrNull()?.id) }
    var toId by remember { mutableStateOf(computers.getOrNull(1)?.id) }
    var fromEdge by remember { mutableStateOf(ScreenEdge.RIGHT) }
    var toEdge by remember { mutableStateOf(ScreenEdge.LEFT) }
    var mirror by remember { mutableStateOf(true) }
    var preferredPrimary by remember { mutableStateOf<String?>(null) }

    val names = computers.associate { it.id to it.name }
    val validLinks = draft.filter { it.fromComputerId in names && it.toComputerId in names }
    val participants = computers.filter { computer ->
        validLinks.any { it.fromComputerId == computer.id || it.toComputerId == computer.id }
    }
    val primaryId = preferredPrimary?.takeIf { id -> participants.any { it.id == id } }
        ?: participants.firstOrNull()?.id

    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Multi-PC (Bordas)") },
        text = {
            Column(
                modifier = Modifier.heightIn(max = 520.dp).verticalScroll(rememberScrollState()),
                verticalArrangement = Arrangement.spacedBy(8.dp)
            ) {
                ToggleRow("Alternar PCs pelas bordas", enabled) { enabled = it }
                if (computers.size < 2) {
                    Text("Cadastre pelo menos dois computadores para conectar suas bordas.")
                } else {
                    ChoiceMenu("De", fromId, computers.map { it.id }, { names[it].orEmpty() }) { id ->
                        fromId = id
                        if (toId == id) toId = computers.first { it.id != id }.id
                    }
                    ChoiceMenu("Borda de saída", fromEdge, ScreenEdge.entries, ScreenEdge::label) { edge ->
                        fromEdge = edge
                        toEdge = edge.opposite
                    }
                    ChoiceMenu(
                        "Para", toId, computers.filter { it.id != fromId }.map { it.id }, { names[it].orEmpty() }
                    ) { toId = it }
                    ChoiceMenu("Borda de entrada", toEdge, ScreenEdge.entries, ScreenEdge::label) { toEdge = it }
                    ToggleRow("Espelhar automaticamente (⇄)", mirror) { mirror = it }
                    OutlinedButton(
                        enabled = fromId in names && toId in names && fromId != toId,
                        onClick = {
                            val link = EdgeLink(checkNotNull(fromId), fromEdge, checkNotNull(toId), toEdge)
                            val additions = if (mirror) listOf(link, link.reversed()) else listOf(link)
                            val overwritten = validLinks.filter { existing ->
                                additions.any {
                                    it.fromComputerId == existing.fromComputerId && it.fromEdge == existing.fromEdge
                                }
                            }
                            val staleMirrors = overwritten.filterNot { it in additions }.map(EdgeLink::reversed)
                            draft = validLinks - (overwritten + staleMirrors).toSet() + additions
                        }
                    ) { Text("Adicionar vínculo") }
                }

                val previews = validLinks.filterIndexed { index, link -> link.reversed() !in validLinks.take(index) }
                previews.forEach { link ->
                    val mirrored = link.reversed() in validLinks
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(4.dp)
                    ) {
                        EdgePreview(link, names, mirrored, Modifier.weight(1f))
                        IconButton(onClick = { draft = draft - link - link.reversed() }) {
                            Icon(Icons.Default.DeleteOutline, contentDescription = "Remover vínculo")
                        }
                    }
                }
                if (participants.size >= 2) {
                    ChoiceMenu(
                        "PC inicial", primaryId, participants.map { it.id }, { names[it].orEmpty() }
                    ) { preferredPrimary = it }
                }
                Text("Resistência da borda · $overshoot px")
                Slider(
                    value = overshoot.toFloat(),
                    onValueChange = { overshoot = it.roundToInt() },
                    valueRange = 0f..500f,
                    modifier = Modifier.semantics { contentDescription = "Resistência da borda" }
                )
            }
        },
        dismissButton = {
            TextButton(onClick = { onSave(validLinks, enabled, overshoot, null) }) { Text("Salvar") }
        },
        confirmButton = {
            TextButton(
                enabled = enabled && primaryId != null,
                onClick = { onSave(validLinks, enabled, overshoot, primaryId) }
            ) { Text("Iniciar Sessão Multi-PC") }
        }
    )
}

@Composable
private fun EdgePreview(link: EdgeLink, names: Map<String, String>, mirrored: Boolean, modifier: Modifier) {
    val reverseOrder = link.fromEdge == ScreenEdge.LEFT || link.fromEdge == ScreenEdge.TOP
    val first = if (reverseOrder) link.reversed() else link
    val start = names.getValue(first.fromComputerId)
    val end = names.getValue(first.toComputerId)
    val vertical = link.fromEdge == ScreenEdge.TOP || link.fromEdge == ScreenEdge.BOTTOM
    val arrow = when {
        mirrored && vertical -> "↕"
        mirrored -> "⇄"
        else -> link.fromEdge.arrow
    }
    val arrowColor = MaterialTheme.colorScheme.primary
    if (vertical) {
        Column(modifier = modifier, horizontalAlignment = Alignment.CenterHorizontally) {
            EdgeEndpoint(start, first.fromEdge, Modifier.fillMaxWidth())
            Text(arrow, color = arrowColor)
            EdgeEndpoint(end, first.toEdge, Modifier.fillMaxWidth())
        }
    } else {
        Row(
            modifier = modifier,
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(4.dp)
        ) {
            EdgeEndpoint(start, first.fromEdge, Modifier.weight(1f))
            Text(arrow, color = arrowColor)
            EdgeEndpoint(end, first.toEdge, Modifier.weight(1f))
        }
    }
}

@Composable
private fun EdgeEndpoint(name: String, edge: ScreenEdge, modifier: Modifier = Modifier) {
    Column(
        modifier = modifier
            .border(1.dp, MaterialTheme.colorScheme.outline, MaterialTheme.shapes.small)
            .padding(6.dp),
        horizontalAlignment = Alignment.CenterHorizontally
    ) {
        Text(name, maxLines = 1, overflow = TextOverflow.Ellipsis, style = MaterialTheme.typography.labelLarge)
        Text(edge.label, color = MaterialTheme.colorScheme.primary, style = MaterialTheme.typography.labelSmall)
    }
}

@Composable
private fun ToggleRow(label: String, checked: Boolean, onCheckedChange: (Boolean) -> Unit) {
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.spacedBy(12.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        Text(label, modifier = Modifier.weight(1f))
        Switch(
            checked = checked,
            onCheckedChange = onCheckedChange,
            modifier = Modifier.semantics { contentDescription = label }
        )
    }
}

@Composable
private fun <T> ChoiceMenu(label: String, selected: T?, choices: List<T>, name: (T) -> String, onSelect: (T) -> Unit) {
    var expanded by remember { mutableStateOf(false) }
    Column {
        OutlinedButton(onClick = { expanded = true }, enabled = choices.isNotEmpty()) {
            Text("$label · ${selected?.let(name) ?: "Selecionar"}", maxLines = 1, overflow = TextOverflow.Ellipsis)
            Icon(Icons.Default.ArrowDropDown, contentDescription = null)
        }
        DropdownMenu(expanded = expanded, onDismissRequest = { expanded = false }) {
            choices.forEach { choice ->
                DropdownMenuItem(text = { Text(name(choice)) }, onClick = {
                    onSelect(choice)
                    expanded = false
                })
            }
        }
    }
}
