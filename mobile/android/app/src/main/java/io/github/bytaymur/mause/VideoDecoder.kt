package io.github.bytaymur.mause

import android.media.MediaCodec
import android.media.MediaFormat
import android.os.Build
import android.view.Surface

class VideoDecoder {
    private var codec: MediaCodec? = null
    private var surface: Surface? = null
    private var width = 0
    private var height = 0
    private var presentation = 0L

    @Synchronized
    fun setSurface(value: Surface?) {
        if (value === surface) return
        close()
        surface = value
    }

    @Synchronized
    fun feed(frameWidth: Int, frameHeight: Int, data: ByteArray) {
        val target = surface ?: return
        try {
            if (codec == null || frameWidth != width || frameHeight != height) {
                close()
                if (!startsStream(data)) return
                open(target, frameWidth, frameHeight)
            }
            val current = codec ?: return
            if (!queue(current, data)) {
                close()
                return
            }
            render(current)
        } catch (error: Exception) {
            close()
        }
    }

    @Synchronized
    fun release() {
        close()
        surface = null
    }

    private fun open(target: Surface, frameWidth: Int, frameHeight: Int) {
        val format = MediaFormat.createVideoFormat(MIME, frameWidth, frameHeight)
        format.setInteger(MediaFormat.KEY_PRIORITY, 0)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            format.setInteger(MediaFormat.KEY_LOW_LATENCY, 1)
        }
        val created = MediaCodec.createDecoderByType(MIME)
        try {
            created.configure(format, target, null, 0)
            created.start()
        } catch (error: Exception) {
            created.release()
            throw error
        }
        codec = created
        width = frameWidth
        height = frameHeight
    }

    private fun queue(current: MediaCodec, data: ByteArray): Boolean {
        val index = current.dequeueInputBuffer(INPUT_TIMEOUT_US)
        if (index < 0) return false
        val buffer = current.getInputBuffer(index) ?: return false
        if (data.size > buffer.capacity()) {
            current.queueInputBuffer(index, 0, 0, presentation, 0)
            return false
        }
        buffer.clear()
        buffer.put(data)
        current.queueInputBuffer(index, 0, data.size, presentation, 0)
        presentation += FRAME_US
        return true
    }

    private fun render(current: MediaCodec) {
        val info = MediaCodec.BufferInfo()
        var pending = -1
        while (true) {
            val output = current.dequeueOutputBuffer(info, 0)
            if (output >= 0) {
                if (pending >= 0) current.releaseOutputBuffer(pending, false)
                pending = output
            } else if (output == MediaCodec.INFO_TRY_AGAIN_LATER) {
                break
            }
        }
        if (pending >= 0) current.releaseOutputBuffer(pending, true)
    }

    private fun startsStream(data: ByteArray): Boolean {
        var index = 0
        while (index + 4 < data.size) {
            if (data[index].toInt() == 0 && data[index + 1].toInt() == 0) {
                val offset = when {
                    data[index + 2].toInt() == 1 -> index + 3
                    data[index + 2].toInt() == 0 && data[index + 3].toInt() == 1 -> index + 4
                    else -> -1
                }
                if (offset in 0 until data.size) {
                    val type = data[offset].toInt() and 0x1F
                    if (type == 5 || type == 7) return true
                    index = offset
                    continue
                }
            }
            index++
        }
        return false
    }

    private fun close() {
        codec?.let {
            runCatching { it.stop() }
            it.release()
        }
        codec = null
        width = 0
        height = 0
    }

    companion object {
        private const val MIME = "video/avc"
        private const val INPUT_TIMEOUT_US = 50_000L
        private const val FRAME_US = 33_333L
    }
}

object Viewer {
    val decoder = VideoDecoder()
}
