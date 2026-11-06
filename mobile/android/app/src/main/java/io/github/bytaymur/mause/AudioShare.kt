package io.github.bytaymur.mause

import android.annotation.SuppressLint
import android.media.AudioAttributes
import android.media.AudioFormat
import android.media.AudioPlaybackCaptureConfiguration
import android.media.AudioRecord
import android.media.AudioTrack
import android.media.MediaRecorder
import android.os.Build
import android.os.Process
import java.util.concurrent.ArrayBlockingQueue
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger
import kotlin.concurrent.thread

class AudioShare(private val bridge: NativeBridge) {
    private class Loop(val record: AudioRecord) {
        @Volatile
        var running = true
    }

    private val loops = arrayOfNulls<Loop>(2)
    private var track: AudioTrack? = null
    private val queue = ArrayBlockingQueue<ByteArray>(MAX_PACKETS)
    private val queued = AtomicInteger()

    @Volatile
    private var playing = false

    init {
        Capture.onStopped { stopCapture(0) }
    }

    @Synchronized
    fun request(kind: Int, on: Boolean) {
        if (kind !in 0..1) return
        stopCapture(kind)
        if (!on) return
        val record = open(kind)
        if (record == null) {
            AgentState.note("Ses yakalanamadı: izin ya da ekran yakalama hazır değil")
            return
        }
        val loop = Loop(record)
        loops[kind] = loop
        thread(name = "mause-audio-$kind", isDaemon = true) { capture(kind, loop) }
    }

    @Synchronized
    private fun stopCapture(kind: Int) {
        loops[kind]?.running = false
        loops[kind] = null
    }

    @SuppressLint("MissingPermission")
    private fun open(kind: Int): AudioRecord? {
        val mask = if (kind == 0) AudioFormat.CHANNEL_IN_STEREO else AudioFormat.CHANNEL_IN_MONO
        val format = AudioFormat.Builder()
            .setEncoding(AudioFormat.ENCODING_PCM_16BIT)
            .setSampleRate(SAMPLE_RATE)
            .setChannelMask(mask)
            .build()
        val minimum = AudioRecord.getMinBufferSize(SAMPLE_RATE, mask, AudioFormat.ENCODING_PCM_16BIT)
        val builder = AudioRecord.Builder()
            .setAudioFormat(format)
            .setBufferSizeInBytes(maxOf(minimum, CHUNK * 4))
        if (kind == 0) {
            val projection = Capture.projection ?: return null
            if (Build.VERSION.SDK_INT < Build.VERSION_CODES.Q) return null
            val config = AudioPlaybackCaptureConfiguration.Builder(projection)
                .addMatchingUsage(AudioAttributes.USAGE_MEDIA)
                .addMatchingUsage(AudioAttributes.USAGE_GAME)
                .addMatchingUsage(AudioAttributes.USAGE_UNKNOWN)
                .excludeUid(Process.myUid())
                .build()
            builder.setAudioPlaybackCaptureConfig(config)
        } else {
            builder.setAudioSource(MediaRecorder.AudioSource.MIC)
        }
        return runCatching { builder.build() }.getOrNull()
            ?.takeIf { it.state == AudioRecord.STATE_INITIALIZED }
    }

    private fun capture(kind: Int, loop: Loop) {
        val record = loop.record
        val stereo = ByteArray(CHUNK)
        val mono = ByteArray(CHUNK / 2)
        try {
            record.startRecording()
            while (loop.running) {
                if (kind == 0) {
                    if (!fill(record, stereo, CHUNK, loop)) break
                } else {
                    if (!fill(record, mono, CHUNK / 2, loop)) break
                    upmix(mono, stereo)
                }
                bridge.audioPacket(kind, stereo, CHUNK)
            }
        } finally {
            runCatching { record.stop() }
            record.release()
        }
    }

    private fun fill(record: AudioRecord, buffer: ByteArray, size: Int, loop: Loop): Boolean {
        var filled = 0
        while (filled < size && loop.running) {
            val read = record.read(buffer, filled, size - filled)
            if (read <= 0) return false
            filled += read
        }
        return filled == size
    }

    private fun upmix(mono: ByteArray, stereo: ByteArray) {
        var from = 0
        var to = 0
        while (from < mono.size) {
            stereo[to] = mono[from]
            stereo[to + 1] = mono[from + 1]
            stereo[to + 2] = mono[from]
            stereo[to + 3] = mono[from + 1]
            from += 2
            to += 4
        }
    }

    @Synchronized
    fun startPlayback() {
        if (track != null) return
        val minimum = AudioTrack.getMinBufferSize(
            SAMPLE_RATE,
            AudioFormat.CHANNEL_OUT_STEREO,
            AudioFormat.ENCODING_PCM_16BIT,
        )
        val builder = AudioTrack.Builder()
            .setAudioAttributes(
                AudioAttributes.Builder()
                    .setUsage(AudioAttributes.USAGE_MEDIA)
                    .setContentType(AudioAttributes.CONTENT_TYPE_MUSIC)
                    .build(),
            )
            .setAudioFormat(
                AudioFormat.Builder()
                    .setEncoding(AudioFormat.ENCODING_PCM_16BIT)
                    .setSampleRate(SAMPLE_RATE)
                    .setChannelMask(AudioFormat.CHANNEL_OUT_STEREO)
                    .build(),
            )
            .setBufferSizeInBytes(maxOf(minimum, CHUNK * TRACK_CHUNKS))
            .setTransferMode(AudioTrack.MODE_STREAM)
        val created = builder.build()
        created.play()
        track = created
        queue.clear()
        queued.set(0)
        playing = true
        thread(name = "mause-play", isDaemon = true) { playLoop(created) }
    }

    private fun playLoop(target: AudioTrack) {
        var primed = false
        while (playing) {
            if (!primed) {
                if (queued.get() < PREBUFFER_BYTES) {
                    Thread.sleep(5)
                    continue
                }
                primed = true
            }
            val chunk = queue.poll(30, TimeUnit.MILLISECONDS)
            if (chunk == null) {
                primed = false
                continue
            }
            queued.addAndGet(-chunk.size)
            target.write(chunk, 0, chunk.size)
        }
    }

    @Synchronized
    fun stopPlayback() {
        playing = false
        queue.clear()
        queued.set(0)
        track?.let {
            runCatching { it.stop() }
            it.release()
        }
        track = null
    }

    fun play(pcm: ByteArray) {
        if (!playing) return
        while (queued.get() + pcm.size > MAX_BYTES) {
            val dropped = queue.poll() ?: break
            queued.addAndGet(-dropped.size)
        }
        if (queue.offer(pcm)) queued.addAndGet(pcm.size)
    }

    fun release() {
        stopCapture(0)
        stopCapture(1)
        stopPlayback()
    }

    companion object {
        private const val SAMPLE_RATE = 48000
        private const val CHUNK = 3840
        private const val PREBUFFER_BYTES = 15_360
        private const val MAX_BYTES = 38_400
        private const val MAX_PACKETS = 256
        private const val TRACK_CHUNKS = 8
    }
}
