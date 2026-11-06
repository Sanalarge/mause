package io.github.bytaymur.mause

import android.annotation.SuppressLint
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.content.pm.PackageManager
import android.graphics.ImageFormat
import android.graphics.Rect
import android.graphics.YuvImage
import android.hardware.camera2.CameraCaptureSession
import android.hardware.camera2.CameraCharacteristics
import android.hardware.camera2.CameraDevice
import android.hardware.camera2.CameraManager
import android.hardware.camera2.CaptureRequest
import android.location.Location
import android.location.LocationManager
import android.media.AudioAttributes
import android.media.Image
import android.media.ImageReader
import android.os.BatteryManager
import android.os.Build
import android.os.Handler
import android.os.HandlerThread
import android.os.SystemClock
import android.os.VibrationEffect
import android.os.Vibrator
import android.os.VibratorManager
import android.util.Size
import java.io.ByteArrayOutputStream
import java.util.Locale
import kotlin.concurrent.thread

class DeviceShare(private val context: Context, private val bridge: NativeBridge) {
    private val manager = context.getSystemService(CameraManager::class.java)
    private val worker = HandlerThread("mause-camera").apply { start() }
    private val handler = Handler(worker.looper)
    private var camera: CameraDevice? = null
    private var session: CameraCaptureSession? = null
    private var reader: ImageReader? = null
    private var lastFrame = 0L

    @Volatile
    private var generation = 0

    fun handle(op: Int, data: ByteArray) {
        when (op) {
            LIST -> announce()
            CAMERA_START -> handler.post { startCamera(String(data)) }
            CAMERA_STOP -> handler.post { stopCamera() }
            TORCH -> if (AgentState.shareDevice) {
                torch(data.firstOrNull()?.toInt() == 1)
            } else {
                AgentState.note("El feneri isteği reddedildi: donanım paylaşımı kapalı")
            }
            VIBRATE -> if (AgentState.shareDevice) {
                vibrate()
            } else {
                AgentState.note("Titreşim isteği reddedildi: donanım paylaşımı kapalı")
            }
        }
    }

    fun announce() {
        bridge.deviceReply(LIST, capabilities().toByteArray())
    }

    fun connected(on: Boolean) {
        val mine = ++generation
        if (!on) return
        thread(name = "mause-status", isDaemon = true) {
            while (generation == mine) {
                if (AgentState.shareDevice) bridge.deviceReply(STATUS, status().toByteArray())
                Thread.sleep(STATUS_MS)
            }
        }
    }

    fun release() {
        generation++
        handler.post {
            stopCamera()
            worker.quitSafely()
        }
    }

    private fun capabilities(): String {
        val lines = mutableListOf<String>()
        if (AgentState.shareCamera && AgentState.cameraReady) {
            for (id in manager.cameraIdList) {
                val characteristics = manager.getCameraCharacteristics(id)
                val label = when (characteristics.get(CameraCharacteristics.LENS_FACING)) {
                    CameraCharacteristics.LENS_FACING_FRONT -> "Ön kamera"
                    CameraCharacteristics.LENS_FACING_BACK -> "Arka kamera"
                    else -> "Harici kamera"
                }
                val rotation = characteristics.get(CameraCharacteristics.SENSOR_ORIENTATION) ?: 0
                lines += "camera|$id|$label ($id)|$rotation"
            }
        }
        if (AgentState.shareDevice) {
            if (torchId() != null) lines += "torch||El feneri|0"
            if (vibrator()?.hasVibrator() == true) lines += "vibrate||Titret|0"
        }
        return lines.joinToString("\n")
    }

    private fun status(): String {
        val battery = context.registerReceiver(null, IntentFilter(Intent.ACTION_BATTERY_CHANGED))
        val level = battery?.getIntExtra(BatteryManager.EXTRA_LEVEL, -1) ?: -1
        val scale = battery?.getIntExtra(BatteryManager.EXTRA_SCALE, 100) ?: 100
        val plugged = (battery?.getIntExtra(BatteryManager.EXTRA_PLUGGED, 0) ?: 0) != 0
        val parts = mutableListOf<String>()
        if (level >= 0 && scale > 0) parts += "battery=${level * 100 / scale}"
        parts += "charging=${if (plugged) 1 else 0}"
        location()?.let {
            parts += "lat=${String.format(Locale.US, "%.5f", it.latitude)}"
            parts += "lon=${String.format(Locale.US, "%.5f", it.longitude)}"
        }
        return parts.joinToString(";")
    }

    @SuppressLint("MissingPermission")
    private fun location(): Location? {
        if (!AgentState.locationReady) return null
        val locations = context.getSystemService(LocationManager::class.java)
        return listOf(LocationManager.GPS_PROVIDER, LocationManager.NETWORK_PROVIDER)
            .mapNotNull { runCatching { locations.getLastKnownLocation(it) }.getOrNull() }
            .maxByOrNull { it.time }
    }

    private fun torchId(): String? = manager.cameraIdList.firstOrNull { id ->
        val characteristics = manager.getCameraCharacteristics(id)
        characteristics.get(CameraCharacteristics.FLASH_INFO_AVAILABLE) == true &&
            characteristics.get(CameraCharacteristics.LENS_FACING) == CameraCharacteristics.LENS_FACING_BACK
    }

    private fun torch(on: Boolean) {
        val id = torchId() ?: return
        runCatching { manager.setTorchMode(id, on) }
            .onFailure { AgentState.note("El feneri açılamadı: ${it.message}") }
    }

