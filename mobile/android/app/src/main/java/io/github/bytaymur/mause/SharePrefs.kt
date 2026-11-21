package io.github.bytaymur.mause

import android.content.Context

object SharePrefs {
    private const val CLIPBOARD = "clipboard_allowed"
    private const val FILES = "files_allowed"
    private const val AUDIO = "share_audio"
    private const val MIC = "share_mic"
    private const val PLAY = "play_pc_audio_v2"
    private const val SCREEN = "share_screen"
    private const val REMOTE = "remote_prefs"
    private const val CAMERA = "share_camera"
    private const val DEVICE = "share_device"

    private const val CONTROL_IN = 1 shl 0
    private const val AUDIO_OUT = 1 shl 1
    private const val MIC_OUT = 1 shl 2
    private const val FILES_IN = 1 shl 3
    private const val CLIP_IN = 1 shl 4
    private const val RELAY_IN = 1 shl 6
    private const val SHARE_TEXT = 1 shl 8

    fun load(context: Context) {
        val prefs = context.getSharedPreferences("mause", Context.MODE_PRIVATE)
        AgentState.clipboardAllowed = prefs.getBoolean(CLIPBOARD, true)
        AgentState.filesAllowed = prefs.getBoolean(FILES, false)
        AgentState.shareAudio = prefs.getBoolean(AUDIO, false)
        AgentState.shareMic = prefs.getBoolean(MIC, false)
        AgentState.playPcAudio = prefs.getBoolean(PLAY, true)
        AgentState.shareScreen = prefs.getBoolean(SCREEN, false)
        AgentState.remotePrefs = prefs.getBoolean(REMOTE, true)
        AgentState.shareCamera = prefs.getBoolean(CAMERA, false)
        AgentState.shareDevice = prefs.getBoolean(DEVICE, false)
    }

    fun save(context: Context) {
        context.getSharedPreferences("mause", Context.MODE_PRIVATE).edit()
            .putBoolean(CLIPBOARD, AgentState.clipboardAllowed)
            .putBoolean(FILES, AgentState.filesAllowed)
            .putBoolean(AUDIO, AgentState.shareAudio)
            .putBoolean(MIC, AgentState.shareMic)
            .putBoolean(PLAY, AgentState.playPcAudio)
            .putBoolean(SCREEN, AgentState.shareScreen)
            .putBoolean(REMOTE, AgentState.remotePrefs)
            .putBoolean(CAMERA, AgentState.shareCamera)
            .putBoolean(DEVICE, AgentState.shareDevice)
            .apply()
    }

    fun bits(): Int {
        var bits = CONTROL_IN
        if (AgentState.clipboardAllowed) bits = bits or CLIP_IN or SHARE_TEXT
        if (AgentState.shareAudio && AgentState.captureReady && AgentState.micReady) bits = bits or AUDIO_OUT
        if (AgentState.shareMic && AgentState.micReady) bits = bits or MIC_OUT
        if (AgentState.filesAllowed) bits = bits or FILES_IN
        if (AgentState.playPcAudio) bits = bits or RELAY_IN
        return bits
    }

    fun screenAllowed(): Boolean = AgentState.shareScreen && AgentState.captureReady
}
