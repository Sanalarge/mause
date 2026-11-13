package io.github.bytaymur.mause

import android.os.Build
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.KeyboardArrowRight
import androidx.compose.material.icons.rounded.Cast
import androidx.compose.material.icons.rounded.DesktopWindows
import androidx.compose.material.icons.rounded.ErrorOutline
import androidx.compose.material.icons.rounded.UploadFile
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

private val SharingSummary = listOf(
    SettingsSection.Files,
    SettingsSection.Audio,
    SettingsSection.Screen,
    SettingsSection.Hardware,
)

@Composable
fun HomeScreen(
    version: String,
    share: ShareActions,
    onStart: () -> Unit,
    onStop: () -> Unit,
    onOpenSection: (SettingsSection) -> Unit,
) {
    Column(verticalArrangement = Arrangement.spacedBy(14.dp)) {
        Header()
        ConnectionCard(onStart, onStop)
        if (!AgentState.accessibilityReady) {
            SetupCard { onOpenSection(SettingsSection.Pointer) }
        }
        GroupLabel("Hızlı işlemler")
        Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            ActionTile(
                Icons.Rounded.UploadFile,
                "Dosya gönder",
                "Bilgisayara",
                AgentState.connected,
                share.onSendFiles,
                Modifier.weight(1f),
            )
            ActionTile(
                Icons.Rounded.Cast,
                "Ekranı yansıt",
                "Bilgisayarın ekranı",
                AgentState.connected,
                { share.onViewScreen(false) },
                Modifier.weight(1f),
            )
        }
        Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            ActionTile(
                Icons.Rounded.DesktopWindows,
                "İkinci monitör",
                "Masaüstünü genişlet",
                AgentState.connected,
                { share.onViewScreen(true) },
                Modifier.weight(1f),
            )
            ActionTile(
                SettingsSection.Screen.icon,
                "Ekranımı paylaş",
                sectionStatus(SettingsSection.Screen, version).text,
                true,
                { onOpenSection(SettingsSection.Screen) },
                Modifier.weight(1f),
            )
        }
        if (!AgentState.connected) {
            Note("Hızlı işlemler bir bilgisayara bağlanınca açılır.")
        }
        GroupLabel("Paylaşılanlar")
        SharingSummaryCard(version, onOpenSection)
        GroupLabel("Son işlemler")
        ActivityList()
    }
}

@Composable
private fun Header() {
    Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.padding(bottom = 4.dp)) {
        Image(
            painter = painterResource(R.drawable.ic_mause),
            contentDescription = null,
            modifier = Modifier.size(38.dp),
        )
        Text(
            "mause",
            modifier = Modifier.padding(start = 10.dp).weight(1f),
            color = MauseColors.Ink,
            fontWeight = FontWeight.SemiBold,
            fontSize = 22.sp,
        )
        Column(horizontalAlignment = Alignment.End) {
            Text(
                Build.MODEL,
                color = MauseColors.Ink,
                fontSize = 13.sp,
                fontWeight = FontWeight.Medium,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            Text("Bu cihaz", color = MauseColors.Muted, fontSize = 11.sp)
        }
    }
}

private class Connection(val title: String, val color: Color)

private fun connection() = when {
    AgentState.connected -> Connection("Bağlı", MauseColors.Ok)
    AgentState.running -> Connection("Bilgisayar bekleniyor", MauseColors.Accent)
    else -> Connection("Kapalı", MauseColors.Off)
}

@Composable
private fun ConnectionCard(onStart: () -> Unit, onStop: () -> Unit) {
    val state = connection()
    Column(
        Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(20.dp))
            .background(MauseColors.Surface)
            .border(1.dp, MauseColors.RuleStrong, RoundedCornerShape(20.dp))
            .padding(20.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Box(
                Modifier
                    .size(46.dp)
                    .clip(CircleShape)
                    .background(state.color.copy(alpha = 0.14f)),
                contentAlignment = Alignment.Center,
            ) {
                Box(Modifier.size(14.dp).clip(CircleShape).background(state.color))
            }
            Column(Modifier.padding(start = 14.dp)) {
                Text(state.title, color = MauseColors.Ink, fontSize = 20.sp, fontWeight = FontWeight.SemiBold)
                Text(
                    if (AgentState.running) AgentState.status else "Bilgisayarınıza bağlanmak için başlatın.",
                    color = MauseColors.Muted,
                    fontSize = 13.sp,
                    lineHeight = 18.sp,
                )
            }
        }
        if (AgentState.running) {
            SecondaryButton("Bağlantıyı durdur", onStop, modifier = Modifier.fillMaxWidth().height(50.dp))
        } else {
            PrimaryButton(
                "Bu cihazı kullanıma aç",
                onStart,
                enabled = AgentState.accessibilityReady,
                modifier = Modifier.fillMaxWidth().height(50.dp),
            )
        }
    }
}

