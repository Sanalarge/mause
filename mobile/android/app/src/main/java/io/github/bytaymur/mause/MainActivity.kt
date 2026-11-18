package io.github.bytaymur.mause

import android.Manifest
import android.accessibilityservice.AccessibilityServiceInfo
import android.content.ClipboardManager
import android.content.Intent
import android.content.pm.PackageManager
import android.media.projection.MediaProjectionManager
import android.os.Build
import android.net.Uri
import android.os.Bundle
import android.os.Environment
import android.provider.Settings
import android.view.accessibility.AccessibilityManager
import android.view.inputmethod.InputMethodManager
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color as ComposeColor
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import java.util.concurrent.Executors

class MainActivity : ComponentActivity() {
    private val notificationPermission = registerForActivityResult(
        ActivityResultContracts.RequestPermission(),
    ) {}

    private val microphonePermission = registerForActivityResult(
        ActivityResultContracts.RequestPermission(),
    ) { granted ->
        AgentState.micReady = granted
        AgentService.permissionsChanged()
    }

    private val cameraPermission = registerForActivityResult(
        ActivityResultContracts.RequestPermission(),
    ) { granted ->
        AgentState.cameraReady = granted
        AgentService.permissionsChanged()
    }

    private val locationPermission = registerForActivityResult(
        ActivityResultContracts.RequestPermission(),
    ) { granted ->
        AgentState.locationReady = granted
        AgentService.permissionsChanged()
    }

    private val storagePermission = registerForActivityResult(
        ActivityResultContracts.RequestPermission(),
    ) {
        refreshCapabilities()
        AgentService.sharingChanged()
    }

    private val captureConsent = registerForActivityResult(
        ActivityResultContracts.StartActivityForResult(),
    ) { result ->
        val data = result.data
        if (result.resultCode == RESULT_OK && data != null) {
            AgentService.grantCapture(result.resultCode, data)
        }
    }

    private val filePicker = registerForActivityResult(
        ActivityResultContracts.GetMultipleContents(),
    ) { uris ->
        if (uris.isNotEmpty()) AgentService.sendFiles(uris)
    }

    private val clipboardImagePicker = registerForActivityResult(
        ActivityResultContracts.GetContent(),
    ) { uri ->
        if (uri != null) AgentService.current?.sendClipboardImage(uri)
    }

    private val clipboardPreviewWorker = Executors.newSingleThreadExecutor()
    private var previewRevision = 0L
    private val clipboardListener = ClipboardManager.OnPrimaryClipChangedListener {
        if (hasWindowFocus()) refreshClipboardImage()
    }

    private fun clipboardImageUri(): Uri? =
        getSystemService(ClipboardManager::class.java).primaryClip
            ?.takeIf { it.description.hasMimeType("image/*") && it.itemCount > 0 }
            ?.getItemAt(0)?.uri

    private fun refreshClipboardImage() {
        val revision = ++previewRevision
        val uri = clipboardImageUri()
        if (AgentState.clipboardImage?.uri == uri) return
        AgentState.clipboardImage = null
        if (uri != null) {
            clipboardPreviewWorker.execute {
                val preview = runCatching { ClipboardImages.preview(this, uri) }.getOrNull()
                runOnUiThread {
                    if (revision == previewRevision) AgentState.clipboardImage = preview
                }
            }
        }
    }

    private fun sendClipboardImage() {
        val image = clipboardImageUri()
        if (image == null) {
            AgentState.note("Panoda görsel yok. Bir ekran görüntüsü seçebilirsiniz.")
        } else {
            AgentService.current?.sendClipboardImage(image)
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        KeyboardInput.load(this)
        SharePrefs.load(this)
        refreshCapabilities()
        setContent {
            MauseTheme {
                MauseScreen(
                    onStart = ::startAgent,
                    onStop = ::stopAgent,
                    onAccessibility = {
                        startActivity(Intent(Settings.ACTION_ACCESSIBILITY_SETTINGS))
                    },
                    onKeyboard = {
                        startActivity(Intent(Settings.ACTION_INPUT_METHOD_SETTINGS))
                    },
                    onAppInfo = {
                        startActivity(
                            Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS)
                                .setData(Uri.fromParts("package", packageName, null)),
                        )
                    },
                    onPairAnswer = AgentService::answerPair,
                    onLayout = { KeyboardInput.choose(this, it) },
                    share = shareActions(),
                )
            }
        }
    }

