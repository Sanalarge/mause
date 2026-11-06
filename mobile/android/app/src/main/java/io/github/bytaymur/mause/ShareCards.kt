package io.github.bytaymur.mause

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

class ShareActions(
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

private val Ready = Color(0xFF9BE000)

@Composable
private fun ShareCard(title: String, ready: Boolean, content: @Composable () -> Unit) {
    Card(
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
        shape = RoundedCornerShape(18.dp),
        modifier = Modifier.fillMaxWidth(),
    ) {
        Column(Modifier.padding(18.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                Text(title, modifier = Modifier.weight(1f), fontWeight = FontWeight.SemiBold)
                Text(
                    if (ready) "Açık" else "Kapalı",
                    color = if (ready) Ready else MaterialTheme.colorScheme.onSurfaceVariant,
                    fontSize = 13.sp,
                )
            }
            content()
        }
    }
}

@Composable
private fun Note(text: String) {
    Text(
        text,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        fontSize = 13.sp,
        lineHeight = 18.sp,
    )
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
            Text(label, fontSize = 15.sp)
            detail?.let {
                Text(
                    it,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    fontSize = 12.sp,
                    lineHeight = 17.sp,
                )
            }
        }
        Switch(checked = checked, onCheckedChange = onChange)
    }
}

@Composable
fun RemoteCard(actions: ShareActions) {
    ShareCard("Bilgisayarla ortak ayar", AgentState.remotePrefs) {
        SwitchRow(
            "Bilgisayar paylaşım ayarlarımı değiştirebilsin",
            AgentState.remotePrefs,
            actions.onRemotePrefs,
            detail = "Bilgisayarda bir akış tuşuna basınca burada da aynı akış açılır ya da kapanır. Kapalıysa bilgisayar yalnız kapatabilir.",
        )
    }
}

@Composable
fun FilesCard(actions: ShareActions) {
    ShareCard("Dosyalar", AgentState.filesAllowed) {
        Note("Bilgisayar bu cihazın dosyalarını görüp indirebilir ve dosya yükleyebilir. Bilgisayardan gelen dosyalar İndirilenler/Mause klasörüne kaydedilir.")
        SwitchRow(
            "Bilgisayarın dosyalara erişmesine izin ver",
            AgentState.filesAllowed,
            actions.onFiles,
        )
        if (AgentState.filesAllowed && !AgentState.storageReady) {
            Note("Tüm dosyalara erişim izni olmadan yalnızca Mause'un kendi klasörü paylaşılır.")
            OutlinedButton(onClick = actions.onStorage) { Text("Tüm dosyalara erişim izni") }
        }
        OutlinedButton(onClick = actions.onSendFiles, enabled = AgentState.connected) {
            Text("Bilgisayara dosya gönder")
        }
    }
}

@Composable
fun AudioCard(actions: ShareActions) {
    ShareCard("Ses", AgentState.shareAudio || AgentState.shareMic || AgentState.playPcAudio) {
        SwitchRow(
            "Bu cihazın sesini bilgisayara gönder",
            AgentState.shareAudio,
            actions.onShareAudio,
            detail = "Ekran yakalama izni ve mikrofon izni gerekir.",
        )
        if (AgentState.shareAudio && !AgentState.captureReady) {
            Note("Cihaz sesi için aşağıdaki Ekran kartından yakalama iznini hazırlayın.")
        }
        SwitchRow(
            "Mikrofonu bilgisayara gönder",
            AgentState.shareMic,
            actions.onShareMic,
        )
        SwitchRow(
            "Bilgisayarın sesini bu cihazda çal",
            AgentState.playPcAudio,
            actions.onPlayAudio,
        )
    }
}

@Composable
fun ScreenCard(actions: ShareActions) {
    ShareCard("Ekran", AgentState.shareScreen && AgentState.captureReady) {
        Note("Bilgisayardan bu cihazın ekranını izleyin. Erişilebilirlik servisi açıksa bilgisayardan dokunma ve klavye de çalışır.")
        if (!AgentState.captureReady) {
            Note("Android ekran yakalama için onay ister. Onay, Mause durdurulunca sıfırlanır.")
            Button(
                onClick = actions.onPrepareCapture,
                enabled = AgentState.running,
                colors = ButtonDefaults.buttonColors(
                    containerColor = MaterialTheme.colorScheme.primary,
                    contentColor = Color.Black,
                ),
            ) { Text("Ekran yakalamayı hazırla") }
        }
        SwitchRow(
            "Bilgisayarın ekranı izlemesine izin ver",
            AgentState.shareScreen,
            actions.onShareScreen,
        )
        if (AgentState.screenLive) {
            Text("Ekran şu an bilgisayara aktarılıyor", color = Ready, fontSize = 13.sp)
        }
    }
}

@Composable
fun HardwareCard(actions: ShareActions) {
    ShareCard("Donanım", AgentState.shareCamera || AgentState.shareDevice) {
        Note("Bilgisayar bu cihazın kameralarını canlı izleyebilir, el fenerini açıp titreşimi çalıştırabilir, pil ve konum bilgisini görebilir. Bilgisayarda cihazın monitör kartına tıklayınca menüde çıkar.")
        SwitchRow(
            "Kameraları bilgisayarla paylaş",
            AgentState.shareCamera,
            actions.onShareCamera,
            detail = "Kamera izni gerekir. Kamera yalnız bilgisayardan açılınca çalışır.",
        )
        SwitchRow(
            "El feneri, titreşim, pil ve konum",
            AgentState.shareDevice,
            actions.onShareDevice,
            detail = "Konum için konum izni istenir.",
        )
    }
}

@Composable
fun ComputerScreenCard(actions: ShareActions) {
    ShareCard("Bilgisayarın ekranı", AgentState.viewFrame != null) {
        Note("Bağlı bilgisayarın ekranını bu cihazda akıcı video olarak izleyin ve dokunarak kullanın: dokunma sol tık, uzun basma sağ tık, sürükleme basılı tutarak sürükleme.")
        Note("Bilgisayarda bu cihazın altındaki üst satırdan Ekran simgesi açık olmalı. İkinci monitör için bilgisayar Linux Wayland (KDE ya da GNOME) olmalı; ilk kullanımda masaüstü bir paylaşım onayı sorar.")
        Button(
            onClick = { actions.onViewScreen(false) },
            enabled = AgentState.connected,
            colors = ButtonDefaults.buttonColors(
                containerColor = MaterialTheme.colorScheme.primary,
                contentColor = Color.Black,
            ),
        ) { Text("Ekranı yansıt") }
        OutlinedButton(
            onClick = { actions.onViewScreen(true) },
            enabled = AgentState.connected,
        ) { Text("İkinci monitör olarak kullan") }
    }
}

@Composable
fun ActivityCard() {
    if (AgentState.activity.isEmpty()) return
    Card(
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
        shape = RoundedCornerShape(18.dp),
        modifier = Modifier.fillMaxWidth(),
    ) {
        Column(Modifier.padding(18.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            Text("Son işlemler", fontWeight = FontWeight.SemiBold)
            AgentState.activity.forEach { Note(it) }
        }
    }
}