@Composable
private fun SetupCard(onOpen: () -> Unit) {
    Row(
        Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(18.dp))
            .background(MauseColors.Accent.copy(alpha = 0.10f))
            .border(1.dp, MauseColors.Accent.copy(alpha = 0.45f), RoundedCornerShape(18.dp))
            .clickable(onClick = onOpen)
            .padding(16.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(Icons.Rounded.ErrorOutline, contentDescription = null, tint = MauseColors.Accent)
        Column(Modifier.weight(1f).padding(start = 12.dp)) {
            Text("Kurulumu tamamlayın", color = MauseColors.Ink, fontWeight = FontWeight.SemiBold, fontSize = 15.sp)
            Text(
                "Fare ve dokunma için erişilebilirlik izni gerekli.",
                color = MauseColors.Ink2,
                fontSize = 13.sp,
            )
        }
        Icon(Icons.AutoMirrored.Rounded.KeyboardArrowRight, contentDescription = null, tint = MauseColors.Accent)
    }
}

@Composable
private fun ActionTile(
    icon: ImageVector,
    title: String,
    subtitle: String,
    enabled: Boolean,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val alpha = if (enabled) 1f else 0.45f
    Column(
        modifier
            .clip(RoundedCornerShape(18.dp))
            .background(MauseColors.Surface)
            .clickable(enabled = enabled, onClick = onClick)
            .padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        IconBadge(icon, MauseColors.Accent.copy(alpha = alpha))
        Column {
            Text(
                title,
                color = MauseColors.Ink.copy(alpha = alpha),
                fontSize = 15.sp,
                fontWeight = FontWeight.Medium,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            Text(
                subtitle,
                color = MauseColors.Muted.copy(alpha = alpha),
                fontSize = 12.sp,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
    }
}

@Composable
private fun SharingSummaryCard(version: String, onOpen: (SettingsSection) -> Unit) {
    Row(
        Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(18.dp))
            .background(MauseColors.Surface)
            .padding(vertical = 14.dp, horizontal = 6.dp),
    ) {
        SharingSummary.forEach { section ->
            val status = sectionStatus(section, version)
            Column(
                Modifier
                    .weight(1f)
                    .clip(RoundedCornerShape(12.dp))
                    .clickable { onOpen(section) }
                    .padding(vertical = 6.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
                verticalArrangement = Arrangement.spacedBy(6.dp),
            ) {
                Icon(section.icon, contentDescription = null, tint = MauseColors.Ink2, modifier = Modifier.size(22.dp))
                Text(section.title.substringBefore(' '), color = MauseColors.Ink, fontSize = 12.sp)
                Box(Modifier.size(6.dp).clip(CircleShape).background(status.color))
            }
        }
    }
}

@Composable
private fun ActivityList() {
    Column(
        Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(18.dp))
            .background(MauseColors.Surface)
            .padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        if (AgentState.activity.isEmpty()) {
            Note("Henüz bir işlem yok. Dosya, ses ve ekran olayları burada görünür.")
        }
        AgentState.activity.forEach { line ->
            Row(verticalAlignment = Alignment.Top) {
                Box(
                    Modifier
                        .padding(top = 7.dp)
                        .size(6.dp)
                        .clip(CircleShape)
                        .background(MauseColors.Accent),
                )
                Text(line, color = MauseColors.Ink2, fontSize = 13.sp, lineHeight = 18.sp, modifier = Modifier.padding(start = 10.dp))
            }
        }
    }
}