    override fun onStart() {
        super.onStart()
        getSystemService(ClipboardManager::class.java)
            .addPrimaryClipChangedListener(clipboardListener)
    }

    override fun onWindowFocusChanged(hasFocus: Boolean) {
        super.onWindowFocusChanged(hasFocus)
        if (hasFocus) refreshClipboardImage()
    }

    override fun onStop() {
        getSystemService(ClipboardManager::class.java)
            .removePrimaryClipChangedListener(clipboardListener)
        previewRevision++
        super.onStop()
    }

    override fun onDestroy() {
        clipboardPreviewWorker.shutdownNow()
        super.onDestroy()
    }

    override fun onResume() {
        super.onResume()
        refreshCapabilities()
        AgentService.sharingChanged()
    }

    private fun shareActions() = ShareActions(
        onClipboard = { change { AgentState.clipboardAllowed = it } },
        onSendClipboard = ::sendClipboardImage,
        onPickClipboardImage = { clipboardImagePicker.launch("image/*") },
        onFiles = { change { AgentState.filesAllowed = it } },
        onStorage = ::requestStorage,
        onSendFiles = { filePicker.launch("*/*") },
        onShareAudio = { on ->
            change { AgentState.shareAudio = on }
            if (on) requireMicrophone()
        },
        onShareMic = { on ->
            change { AgentState.shareMic = on }
            if (on) requireMicrophone()
        },
        onPlayAudio = { change { AgentState.playPcAudio = it } },
        onShareScreen = { change { AgentState.shareScreen = it } },
        onRemotePrefs = { change { AgentState.remotePrefs = it } },
        onShareCamera = { on ->
            change { AgentState.shareCamera = on }
            if (on && !AgentState.cameraReady) cameraPermission.launch(Manifest.permission.CAMERA)
        },
        onShareDevice = { on ->
            change { AgentState.shareDevice = on }
            if (on && !AgentState.locationReady) {
                locationPermission.launch(Manifest.permission.ACCESS_FINE_LOCATION)
            }
        },
        onViewScreen = { extend ->
            startActivity(Intent(this, ViewActivity::class.java).putExtra(ViewActivity.EXTEND, extend))
        },
        onPrepareCapture = {
            val manager = getSystemService(MediaProjectionManager::class.java)
            captureConsent.launch(manager.createScreenCaptureIntent())
        },
    )

    private fun change(update: () -> Unit) {
        update()
        SharePrefs.save(this)
        AgentService.sharingChanged()
    }

    private fun requireMicrophone() {
        if (!AgentState.micReady) microphonePermission.launch(Manifest.permission.RECORD_AUDIO)
    }

    private fun requestStorage() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            startActivity(
                Intent(
                    Settings.ACTION_MANAGE_APP_ALL_FILES_ACCESS_PERMISSION,
                    Uri.parse("package:$packageName"),
                ),
            )
        } else {
            storagePermission.launch(Manifest.permission.WRITE_EXTERNAL_STORAGE)
        }
    }

    private fun startAgent() {
        if (Build.VERSION.SDK_INT >= 33 && checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED) {
            notificationPermission.launch(Manifest.permission.POST_NOTIFICATIONS)
        }
        val intent = Intent(this, AgentService::class.java)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            startForegroundService(intent)
        } else {
            startService(intent)
        }
    }

    private fun stopAgent() {
        stopService(Intent(this, AgentService::class.java))
    }

    private fun refreshCapabilities() {
        val services = getSystemService(AccessibilityManager::class.java)
            .getEnabledAccessibilityServiceList(AccessibilityServiceInfo.FEEDBACK_ALL_MASK)
        AgentState.accessibilityReady = services.any {
            it.resolveInfo.serviceInfo.packageName == packageName
        }
        val keyboards = getSystemService(InputMethodManager::class.java).enabledInputMethodList
        AgentState.keyboardReady = keyboards.any { it.packageName == packageName }
        AgentState.micReady =
            checkSelfPermission(Manifest.permission.RECORD_AUDIO) == PackageManager.PERMISSION_GRANTED
        AgentState.cameraReady = hasPermission(this, Manifest.permission.CAMERA)
        AgentState.locationReady = hasPermission(this, Manifest.permission.ACCESS_FINE_LOCATION)
        AgentState.storageReady = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            Environment.isExternalStorageManager()
        } else {
            checkSelfPermission(Manifest.permission.WRITE_EXTERNAL_STORAGE) ==
                PackageManager.PERMISSION_GRANTED
        }
    }
}

