package io.github.bytaymur.mause

import android.Manifest
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.Service
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.content.pm.ServiceInfo
import android.graphics.BitmapFactory
import android.net.Uri
import android.net.wifi.WifiManager
import android.os.Build
import android.os.Environment
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.provider.OpenableColumns
import androidx.core.app.NotificationCompat
import java.io.File
import java.util.UUID
import java.util.concurrent.Executors
import kotlin.concurrent.thread

class AgentService : Service(), NativeBridge.Callbacks {
    private val main = Handler(Looper.getMainLooper())
    private val worker = Executors.newSingleThreadExecutor()
    private lateinit var bridge: NativeBridge
    private lateinit var screen: ScreenShare
    private lateinit var audio: AudioShare
    private lateinit var device: DeviceShare
    private var wifiLock: WifiManager.WifiLock? = null
    private var playing = false

    override fun onCreate() {
        super.onCreate()
        createChannel()
        SharePrefs.load(this)
        refreshPermissions()
        enterForeground()
        bridge = NativeBridge(this)
        screen = ScreenShare(this, bridge)
        audio = AudioShare(bridge)
        device = DeviceShare(this, bridge)
        wifiLock = lowLatencyWifi()
        val prefs = getSharedPreferences("mause", MODE_PRIVATE)
        val id = prefs.getString("device_id", null)?.takeIf { it.startsWith(ID_PREFIX) }
            ?: (ID_PREFIX + UUID.randomUUID().toString().replace("-", ""))
                .also { prefs.edit().putString("device_id", it).apply() }
        val metrics = resources.displayMetrics
        val started = bridge.start(
            Build.MODEL.ifBlank { "Android" },
            id,
            24800,
            metrics.widthPixels,
            metrics.heightPixels,
            filesDir.absolutePath,
        )
        AgentState.running = started
        AgentState.status = if (started) "Ağdaki bilgisayarlar bekleniyor" else "Servis başlatılamadı"
        if (started) pushShare()
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (intent?.action == ACTION_STOP) {
            stopSelf()
        }
        return START_STICKY
    }

    override fun onDestroy() {
        wifiLock?.takeIf { it.isHeld }?.release()
        screen.release()
        audio.release()
        device.release()
        Capture.release()
        bridge.stop()
        worker.shutdown()
        if (current === this) current = null
        AgentState.running = false
        AgentState.connected = false
        AgentState.screenLive = false
        AgentState.status = "Durduruldu"
        super.onDestroy()
    }

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStatus(status: String) {
        main.post { AgentState.status = status }
    }

    override fun onPairRequest(computer: String, code: String) {
        main.post { AgentState.pairRequest = PairRequest(computer, code) }
    }

    override fun onConnected(connected: Boolean) {
        main.post {
            AgentState.connected = connected
            AgentState.status = if (connected) "Bilgisayara bağlı" else "Ağdaki bilgisayarlar bekleniyor"
        }
        device.connected(connected)
        worker.execute {
            if (!connected) playing = false
            if (connected) pushNow() else audio.stopPlayback()
        }
    }

    override fun onEnter(position: Int, side: Int) {
        InputController.enter(position, side)
    }

    override fun onInput(events: IntArray) {
        InputController.input(events)
    }

    override fun onRelease() {
        InputController.release()
    }

    override fun onClipboard(text: String) {
        main.post {
            val clipboard = getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
            clipboard.setPrimaryClip(ClipData.newPlainText("Mause", text))
        }
    }

    override fun onActivity(text: String) {
        note(text)
    }

    override fun onAudioRequest(kind: Int, on: Boolean) {
        audio.request(kind, on)
    }

    override fun onAudioPlay(kind: Int, pcm: ByteArray) {
        if (kind == 0) audio.play(pcm)
    }

    override fun onScreenStart(options: ByteArray) {
        screen.start(options)
    }

    override fun onScreenStop() {
        screen.stop()
    }

    override fun onScreenInput(events: IntArray) {
        if (AgentState.accessibilityReady) InputController.screenInput(events)
    }

    override fun onPrefOff(pref: Int) {
        main.post {
            when (pref) {
                1 -> AgentState.shareAudio = false
                2 -> AgentState.shareMic = false
                3 -> AgentState.filesAllowed = false
                6 -> AgentState.playPcAudio = false
            }
            SharePrefs.save(this)
        }
        if (pref == 6) worker.execute { syncPlayback() }
    }

    override fun onDevice(op: Int, data: ByteArray) {
        device.handle(op, data)
    }

