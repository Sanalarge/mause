package io.github.bytaymur.mause

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.ArrowBack
import androidx.compose.material.icons.automirrored.rounded.KeyboardArrowRight
import androidx.compose.material.icons.automirrored.rounded.ScreenShare
import androidx.compose.material.icons.automirrored.rounded.VolumeUp
import androidx.compose.material.icons.rounded.Folder
import androidx.compose.material.icons.rounded.Info
import androidx.compose.material.icons.rounded.Keyboard
import androidx.compose.material.icons.rounded.Mouse
import androidx.compose.material.icons.rounded.PhoneAndroid
import androidx.compose.material.icons.rounded.Sync
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

enum class SettingsSection(val title: String, val icon: ImageVector) {
    Pointer("Fare ve dokunma", Icons.Rounded.Mouse),
    Keyboard("Klavye", Icons.Rounded.Keyboard),
    Files("Dosyalar", Icons.Rounded.Folder),
    Audio("Ses", Icons.AutoMirrored.Rounded.VolumeUp),
    Screen("Ekran paylaşımı", Icons.AutoMirrored.Rounded.ScreenShare),
    Hardware("Donanım", Icons.Rounded.PhoneAndroid),
    Remote("Bilgisayarla ortak ayar", Icons.Rounded.Sync),
    About("Hakkında", Icons.Rounded.Info),
}

private val Permissions = listOf(SettingsSection.Pointer, SettingsSection.Keyboard)
private val Sharing = listOf(
    SettingsSection.Files,
    SettingsSection.Audio,
    SettingsSection.Screen,
    SettingsSection.Hardware,
    SettingsSection.Remote,
)
private val Application = listOf(SettingsSection.About)

class SectionStatus(val text: String, val color: Color)

private fun onOff(on: Boolean) =
    if (on) SectionStatus("Açık", MauseColors.Ok) else SectionStatus("Kapalı", MauseColors.Muted)

fun sectionStatus(section: SettingsSection, version: String): SectionStatus = when (section) {
    SettingsSection.Pointer ->
        if (AgentState.accessibilityReady) SectionStatus("Hazır", MauseColors.Ok)
        else SectionStatus("İzin gerekli", MauseColors.Accent)
    SettingsSection.Keyboard ->
        if (AgentState.accessibilityReady) SectionStatus("Hazır · ${AgentState.keyboardLayout.title}", MauseColors.Ok)
        else SectionStatus("Fare izni gerekli", MauseColors.Accent)
    SettingsSection.Files -> onOff(AgentState.filesAllowed)
    SettingsSection.Audio -> onOff(AgentState.shareAudio || AgentState.shareMic || AgentState.playPcAudio)
    SettingsSection.Screen -> when {
        AgentState.shareScreen && AgentState.captureReady -> SectionStatus("Açık", MauseColors.Ok)
        AgentState.shareScreen -> SectionStatus("Yakalama izni bekleniyor", MauseColors.Accent)
        else -> onOff(false)
    }
    SettingsSection.Hardware -> onOff(AgentState.shareCamera || AgentState.shareDevice)
    SettingsSection.Remote -> onOff(AgentState.remotePrefs)
    SettingsSection.About -> SectionStatus("Sürüm $version", MauseColors.Muted)
}

@Composable
fun SettingsMenu(version: String, onOpen: (SettingsSection) -> Unit) {
    Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
        PageTitle("Ayarlar")
        SettingsGroup("İzinler", Permissions, version, onOpen)
        SettingsGroup("Paylaşım", Sharing, version, onOpen)
        SettingsGroup("Uygulama", Application, version, onOpen)
    }
}

@Composable
fun SettingsDetail(
    section: SettingsSection,
    version: String,
    share: ShareActions,
    system: SystemActions,
    onBack: () -> Unit,
) {
    Column(verticalArrangement = Arrangement.spacedBy(16.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            IconButton(onClick = onBack) {
                Icon(Icons.AutoMirrored.Rounded.ArrowBack, contentDescription = "Geri", tint = MauseColors.Ink)
            }
            Column(Modifier.padding(start = 4.dp)) {
                Text(section.title, color = MauseColors.Ink, fontSize = 22.sp, fontWeight = FontWeight.SemiBold)
                val status = sectionStatus(section, version)
                Text(status.text, color = status.color, fontSize = 13.sp)
            }
        }
        when (section) {
            SettingsSection.Pointer -> PointerPanel(system)
            SettingsSection.Keyboard -> KeyboardPanel(system)
            SettingsSection.Files -> FilesPanel(share)
            SettingsSection.Audio -> AudioPanel(share)
            SettingsSection.Screen -> ScreenPanel(share)
            SettingsSection.Hardware -> HardwarePanel(share)
            SettingsSection.Remote -> RemotePanel(share)
            SettingsSection.About -> AboutPanel(version, system)
        }
    }
}

@Composable
fun PageTitle(text: String) {
    Text(
        text,
        color = MauseColors.Ink,
        fontSize = 26.sp,
        fontWeight = FontWeight.SemiBold,
        modifier = Modifier.padding(bottom = 6.dp),
    )
}

@Composable
fun GroupLabel(text: String) {
    Text(
        text.uppercase(),
        color = MauseColors.Muted,
        fontSize = 12.sp,
        fontWeight = FontWeight.Medium,
        letterSpacing = 0.8.sp,
        modifier = Modifier.padding(start = 4.dp, top = 8.dp),
    )
}

@Composable
private fun SettingsGroup(
    label: String,
    sections: List<SettingsSection>,
    version: String,
    onOpen: (SettingsSection) -> Unit,
) {
    GroupLabel(label)
    Column(
        Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(18.dp))
            .background(MauseColors.Surface),
    ) {
        sections.forEachIndexed { index, section ->
            if (index > 0) HorizontalDivider(color = MauseColors.Rule, modifier = Modifier.padding(start = 70.dp))
            SettingsRow(section, sectionStatus(section, version)) { onOpen(section) }
        }
    }
}

@Composable
fun IconBadge(icon: ImageVector, tint: Color = MauseColors.Accent) {
    Box(
        Modifier
            .size(40.dp)
            .clip(RoundedCornerShape(12.dp))
            .background(MauseColors.Inset),
        contentAlignment = Alignment.Center,
    ) {
        Icon(icon, contentDescription = null, tint = tint, modifier = Modifier.size(22.dp))
    }
}

@Composable
private fun SettingsRow(section: SettingsSection, status: SectionStatus, onClick: () -> Unit) {
    Row(
        Modifier
            .fillMaxWidth()
            .clickable(onClick = onClick)
            .padding(horizontal = 16.dp, vertical = 14.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        IconBadge(section.icon)
        Column(Modifier.weight(1f).padding(start = 14.dp)) {
            Text(section.title, color = MauseColors.Ink, fontSize = 15.sp, fontWeight = FontWeight.Medium)
            Text(status.text, color = status.color, fontSize = 12.5.sp)
        }
        Icon(
            Icons.AutoMirrored.Rounded.KeyboardArrowRight,
            contentDescription = null,
            tint = MauseColors.Muted,
        )
    }
}
