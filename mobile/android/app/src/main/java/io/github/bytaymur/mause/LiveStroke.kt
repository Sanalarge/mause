package io.github.bytaymur.mause

import android.accessibilityservice.AccessibilityService
import android.accessibilityservice.GestureDescription
import android.graphics.Path
import android.os.Build
import androidx.annotation.RequiresApi

class LiveStroke(private val service: AccessibilityService) {
    private val queue = ArrayDeque<FloatArray>()
    private var stroke: GestureDescription.StrokeDescription? = null
    private var lastX = 0f
    private var lastY = 0f
    private var busy = false

    private val callback = object : AccessibilityService.GestureResultCallback() {
        override fun onCompleted(gestureDescription: GestureDescription?) {
            busy = false
            pump()
        }

        override fun onCancelled(gestureDescription: GestureDescription?) {
            busy = false
            stroke = null
            queue.clear()
        }
    }

    fun down(x: Float, y: Float) = push(DOWN, x, y)

    fun move(x: Float, y: Float) = push(MOVE, x, y)

    fun up(x: Float, y: Float) = push(UP, x, y)

    private fun push(kind: Int, x: Float, y: Float) {
        val last = queue.lastOrNull()
        if (kind == MOVE && last != null && last[0] == MOVE.toFloat()) {
            last[1] = x
            last[2] = y
        } else {
            queue.addLast(floatArrayOf(kind.toFloat(), x, y))
        }
        pump()
    }

    private fun pump() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return
        while (!busy) {
            val op = queue.removeFirstOrNull() ?: return
            val kind = op[0].toInt()
            val x = op[1]
            val y = op[2]
            if (kind == DOWN && stroke != null) {
                queue.addFirst(op)
                queue.addFirst(floatArrayOf(UP.toFloat(), lastX, lastY))
                continue
            }
            val current = stroke
            val next = when {
                kind == DOWN -> begin(x, y)
                current == null -> continue
                else -> follow(current, x, y, kind == MOVE)
            }
            stroke = if (kind == UP) null else next
            lastX = x
            lastY = y
            busy = true
            val gesture = GestureDescription.Builder().addStroke(next).build()
            if (!service.dispatchGesture(gesture, callback, null)) {
                busy = false
                stroke = null
                queue.clear()
                return
            }
        }
    }

    @RequiresApi(Build.VERSION_CODES.O)
    private fun begin(x: Float, y: Float): GestureDescription.StrokeDescription =
        GestureDescription.StrokeDescription(Path().apply { moveTo(x, y) }, 0, START_MS, true)

    @RequiresApi(Build.VERSION_CODES.O)
    private fun follow(
        previous: GestureDescription.StrokeDescription,
        x: Float,
        y: Float,
        more: Boolean,
    ): GestureDescription.StrokeDescription {
        val path = Path().apply {
            moveTo(lastX, lastY)
            if (x != lastX || y != lastY) lineTo(x, y)
        }
        return previous.continueStroke(path, 0, if (more) STEP_MS else END_MS, more)
    }

    private companion object {
        const val DOWN = 0
        const val MOVE = 1
        const val UP = 2
        const val START_MS = 1L
        const val STEP_MS = 12L
        const val END_MS = 10L
    }
}