    override fun onPrefOn(pref: Int) {
        if (AgentState.remotePrefs) {
            when (pref) {
                1 -> AgentState.shareAudio = true
                2 -> AgentState.shareMic = true
                3 -> AgentState.filesAllowed = true
                6 -> AgentState.playPcAudio = true
            }
            SharePrefs.save(this)
        }
        pushShare()
    }

    override fun onViewFrame(width: Int, height: Int, jpeg: ByteArray) {
        AgentState.viewWidth = width
        AgentState.viewHeight = height
        if (jpeg.isNotEmpty() && jpeg[0].toInt() == 0) {
            if (!AgentState.viewVideo) AgentState.viewVideo = true
            Viewer.decoder.feed(width, height, jpeg)
            return
        }
        AgentState.viewFrame = BitmapFactory.decodeByteArray(jpeg, 0, jpeg.size) ?: return
    }

    override fun onViewEnded(reason: String) {
        main.post {
            AgentState.viewFrame = null
            AgentState.viewNote = reason
        }
    }

    override fun onViewInfo(control: Boolean) {
        main.post { AgentState.viewControl = control }
    }

    fun watchScreen(on: Boolean, extend: Boolean) {
        worker.execute { bridge.watchScreen(on, viewOptions(extend)) }
    }

    private fun viewOptions(extend: Boolean): ByteArray {
        val options = mutableListOf<Byte>(70, 30, 0x80.toByte(), 0x07, 0, 1, 1)
        if (extend) {
            val metrics = resources.displayMetrics
            val longSide = maxOf(metrics.widthPixels, metrics.heightPixels).toFloat()
            val shortSide = minOf(metrics.widthPixels, metrics.heightPixels).toFloat()
            val width = 1920
            val height = ((width * shortSide / longSide).toInt() and 1.inv()).coerceIn(540, 1080)
            options += listOf<Byte>(
                1,
                (width and 0xFF).toByte(),
                (width shr 8).toByte(),
                (height and 0xFF).toByte(),
                (height shr 8).toByte(),
            )
        }
        return options.toByteArray()
    }

    fun sendScreenInput(events: IntArray) {
        worker.execute { bridge.sendScreenInput(events) }
    }

    fun answerPair(accept: Boolean) {
        bridge.answerPair(accept)
        AgentState.pairRequest = null
    }

    fun leave(position: Int) {
        thread(name = "mause-leave") { bridge.leave(position) }
    }

    fun pushShare() {
        worker.execute { pushNow() }
    }

    fun grantCapture(resultCode: Int, data: Intent) {
        try {
            enterForeground(withProjection = true)
            Capture.grant(this, resultCode, data)
        } catch (error: Exception) {
            note("Ekran yakalama başlatılamadı: ${error.message}")
            return
        }
        pushShare()
    }

    fun refreshPermissions() {
        AgentState.micReady =
            checkSelfPermission(Manifest.permission.RECORD_AUDIO) == PackageManager.PERMISSION_GRANTED
        AgentState.storageReady = storageGranted()
        AgentState.cameraReady = hasPermission(this, Manifest.permission.CAMERA)
        AgentState.locationReady = hasPermission(this, Manifest.permission.ACCESS_FINE_LOCATION)
    }

    fun permissionsChanged() {
        refreshPermissions()
        try {
            enterForeground(withProjection = Capture.projection != null)
        } catch (error: Exception) {
            note("Ses izni uygulanamadı: ${error.message}")
        }
        pushShare()
    }

    fun sendFiles(uris: List<Uri>) {
        if (!AgentState.connected) {
            note("Dosya göndermek için önce bir bilgisayara bağlanın")
            return
        }
        thread(name = "mause-send") {
            uris.forEach { uri ->
                runCatching { sendOne(uri) }
                    .onFailure { note("Dosya gönderilemedi: ${it.message}") }
            }
        }
    }

    private fun sendOne(uri: Uri) {
        var name = "dosya"
        var size = -1L
        contentResolver.query(
            uri,
            arrayOf(OpenableColumns.DISPLAY_NAME, OpenableColumns.SIZE),
            null,
            null,
            null,
        )?.use { cursor ->
            if (cursor.moveToFirst()) {
                cursor.getString(0)?.let { name = it }
                if (!cursor.isNull(1)) size = cursor.getLong(1)
            }
        }
        val descriptor = contentResolver.openFileDescriptor(uri, "r")
            ?: throw IllegalStateException("dosya açılamadı")
        if (size < 0) size = descriptor.statSize
        if (size < 0) {
            descriptor.close()
            throw IllegalStateException("$name boyutu belirlenemedi")
        }
        bridge.sendFile(descriptor.detachFd(), name, size)
    }

