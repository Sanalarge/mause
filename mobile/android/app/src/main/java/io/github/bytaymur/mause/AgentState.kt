package io.github.bytaymur.mause

import android.graphics.Bitmap
import android.os.Handler
import android.os.Looper
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue

data class PairRequest(val computer: String, val code: String)

object AgentState {
    var running by mutableStateOf(false)
    var connected by mutableStateOf(false)
    var status by mutableStateOf("Hazır")
    var pairRequest by mutableStateOf<PairRequest?>(null)
    var accessibilityReady by mutableStateOf(false)
    var keyboardReady by mutableStateOf(false)
    var keyboardLayout by mutableStateOf(KeyLayout.TurkishQ)
    var activity by mutableStateOf<List<String>>(emptyList())
    var filesAllowed by mutableStateOf(false)
    var storageReady by mutableStateOf(false)
    var shareAudio by mutableStateOf(false)
    var shareMic by mutableStateOf(false)
    var playPcAudio by mutableStateOf(false)
    var shareScreen by mutableStateOf(false)
    var remotePrefs by mutableStateOf(true)
    var shareCamera by mutableStateOf(false)
    var shareDevice by mutableStateOf(false)
    var cameraReady by mutableStateOf(false)
    var locationReady by mutableStateOf(false)
    var captureReady by mutableStateOf(false)
    var micReady by mutableStateOf(false)
    var screenLive by mutableStateOf(false)
    var viewFrame by mutableStateOf<Bitmap?>(null)
    var viewNote by mutableStateOf<String?>(null)
    var viewControl by mutableStateOf(false)
    var viewVideo by mutableStateOf(false)
    var viewWidth by mutableStateOf(0)
    var viewHeight by mutableStateOf(0)

    private val main = Handler(Looper.getMainLooper())

    fun note(text: String) {
        main.post { activity = (listOf(text) + activity).take(6) }
    }
}
