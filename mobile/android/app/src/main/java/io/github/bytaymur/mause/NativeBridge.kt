package io.github.bytaymur.mause

class NativeBridge(private val callbacks: Callbacks) {
    interface Callbacks {
        fun onStatus(status: String)
        fun onPairRequest(computer: String, code: String)
        fun onConnected(connected: Boolean)
        fun onEnter(position: Int, side: Int)
        fun onInput(events: IntArray)
        fun onRelease()
        fun onClipboard(text: String)
        fun onActivity(text: String)
        fun onAudioRequest(kind: Int, on: Boolean)
        fun onAudioPlay(kind: Int, pcm: ByteArray)
        fun onScreenStart(options: ByteArray)
        fun onScreenStop()
        fun onScreenInput(events: IntArray)
        fun onPrefOff(pref: Int)
        fun onPrefOn(pref: Int)
        fun onDevice(op: Int, data: ByteArray)
        fun onViewFrame(width: Int, height: Int, jpeg: ByteArray)
        fun onViewEnded(reason: String)
        fun onViewInfo(control: Boolean)
    }

    external fun start(
        name: String,
        id: String,
        port: Int,
        width: Int,
        height: Int,
        storagePath: String,
    ): Boolean

    external fun stop()
    external fun answerPair(accept: Boolean)
    external fun leave(position: Int)
    external fun configureFiles(shareRoot: String, inbox: String)
    external fun setPrefs(bits: Int)
    external fun setScreenAllowed(allowed: Boolean)
    external fun requestAudio(kind: Int, on: Boolean)
    external fun audioPacket(kind: Int, pcm: ByteArray, length: Int)
    external fun screenFrame(jpeg: ByteArray, length: Int, width: Int, height: Int): Boolean
    external fun screenInfo(control: Boolean, label: String)
    external fun screenFailed(reason: String)
    external fun sendFile(descriptor: Int, name: String, size: Long)
    external fun deviceReply(op: Int, data: ByteArray)
    external fun cameraOpen(): Boolean
    external fun cameraFrame(jpeg: ByteArray, length: Int, width: Int, height: Int): Boolean
    external fun cameraClose()
    external fun watchScreen(on: Boolean, options: ByteArray)
    external fun sendScreenInput(events: IntArray)

    private fun nativeStatus(status: String) = callbacks.onStatus(status)
    private fun nativePairRequest(computer: String, code: String) =
        callbacks.onPairRequest(computer, code)
    private fun nativeConnected(connected: Boolean) = callbacks.onConnected(connected)
    private fun nativeEnter(position: Int, side: Int) = callbacks.onEnter(position, side)
    private fun nativeInput(events: IntArray) = callbacks.onInput(events)
    private fun nativeRelease() = callbacks.onRelease()
    private fun nativeClipboard(text: String) = callbacks.onClipboard(text)
    private fun nativeActivity(text: String) = callbacks.onActivity(text)
    private fun nativeAudioRequest(kind: Int, on: Boolean) = callbacks.onAudioRequest(kind, on)
    private fun nativeAudioPlay(kind: Int, pcm: ByteArray) = callbacks.onAudioPlay(kind, pcm)
    private fun nativeScreenStart(options: ByteArray) = callbacks.onScreenStart(options)
    private fun nativeScreenStop() = callbacks.onScreenStop()
    private fun nativeScreenInput(events: IntArray) = callbacks.onScreenInput(events)
    private fun nativePrefOff(pref: Int) = callbacks.onPrefOff(pref)
    private fun nativePrefOn(pref: Int) = callbacks.onPrefOn(pref)
    private fun nativeDevice(op: Int, data: ByteArray) = callbacks.onDevice(op, data)
    private fun nativeViewFrame(width: Int, height: Int, jpeg: ByteArray) =
        callbacks.onViewFrame(width, height, jpeg)
    private fun nativeViewEnded(reason: String) = callbacks.onViewEnded(reason)
    private fun nativeViewInfo(control: Boolean) = callbacks.onViewInfo(control)

    companion object {
        init {
            System.loadLibrary("mause_android")
        }
    }
}