    private fun pushNow() {
        bridge.configureFiles(shareRoot().absolutePath, inbox().absolutePath)
        bridge.setPrefs(SharePrefs.bits())
        bridge.setScreenAllowed(SharePrefs.screenAllowed())
        device.announce()
        syncPlayback()
    }

    private fun syncPlayback() {
        val want = AgentState.playPcAudio && AgentState.connected
        if (want == playing) return
        playing = want
        if (want) {
            audio.startPlayback()
            bridge.requestAudio(0, true)
        } else {
            bridge.requestAudio(0, false)
            audio.stopPlayback()
        }
    }

    @Suppress("DEPRECATION")
    private fun shareRoot(): File =
        if (AgentState.storageReady) {
            Environment.getExternalStorageDirectory()
        } else {
            getExternalFilesDir(null) ?: filesDir
        }

    @Suppress("DEPRECATION")
    private fun inbox(): File =
        if (AgentState.storageReady) {
            File(Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_DOWNLOADS), "Mause")
        } else {
            File(getExternalFilesDir(null) ?: filesDir, "Mause")
        }

    private fun storageGranted(): Boolean =
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            Environment.isExternalStorageManager()
        } else {
            checkSelfPermission(Manifest.permission.WRITE_EXTERNAL_STORAGE) ==
                PackageManager.PERMISSION_GRANTED
        }

    private fun note(text: String) = AgentState.note(text)

    private fun enterForeground(withProjection: Boolean = Capture.projection != null) {
        val notification = notification()
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.Q) {
            startForeground(NOTIFICATION_ID, notification)
            return
        }
        var types = ServiceInfo.FOREGROUND_SERVICE_TYPE_CONNECTED_DEVICE
        if (withProjection) types = types or ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PROJECTION
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R && AgentState.micReady) {
            types = types or ServiceInfo.FOREGROUND_SERVICE_TYPE_MICROPHONE
        }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R && AgentState.cameraReady) {
            types = types or ServiceInfo.FOREGROUND_SERVICE_TYPE_CAMERA
        }
        startForeground(NOTIFICATION_ID, notification, types)
    }

    private fun notification(): Notification =
        NotificationCompat.Builder(this, CHANNEL_ID)
            .setSmallIcon(R.drawable.ic_mause_mono)
            .setContentTitle("Mause hazır")
            .setContentText("Bilgisayar bağlantısı bekleniyor")
            .setOngoing(true)
            .build()

    @Suppress("DEPRECATION")
    private fun lowLatencyWifi(): WifiManager.WifiLock? {
        val wifi = applicationContext.getSystemService(WIFI_SERVICE) as? WifiManager ?: return null
        val mode = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            WifiManager.WIFI_MODE_FULL_LOW_LATENCY
        } else {
            WifiManager.WIFI_MODE_FULL_HIGH_PERF
        }
        return wifi.createWifiLock(mode, "mause").apply {
            setReferenceCounted(false)
            acquire()
        }
    }

    private fun createChannel() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return
        val channel = NotificationChannel(
            CHANNEL_ID,
            "Mause bağlantısı",
            NotificationManager.IMPORTANCE_LOW,
        )
        getSystemService(NotificationManager::class.java).createNotificationChannel(channel)
    }

    companion object {
        const val ACTION_STOP = "io.github.bytaymur.mause.STOP"
        private const val CHANNEL_ID = "mause_agent"
        private const val NOTIFICATION_ID = 24800
        private const val ID_PREFIX = "z"
        var current: AgentService? = null

        fun answerPair(accept: Boolean) {
            current?.answerPair(accept)
        }

        fun leave(position: Int) {
            current?.leave(position)
        }

        fun watchScreen(on: Boolean, extend: Boolean = false) {
            current?.watchScreen(on, extend)
        }

        fun sendScreenInput(events: IntArray) {
            current?.sendScreenInput(events)
        }

        fun sharingChanged() {
            current?.pushShare()
        }

        fun grantCapture(resultCode: Int, data: Intent) {
            current?.grantCapture(resultCode, data)
        }

        fun permissionsChanged() {
            current?.permissionsChanged()
        }

        fun sendFiles(uris: List<Uri>) {
            current?.sendFiles(uris)
        }
    }

    init {
        current = this
    }
}