@Composable
private fun MauseScreen(
    onStart: () -> Unit,
    onStop: () -> Unit,
    onAccessibility: () -> Unit,
    onKeyboard: () -> Unit,
    onAppInfo: () -> Unit,
    onPairAnswer: (Boolean) -> Unit,
    onLayout: (KeyLayout) -> Unit,
    share: ShareActions,
) {
    Surface(modifier = Modifier.fillMaxSize(), color = MaterialTheme.colorScheme.background) {
        Column(
            modifier = Modifier
                .verticalScroll(rememberScrollState())
                .padding(horizontal = 24.dp, vertical = 28.dp),
            verticalArrangement = Arrangement.spacedBy(18.dp),
        ) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Image(
                    painter = painterResource(R.drawable.ic_mause),
                    contentDescription = null,
                    modifier = Modifier.size(42.dp),
                )
                Text(
                    "mause",
                    modifier = Modifier.padding(start = 12.dp),
                    color = MaterialTheme.colorScheme.onBackground,
                    fontWeight = FontWeight.Bold,
                    fontSize = 23.sp,
                )
            }

            Text("Android cihaz", fontSize = 28.sp, fontWeight = FontWeight.Bold)
            Text(
                "Bilgisayarınızdaki fare ve klavyeyi bu cihazda kullanın. Dosya, ses ve ekran paylaşımı siz açana kadar kapalıdır.",
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                lineHeight = 22.sp,
            )

            StatusCard()

            CapabilityCard(
                title = "Fare ve dokunma",
                description = "İmleç, tıklama ve kaydırma için erişilebilirlik servisi gerekir.",
                ready = AgentState.accessibilityReady,
                action = "İzin ver",
                onClick = onAccessibility,
                note = "İzin \"kısıtlı ayar\" diye engellenirse: Uygulama bilgisi → sağ üstteki üç nokta → Kısıtlanmış ayarlara izin ver. Sonra İzin ver'e yeniden basın.",
                secondAction = "Uygulama bilgisi",
                onSecondClick = onAppInfo,
            )
            KeyboardCard(onKeyboard = onKeyboard, onLayout = onLayout)
            RemoteCard(share)
            ClipboardCard(share)
            FilesCard(share)
            HardwareCard(share)
            AudioCard(share)
            ScreenCard(share)
            ComputerScreenCard(share)
            ActivityCard()

            Spacer(Modifier.height(4.dp))
            if (AgentState.running) {
                OutlinedButton(onClick = onStop, modifier = Modifier.fillMaxWidth()) {
                    Text("Bağlantıyı durdur")
                }
            } else {
                Button(
                    onClick = onStart,
                    enabled = AgentState.accessibilityReady,
                    modifier = Modifier.fillMaxWidth(),
                    colors = ButtonDefaults.buttonColors(
                        containerColor = MaterialTheme.colorScheme.primary,
                        contentColor = ComposeColor.Black,
                    ),
                ) {
                    Text("Bu cihazı kullanıma aç")
                }
            }
        }
    }

    AgentState.pairRequest?.let { request ->
        AlertDialog(
            onDismissRequest = {},
            title = { Text("Bilgisayarı eşleştir") },
            text = {
                Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                    Text("${request.computer} bu cihazı kontrol etmek istiyor.")
                    Text(request.code, fontSize = 32.sp, fontWeight = FontWeight.Bold)
                    Text("Aynı kod bilgisayarda görünüyorsa onaylayın.")
                }
            },
            confirmButton = {
                Button(onClick = { onPairAnswer(true) }) { Text("Eşleştir") }
            },
            dismissButton = {
                OutlinedButton(onClick = { onPairAnswer(false) }) { Text("Reddet") }
            },
        )
    }
}

