package io.github.bytaymur.mause

import androidx.compose.foundation.Image
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Switch
import androidx.compose.material3.SwitchDefaults
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

class ShareActions(
    val onClipboard: (Boolean) -> Unit,
    val onSendClipboard: () -> Unit,
    val onPickClipboardImage: () -> Unit,
    val onFiles: (Boolean) -> Unit,
    val onStorage: () -> Unit,
    val onSendFiles: () -> Unit,
    val onShareAudio: (Boolean) -> Unit,
    val onShareMic: (Boolean) -> Unit,
    val onPlayAudio: (Boolean) -> Unit,
    val onShareScreen: (Boolean) -> Unit,
    val onPrepareCapture: () -> Unit,
    val onViewScreen: (Boolean) -> Unit,
    val onRemotePrefs: (Boolean) -> Unit,
    val onShareCamera: (Boolean) -> Unit,
    val onShareDevice: (Boolean) -> Unit,
)

class SystemActions(
    val onAccessibility: () -> Unit,
    val onKeyboard: () -> Unit,
    val onAppInfo: () -> Unit,
    val onLayout: (KeyLayout) -> Unit,
)

@Composable
fun PanelCard(content: @Composable ColumnScope.() -> Unit) {
    Card(
        colors = CardDefaults.cardColors(containerColor = MauseColors.Surface),
        shape = RoundedCornerShape(18.dp),
        modifier = Modifier.fillMaxWidth(),
    ) {
        Column(
            Modifier.padding(18.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
            content = content,
        )
    }
}

@Composable
fun Note(text: String) {
    Text(text, color = MauseColors.Muted, fontSize = 13.sp, lineHeight = 19.sp)
}

@Composable
fun PrimaryButton(text: String, onClick: () -> Unit, enabled: Boolean = true, modifier: Modifier = Modifier) {
    Button(
        onClick = onClick,
        enabled = enabled,
        modifier = modifier,
        shape = RoundedCornerShape(12.dp),
        colors = ButtonDefaults.buttonColors(
            containerColor = MauseColors.Accent,
            contentColor = MauseColors.OnAccent,
            disabledContainerColor = MauseColors.Inset,
            disabledContentColor = MauseColors.Muted,
        ),
    ) { Text(text, fontWeight = FontWeight.Medium) }
}

@Composable
fun SecondaryButton(text: String, onClick: () -> Unit, enabled: Boolean = true, modifier: Modifier = Modifier) {
    OutlinedButton(
        onClick = onClick,
        enabled = enabled,
        modifier = modifier,
        shape = RoundedCornerShape(12.dp),
        colors = ButtonDefaults.outlinedButtonColors(
            contentColor = MauseColors.Ink,
            disabledContentColor = MauseColors.Muted,
        ),
    ) { Text(text, fontWeight = FontWeight.Medium) }
}

@Composable
private fun SwitchRow(
    label: String,
    checked: Boolean,
    onChange: (Boolean) -> Unit,
    detail: String? = null,
) {
    Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
        Column(Modifier.weight(1f).padding(end = 12.dp)) {
            Text(label, fontSize = 15.sp, color = MauseColors.Ink)
            detail?.let { Text(it, color = MauseColors.Muted, fontSize = 12.sp, lineHeight = 17.sp) }
        }
        Switch(
            checked = checked,
            onCheckedChange = onChange,
            colors = SwitchDefaults.colors(
                checkedThumbColor = MauseColors.OnAccent,
                checkedTrackColor = MauseColors.Accent,
                uncheckedThumbColor = MauseColors.Muted,
                uncheckedTrackColor = MauseColors.Inset,
                uncheckedBorderColor = MauseColors.RuleStrong,
            ),
        )
    }
}

@Composable
private fun Divider() {
    HorizontalDivider(color = MauseColors.Rule)
}

@Composable
fun PointerPanel(system: SystemActions) {
    PanelCard {
        Note("İmleç, tıklama ve kaydırma için Android erişilebilirlik servisi gerekir.")
        if (!AgentState.accessibilityReady) {
            Note("İzin \"kısıtlı ayar\" diye engellenirse: Uygulama bilgisi → sağ üstteki üç nokta → Kısıtlanmış ayarlara izin ver. Sonra İzin ver'e yeniden basın.")
            Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                PrimaryButton("İzin ver", system.onAccessibility)
                SecondaryButton("Uygulama bilgisi", system.onAppInfo)
            }
        } else {
            Text("Erişilebilirlik servisi açık.", color = MauseColors.Ok, fontSize = 13.sp)
        }
    }
}

@Composable
fun KeyboardPanel(system: SystemActions) {
    PanelCard {
        Note("Bilgisayar klavyesiyle odaklanan metin kutusuna yazılır. Bilgisayarınızın klavye düzenini seçin.")
        Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
            KeyLayout.entries.forEach { option ->
                if (AgentState.keyboardLayout == option) {
                    PrimaryButton(option.title, { system.onLayout(option) })
                } else {
                    SecondaryButton(option.title, { system.onLayout(option) })
                }
            }
        }
        Divider()
        Note(
            if (AgentState.keyboardReady) {
                "Mause Klavye etkin: yukarı ve aşağı ok tuşları da çalışır."
            } else {
                "İsteğe bağlı: yukarı ve aşağı ok tuşları için Mause Klavye'yi etkinleştirip klavye olarak seçin."
            },
        )
        if (!AgentState.keyboardReady) {
            SecondaryButton("Mause Klavye", system.onKeyboard)
        }
    }
}

