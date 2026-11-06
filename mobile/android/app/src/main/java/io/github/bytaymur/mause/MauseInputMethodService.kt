package io.github.bytaymur.mause

import android.inputmethodservice.InputMethodService
import android.view.View
import android.view.inputmethod.InputConnection

class MauseInputMethodService : InputMethodService() {
    override fun onCreate() {
        super.onCreate()
        current = this
        AgentState.keyboardReady = true
    }

    override fun onDestroy() {
        if (current === this) current = null
        AgentState.keyboardReady = false
        super.onDestroy()
    }

    override fun onCreateInputView(): View = View(this).apply {
        minimumHeight = 1
    }

    companion object {
        private var current: MauseInputMethodService? = null

        fun connection(): InputConnection? = current?.currentInputConnection
    }
}
