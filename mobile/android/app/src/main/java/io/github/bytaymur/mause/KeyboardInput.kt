package io.github.bytaymur.mause

import android.accessibilityservice.AccessibilityService
import android.content.Context
import android.os.Build
import android.os.Bundle
import android.view.KeyEvent
import android.view.accessibility.AccessibilityNodeInfo
import android.view.inputmethod.InputConnection

object KeyboardInput {
    private const val PREFS = "mause"
    private const val LAYOUT_KEY = "keyboard_layout"
    private val layout get() = AgentState.keyboardLayout
    private val held = mutableSetOf<Int>()
    private var caps = false
    private var passwordNode: AccessibilityNodeInfo? = null
    private var passwordText = ""
    private var editedNode: AccessibilityNodeInfo? = null

    private val shift get() = SHIFT in held || RIGHT_SHIFT in held
    private val altGr get() = RIGHT_ALT in held
    private val ctrl get() = !altGr && (CTRL in held || RIGHT_CTRL in held)

    fun load(context: Context) {
        val name = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).getString(LAYOUT_KEY, null)
        AgentState.keyboardLayout = KeyLayout.fromName(name)
    }

    fun choose(context: Context, value: KeyLayout) {
        AgentState.keyboardLayout = value
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit().putString(LAYOUT_KEY, value.name).apply()
    }

    fun reset() {
        held.clear()
        MauseInputMethodService.connection()?.clearMetaKeyStates(KeyEvent.META_ALT_ON or KeyEvent.META_SHIFT_ON or KeyEvent.META_CTRL_ON)
    }

    fun key(service: AccessibilityService, code: Int, value: Int) {
        if (value == 0) {
            held.remove(code)
            return
        }
        if (value == 1) held.add(code)
        if (code in MODIFIERS) return
        when (code) {
            CAPS_LOCK -> if (value == 1) caps = !caps
            ESCAPE -> service.performGlobalAction(AccessibilityService.GLOBAL_ACTION_BACK)
            META, RIGHT_META -> service.performGlobalAction(AccessibilityService.GLOBAL_ACTION_HOME)
            BACKSPACE -> erase(service, before = true)
            DELETE -> erase(service, before = false)
            ENTER, KEYPAD_ENTER -> enter(service)
            TAB -> imeKey(KeyEvent.KEYCODE_TAB)
            LEFT -> moveCursor(service, KeyEvent.KEYCODE_DPAD_LEFT) { start, _, _ -> start - 1 }
            RIGHT -> moveCursor(service, KeyEvent.KEYCODE_DPAD_RIGHT) { _, end, _ -> end + 1 }
            HOME -> moveCursor(service, KeyEvent.KEYCODE_MOVE_HOME) { _, _, _ -> 0 }
            END -> moveCursor(service, KeyEvent.KEYCODE_MOVE_END) { _, _, length -> length }
            UP -> imeKey(KeyEvent.KEYCODE_DPAD_UP)
            DOWN -> imeKey(KeyEvent.KEYCODE_DPAD_DOWN)
            else -> if (ctrl) shortcut(service, code) else type(service, code)
        }
    }

    private fun type(service: AccessibilityService, code: Int) {
        val text = layout.character(code, shift, altGr, caps) ?: return
        MauseInputMethodService.connection()?.let {
            it.commitText(text, 1)
            return
        }
        val node = focusedField(service) ?: return
        val (current, start, end) = contents(node)
        replace(node, current.substring(0, start) + text + current.substring(end), start + text.length)
    }

    private fun erase(service: AccessibilityService, before: Boolean) {
        MauseInputMethodService.connection()?.let {
            imeKey(if (before) KeyEvent.KEYCODE_DEL else KeyEvent.KEYCODE_FORWARD_DEL)
            return
        }
        val node = focusedField(service) ?: return
        val (current, start, end) = contents(node)
        val (from, to) = when {
            start != end -> start to end
            before && start > 0 -> start - 1 to start
            !before && end < current.length -> end to end + 1
            else -> return
        }
        replace(node, current.removeRange(from, to), from)
    }

    private fun enter(service: AccessibilityService) {
        if (imeKey(KeyEvent.KEYCODE_ENTER)) return
        val node = focusedField(service) ?: return
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R &&
            node.actionList.any { it.id == android.R.id.accessibilityActionImeEnter }
        ) {
            node.performAction(android.R.id.accessibilityActionImeEnter)
        } else if (node.isMultiLine) {
            val (current, start, end) = contents(node)
            replace(node, current.substring(0, start) + "\n" + current.substring(end), start + 1)
        }
    }

    private fun moveCursor(
        service: AccessibilityService,
        imeCode: Int,
        target: (start: Int, end: Int, length: Int) -> Int,
    ) {
        if (imeKey(imeCode)) return
        val node = focusedField(service) ?: return
        val (current, start, end) = contents(node)
        select(node, target(start, end, current.length).coerceIn(0, current.length))
    }

    private fun shortcut(service: AccessibilityService, code: Int) {
        val menu = when (code) {
            KEY_A -> android.R.id.selectAll
            KEY_C -> android.R.id.copy
            KEY_X -> android.R.id.cut
            KEY_V -> android.R.id.paste
            else -> return
        }
        MauseInputMethodService.connection()?.let {
            it.performContextMenuAction(menu)
            return
        }
        val node = focusedField(service) ?: return
        when (code) {
            KEY_A -> select(node, 0, contents(node).first.length)
            KEY_C -> node.performAction(AccessibilityNodeInfo.ACTION_COPY)
            KEY_X -> node.performAction(AccessibilityNodeInfo.ACTION_CUT)
            KEY_V -> node.performAction(AccessibilityNodeInfo.ACTION_PASTE)
        }
    }

    private fun imeKey(code: Int): Boolean {
        val connection: InputConnection = MauseInputMethodService.connection() ?: return false
        connection.sendKeyEvent(KeyEvent(KeyEvent.ACTION_DOWN, code))
        connection.sendKeyEvent(KeyEvent(KeyEvent.ACTION_UP, code))
        return true
    }

    private fun focusedField(service: AccessibilityService): AccessibilityNodeInfo? =
        service.findFocus(AccessibilityNodeInfo.FOCUS_INPUT)?.takeIf { it.isEditable }

    private fun contents(node: AccessibilityNodeInfo): Triple<String, Int, Int> {
        val text = when {
            node.isPassword -> if (node == passwordNode) passwordText else "".also {
                passwordNode = node
                passwordText = ""
            }
            else -> visibleText(node)
        }
        val first = node.textSelectionStart
        val second = node.textSelectionEnd
        if (first < 0 || second < 0) return Triple(text, text.length, text.length)
        val start = minOf(first, second).coerceIn(0, text.length)
        val end = maxOf(first, second).coerceIn(start, text.length)
        return Triple(text, start, end)
    }

    private fun visibleText(node: AccessibilityNodeInfo): String {
        node.refresh()
        val text = node.text?.toString().orEmpty()
        if (text.isEmpty() || showsPlaceholder(node, text)) return ""
        return text
    }

    private fun showsPlaceholder(node: AccessibilityNodeInfo, text: String): Boolean {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            if (node.isShowingHintText) return true
            if (node.hintText?.toString() == text) return true
        }
        val untouched = node != editedNode
        val cursorAtStart = node.textSelectionStart <= 0 && node.textSelectionEnd <= 0
        return untouched && cursorAtStart
    }

    private fun replace(node: AccessibilityNodeInfo, text: String, cursor: Int) {
        editedNode = node
        val arguments = Bundle().apply {
            putCharSequence(AccessibilityNodeInfo.ACTION_ARGUMENT_SET_TEXT_CHARSEQUENCE, text)
        }
        node.performAction(AccessibilityNodeInfo.ACTION_SET_TEXT, arguments)
        if (node.isPassword) {
            passwordNode = node
            passwordText = text
        }
        select(node, cursor)
    }

    private fun select(node: AccessibilityNodeInfo, start: Int, end: Int = start) {
        val arguments = Bundle().apply {
            putInt(AccessibilityNodeInfo.ACTION_ARGUMENT_SELECTION_START_INT, start)
            putInt(AccessibilityNodeInfo.ACTION_ARGUMENT_SELECTION_END_INT, end)
        }
        node.performAction(AccessibilityNodeInfo.ACTION_SET_SELECTION, arguments)
    }

    private const val ESCAPE = 1
    private const val BACKSPACE = 14
    private const val TAB = 15
    private const val ENTER = 28
    private const val CTRL = 29
    private const val KEY_A = 30
    private const val SHIFT = 42
    private const val KEY_X = 45
    private const val KEY_C = 46
    private const val KEY_V = 47
    private const val RIGHT_SHIFT = 54
    private const val ALT = 56
    private const val CAPS_LOCK = 58
    private const val KEYPAD_ENTER = 96
    private const val RIGHT_CTRL = 97
    private const val RIGHT_ALT = 100
    private const val HOME = 102
    private const val UP = 103
    private const val LEFT = 105
    private const val RIGHT = 106
    private const val END = 107
    private const val DOWN = 108
    private const val DELETE = 111
    private const val META = 125
    private const val RIGHT_META = 126
    private val MODIFIERS = setOf(CTRL, SHIFT, RIGHT_SHIFT, ALT, RIGHT_CTRL, RIGHT_ALT)
}