@Composable
fun RemotePanel(actions: ShareActions) {
    PanelCard {
        SwitchRow(
            "Bilgisayar paylaşım ayarlarımı değiştirebilsin",
            AgentState.remotePrefs,
            actions.onRemotePrefs,
            detail = "Bilgisayarda bir akış tuşuna basınca burada da aynı akış açılır ya da kapanır. Kapalıysa bilgisayar yalnız kapatabilir.",
        )
    }
}

@Composable
fun ClipboardPanel(actions: ShareActions) {
    PanelCard {
        SwitchRow("Pano paylaşımı", AgentState.clipboardAllowed, actions.onClipboard)
        AgentState.clipboardImage?.let { preview ->
            Image(
                bitmap = preview.bitmap.asImageBitmap(),
                contentDescription = "Panodaki görsel",
                modifier = Modifier.fillMaxWidth().heightIn(max = 200.dp),
            )
        }
        val canSend = AgentState.connected && AgentState.clipboardAllowed && !AgentState.clipboardSending
        SecondaryButton("Panodaki görseli gönder", actions.onSendClipboard, enabled = canSend)
        SecondaryButton("Ekran görüntüsü seç ve gönder", actions.onPickClipboardImage, enabled = canSend)
        if (AgentState.clipboardSending) Note("Görsel gönderiliyor…")
    }
}

@Composable
fun FilesPanel(actions: ShareActions) {
    PanelCard {
        Note("Bilgisayar bu cihazın dosyalarını görüp indirebilir ve dosya yükleyebilir. Bilgisayardan gelen dosyalar İndirilenler/Mause klasörüne kaydedilir.")
        SwitchRow("Bilgisayarın dosyalara erişmesine izin ver", AgentState.filesAllowed, actions.onFiles)
        if (AgentState.filesAllowed && !AgentState.storageReady) {
            Note("Tüm dosyalara erişim izni olmadan yalnızca Mause'un kendi klasörü paylaşılır.")
            SecondaryButton("Tüm dosyalara erişim izni", actions.onStorage)
        }
        Divider()
        SecondaryButton("Bilgisayara dosya gönder", actions.onSendFiles, enabled = AgentState.connected)
    }
}

@Composable
fun AudioPanel(actions: ShareActions) {
    PanelCard {
        SwitchRow(
            "Bu cihazın sesini bilgisayara gönder",
            AgentState.shareAudio,
            actions.onShareAudio,
            detail = "Ekran yakalama izni ve mikrofon izni gerekir.",
        )
        if (AgentState.shareAudio && !AgentState.captureReady) {
            Note("Cihaz sesi için Ayarlar → Ekran bölümünden yakalama iznini hazırlayın.")
        }
        Divider()
        SwitchRow("Mikrofonu bilgisayara gönder", AgentState.shareMic, actions.onShareMic)
        Divider()
        SwitchRow("Bilgisayarın sesini bu cihazda çal", AgentState.playPcAudio, actions.onPlayAudio)
    }
}

@Composable
fun ScreenPanel(actions: ShareActions) {
    PanelCard {
        Note("Bilgisayardan bu cihazın ekranını izleyin. Erişilebilirlik servisi açıksa bilgisayardan dokunma ve klavye de çalışır.")
        if (!AgentState.captureReady) {
            Note("Android ekran yakalama için onay ister. Onay, Mause durdurulunca sıfırlanır.")
            PrimaryButton("Ekran yakalamayı hazırla", actions.onPrepareCapture, enabled = AgentState.running)
        }
        Divider()
        SwitchRow("Bilgisayarın ekranı izlemesine izin ver", AgentState.shareScreen, actions.onShareScreen)
        if (AgentState.screenLive) {
            Text("Ekran şu an bilgisayara aktarılıyor", color = MauseColors.Ok, fontSize = 13.sp)
        }
    }
}

@Composable
fun HardwarePanel(actions: ShareActions) {
    PanelCard {
        Note("Bilgisayar bu cihazın kameralarını canlı izleyebilir, el fenerini açıp titreşimi çalıştırabilir, pil ve konum bilgisini görebilir. Bilgisayarda cihazın monitör kartına tıklayınca menüde çıkar.")
        SwitchRow(
            "Kameraları bilgisayarla paylaş",
            AgentState.shareCamera,
            actions.onShareCamera,
            detail = "Kamera izni gerekir. Kamera yalnız bilgisayardan açılınca çalışır.",
        )
        Divider()
        SwitchRow(
            "El feneri, titreşim, pil ve konum",
            AgentState.shareDevice,
            actions.onShareDevice,
            detail = "Konum için konum izni istenir.",
        )
    }
}

@Composable
fun AboutPanel(version: String, system: SystemActions) {
    PanelCard {
        Text("Mause $version", color = MauseColors.Ink, fontWeight = FontWeight.SemiBold, fontSize = 16.sp)
        Note("Bilgisayarınızdaki fare ve klavyeyi bu cihazda kullanın. Dosya, ses, ekran ve donanım paylaşımı siz açana kadar kapalıdır.")
        Note("Geliştirenler: Muhammed Taymur ve Serdar Kayışkıran")
        SecondaryButton("Uygulama bilgisi", system.onAppInfo)
    }
}