@Composable
private fun StatusCard() {
    val tone = when {
        AgentState.connected -> ComposeColor(0xFF9BE000)
        AgentState.running -> ComposeColor(0xFFFF9800)
        else -> MaterialTheme.colorScheme.onSurfaceVariant
    }
    Card(
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
        shape = RoundedCornerShape(18.dp),
        modifier = Modifier.fillMaxWidth(),
    ) {
        Row(
            modifier = Modifier.padding(18.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Box(Modifier.size(10.dp).background(tone, RoundedCornerShape(5.dp)))
            Column(Modifier.padding(start = 14.dp)) {
                Text(if (AgentState.connected) "Bağlı" else "Bağlantı durumu", fontWeight = FontWeight.SemiBold)
                Text(AgentState.status, color = MaterialTheme.colorScheme.onSurfaceVariant, fontSize = 13.sp)
            }
        }
    }
}

@Composable
private fun CapabilityCard(
    title: String,
    description: String,
    ready: Boolean,
    action: String,
    onClick: () -> Unit,
    note: String? = null,
    secondAction: String? = null,
    onSecondClick: () -> Unit = {},
) {
    Card(
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
        shape = RoundedCornerShape(18.dp),
        modifier = Modifier.fillMaxWidth(),
    ) {
        Column(Modifier.padding(18.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                Text(title, modifier = Modifier.weight(1f), fontWeight = FontWeight.SemiBold)
                Text(
                    if (ready) "Hazır" else "Gerekli",
                    color = if (ready) ComposeColor(0xFF9BE000) else MaterialTheme.colorScheme.primary,
                    fontSize = 13.sp,
                )
            }
            Text(description, color = MaterialTheme.colorScheme.onSurfaceVariant, fontSize = 13.sp)
            if (!ready) {
                note?.let {
                    Text(it, color = MaterialTheme.colorScheme.onSurfaceVariant, fontSize = 12.sp, lineHeight = 17.sp)
                }
                Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                    OutlinedButton(onClick = onClick) { Text(action) }
                    secondAction?.let {
                        OutlinedButton(onClick = onSecondClick) { Text(it) }
                    }
                }
            }
        }
    }
}

@Composable
private fun KeyboardCard(onKeyboard: () -> Unit, onLayout: (KeyLayout) -> Unit) {
    Card(
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
        shape = RoundedCornerShape(18.dp),
        modifier = Modifier.fillMaxWidth(),
    ) {
        Column(Modifier.padding(18.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                Text("Klavye", modifier = Modifier.weight(1f), fontWeight = FontWeight.SemiBold)
                Text(
                    if (AgentState.accessibilityReady) "Hazır" else "Fare izni gerekli",
                    color = if (AgentState.accessibilityReady) ComposeColor(0xFF9BE000) else MaterialTheme.colorScheme.primary,
                    fontSize = 13.sp,
                )
            }
            Text(
                "Bilgisayar klavyesiyle odaklanan metin kutusuna yazılır. Bilgisayarınızın klavye düzenini seçin.",
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                fontSize = 13.sp,
            )
            Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                KeyLayout.entries.forEach { option ->
                    val selected = AgentState.keyboardLayout == option
                    if (selected) {
                        Button(
                            onClick = { onLayout(option) },
                            colors = ButtonDefaults.buttonColors(
                                containerColor = MaterialTheme.colorScheme.primary,
                                contentColor = ComposeColor.Black,
                            ),
                        ) { Text(option.title) }
                    } else {
                        OutlinedButton(onClick = { onLayout(option) }) { Text(option.title) }
                    }
                }
            }
            Text(
                if (AgentState.keyboardReady) {
                    "Mause Klavye etkin: yukarı ve aşağı ok tuşları da çalışır."
                } else {
                    "İsteğe bağlı: yukarı ve aşağı ok tuşları için Mause Klavye'yi etkinleştirip klavye olarak seçin."
                },
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                fontSize = 12.sp,
                lineHeight = 17.sp,
            )
            if (!AgentState.keyboardReady) {
                OutlinedButton(onClick = onKeyboard) { Text("Mause Klavye") }
            }
        }
    }
}

@Composable
private fun MauseTheme(content: @Composable () -> Unit) {
    MaterialTheme(
        colorScheme = darkColorScheme(
            primary = ComposeColor(0xFFFF9800),
            background = ComposeColor(0xFF0F0F12),
            surface = ComposeColor(0xFF1C1C20),
            onBackground = ComposeColor(0xFFF4F2F6),
            onSurface = ComposeColor(0xFFF4F2F6),
            onSurfaceVariant = ComposeColor(0xFFA9A5AF),
        ),
        content = content,
    )
}
