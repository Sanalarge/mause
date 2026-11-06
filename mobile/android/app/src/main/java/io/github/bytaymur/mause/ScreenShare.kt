package io.github.bytaymur.mause

import android.content.Context
import android.graphics.Bitmap
import android.graphics.PixelFormat
import android.hardware.display.DisplayManager
import android.hardware.display.VirtualDisplay
import android.media.ImageReader
import android.os.Handler
import android.os.HandlerThread
import android.os.SystemClock
import java.io.ByteArrayOutputStream

class ScreenShare(private val context: Context, private val bridge: NativeBridge) {
    private class Options(val quality: Int, val fps: Int, val maxWidth: Int)

    private val worker = HandlerThread("mause-screen").apply { start() }
    private val handler = Handler(worker.looper)
    private var display: VirtualDisplay? = null
    private var reader: ImageReader? = null
    private var canvas: Bitmap? = null
    private var options = Options(70, 15, 1920)
    private var lastFrame = 0L

    init {
        Capture.onStopped { handler.post { dropDisplay() } }
    }

    fun start(raw: ByteArray) {
        handler.post { begin(parse(raw)) }
    }

    fun stop() {
        handler.post { pause() }
    }

    fun release() {
        handler.post {
            dropDisplay()
            worker.quitSafely()
        }
    }

    private fun parse(raw: ByteArray): Options {
        if (raw.size < 4) return Options(70, 15, 1920)
        val quality = (raw[0].toInt() and 0xFF).coerceIn(10, 95)
        val fps = (raw[1].toInt() and 0xFF).coerceIn(1, 30)
        val maxWidth = (raw[2].toInt() and 0xFF) or ((raw[3].toInt() and 0xFF) shl 8)
        return Options(quality, fps, maxWidth)
    }

    private fun begin(requested: Options) {
        val projection = Capture.projection
        if (projection == null) {
            bridge.screenFailed("Telefonda ekran yakalama izni verilmemiş. Mause uygulamasında Ekran kartından izni hazırlayın.")
            return
        }
        options = requested
        val metrics = context.resources.displayMetrics
        var width = metrics.widthPixels
        var height = metrics.heightPixels
        if (requested.maxWidth in 1 until width) {
            height = height * requested.maxWidth / width
            width = requested.maxWidth
        }
        reader?.close()
        val next = ImageReader.newInstance(width, height, PixelFormat.RGBA_8888, 2)
        next.setOnImageAvailableListener({ onFrame(it) }, handler)
        reader = next
        try {
            val current = display
            if (current == null) {
                display = projection.createVirtualDisplay(
                    "mause-screen",
                    width,
                    height,
                    metrics.densityDpi,
                    DisplayManager.VIRTUAL_DISPLAY_FLAG_AUTO_MIRROR,
                    next.surface,
                    null,
                    handler,
                )
            } else {
                current.resize(width, height, metrics.densityDpi)
                current.surface = next.surface
            }
        } catch (error: Exception) {
            pause()
            bridge.screenFailed("Ekran yakalanamadı: ${error.message}")
            return
        }
        bridge.screenInfo(
            AgentState.accessibilityReady,
            "Telefon ekranı (${metrics.widthPixels}×${metrics.heightPixels})",
        )
        AgentState.screenLive = true
    }

    private fun pause() {
        display?.surface = null
        reader?.close()
        reader = null
        AgentState.screenLive = false
    }

    private fun dropDisplay() {
        pause()
        display?.release()
        display = null
        canvas?.recycle()
        canvas = null
    }

    private fun onFrame(source: ImageReader) {
        val image = source.acquireLatestImage() ?: return
        try {
            val now = SystemClock.uptimeMillis()
            if (now - lastFrame < 1000L / options.fps) return
            lastFrame = now
            val plane = image.planes[0]
            val stride = plane.rowStride / plane.pixelStride
            val reusable = canvas?.takeIf { it.width == stride && it.height == image.height }
            val full = reusable ?: Bitmap
                .createBitmap(stride, image.height, Bitmap.Config.ARGB_8888)
                .also {
                    canvas?.recycle()
                    canvas = it
                }
            full.copyPixelsFromBuffer(plane.buffer)
            val visible = if (stride == image.width) {
                full
            } else {
                Bitmap.createBitmap(full, 0, 0, image.width, image.height)
            }
            val output = ByteArrayOutputStream(96 * 1024)
            visible.compress(Bitmap.CompressFormat.JPEG, options.quality, output)
            if (visible !== full) visible.recycle()
            val bytes = output.toByteArray()
            if (!bridge.screenFrame(bytes, bytes.size, image.width, image.height)) pause()
        } finally {
            image.close()
        }
    }
}