    private fun vibrator(): Vibrator? =
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            context.getSystemService(VibratorManager::class.java)?.defaultVibrator
        } else {
            @Suppress("DEPRECATION")
            context.getSystemService(Vibrator::class.java)
        }

    private fun vibrate() {
        val device = vibrator()
        if (device == null || !device.hasVibrator()) {
            AgentState.note("Titreşim motoru bulunamadı")
            return
        }
        val alarm = AudioAttributes.Builder()
            .setUsage(AudioAttributes.USAGE_ALARM)
            .setContentType(AudioAttributes.CONTENT_TYPE_SONIFICATION)
            .build()
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val pattern = longArrayOf(0, 500, 250, 500, 250, 500)
            @Suppress("DEPRECATION")
            device.vibrate(VibrationEffect.createWaveform(pattern, -1), alarm)
        } else {
            @Suppress("DEPRECATION")
            device.vibrate(VIBRATE_MS, alarm)
        }
    }

    @SuppressLint("MissingPermission")
    private fun startCamera(id: String) {
        stopCamera()
        if (!AgentState.shareCamera || !AgentState.cameraReady) {
            AgentState.note("Kamera isteği reddedildi: paylaşım kapalı ya da kamera izni yok")
            return
        }
        if (!bridge.cameraOpen()) {
            AgentState.note("Kamera bağlantısı bilgisayara kurulamadı")
            return
        }
        try {
            manager.openCamera(
                id,
                object : CameraDevice.StateCallback() {
                    override fun onOpened(device: CameraDevice) {
                        camera = device
                        begin(device, id)
                    }

                    override fun onDisconnected(device: CameraDevice) {
                        device.close()
                        stopCamera()
                    }

                    override fun onError(device: CameraDevice, error: Int) {
                        device.close()
                        stopCamera()
                    }
                },
                handler,
            )
        } catch (error: Exception) {
            AgentState.note("Kamera açılamadı: ${error.message}")
            bridge.cameraClose()
        }
    }

    @Suppress("DEPRECATION")
    private fun begin(device: CameraDevice, id: String) {
        val map = manager.getCameraCharacteristics(id)
            .get(CameraCharacteristics.SCALER_STREAM_CONFIGURATION_MAP)
        val size = map?.getOutputSizes(ImageFormat.YUV_420_888)
            ?.filter { it.width * it.height <= MAX_PIXELS }
            ?.maxByOrNull { it.width * it.height }
            ?: Size(640, 480)
        val frames = ImageReader.newInstance(size.width, size.height, ImageFormat.YUV_420_888, 2)
        frames.setOnImageAvailableListener({ onImage(it) }, handler)
        reader = frames
        device.createCaptureSession(
            listOf(frames.surface),
            object : CameraCaptureSession.StateCallback() {
                override fun onConfigured(configured: CameraCaptureSession) {
                    session = configured
                    val request = device.createCaptureRequest(CameraDevice.TEMPLATE_PREVIEW).apply {
                        addTarget(frames.surface)
                        set(CaptureRequest.CONTROL_AF_MODE, CaptureRequest.CONTROL_AF_MODE_CONTINUOUS_VIDEO)
                    }
                    configured.setRepeatingRequest(request.build(), null, handler)
                }

                override fun onConfigureFailed(failed: CameraCaptureSession) {
                    AgentState.note("Kamera akışı kurulamadı")
                    stopCamera()
                }
            },
            handler,
        )
    }

    private fun onImage(source: ImageReader) {
        val image = source.acquireLatestImage() ?: return
        var lost = false
        try {
            val now = SystemClock.uptimeMillis()
            if (now - lastFrame < FRAME_MS) return
            lastFrame = now
            val jpeg = encode(image)
            lost = !bridge.cameraFrame(jpeg, jpeg.size, image.width, image.height)
        } finally {
            image.close()
        }
        if (lost) handler.post { stopCamera() }
    }

    private fun encode(image: Image): ByteArray {
        val width = image.width
        val height = image.height
        val nv21 = ByteArray(width * height * 3 / 2)
        val luma = image.planes[0]
        var out = 0
        for (row in 0 until height) {
            luma.buffer.position(row * luma.rowStride)
            luma.buffer.get(nv21, out, width)
            out += width
        }
        val blue = image.planes[1]
        val red = image.planes[2]
        for (row in 0 until height / 2) {
            for (column in 0 until width / 2) {
                nv21[out++] = red.buffer.get(row * red.rowStride + column * red.pixelStride)
                nv21[out++] = blue.buffer.get(row * blue.rowStride + column * blue.pixelStride)
            }
        }
        val stream = ByteArrayOutputStream(64 * 1024)
        YuvImage(nv21, ImageFormat.NV21, width, height, null)
            .compressToJpeg(Rect(0, 0, width, height), QUALITY, stream)
        return stream.toByteArray()
    }

    private fun stopCamera() {
        session?.close()
        session = null
        camera?.close()
        camera = null
        reader?.close()
        reader = null
        bridge.cameraClose()
    }

    companion object {
        private const val LIST = 0
        private const val CAMERA_START = 1
        private const val CAMERA_STOP = 2
        private const val TORCH = 3
        private const val VIBRATE = 4
        private const val STATUS = 5
        private const val MAX_PIXELS = 1280 * 720
        private const val FRAME_MS = 66L
        private const val QUALITY = 70
        private const val STATUS_MS = 5000L
        private const val VIBRATE_MS = 700L
    }
}

fun hasPermission(context: Context, permission: String): Boolean =
    context.checkSelfPermission(permission) == PackageManager.PERMISSION_GRANTED
