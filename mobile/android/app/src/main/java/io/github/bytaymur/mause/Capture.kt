package io.github.bytaymur.mause

import android.content.Context
import android.content.Intent
import android.media.projection.MediaProjection
import android.media.projection.MediaProjectionManager
import android.os.Handler
import android.os.Looper
import java.util.concurrent.CopyOnWriteArrayList

object Capture {
    @Volatile
    var projection: MediaProjection? = null
        private set

    private val listeners = CopyOnWriteArrayList<() -> Unit>()

    fun onStopped(listener: () -> Unit) {
        listeners.add(listener)
    }

    fun grant(context: Context, resultCode: Int, data: Intent) {
        release()
        val manager = context.getSystemService(MediaProjectionManager::class.java)
        val granted = manager.getMediaProjection(resultCode, data)
            ?: throw IllegalStateException("ekran yakalama izni alınamadı")
        granted.registerCallback(
            object : MediaProjection.Callback() {
                override fun onStop() {
                    if (projection === granted) clear()
                }
            },
            Handler(Looper.getMainLooper()),
        )
        projection = granted
        AgentState.captureReady = true
    }

    fun release() {
        projection?.let { held ->
            projection = null
            runCatching { held.stop() }
        }
        clear()
    }

    private fun clear() {
        projection = null
        AgentState.captureReady = false
        listeners.forEach { it() }
    }
}
