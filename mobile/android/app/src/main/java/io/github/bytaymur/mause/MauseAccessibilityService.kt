package io.github.bytaymur.mause

import android.accessibilityservice.AccessibilityService
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.graphics.Path
import android.graphics.Rect
import android.graphics.PixelFormat
import android.view.View
import android.view.WindowManager
import android.view.accessibility.AccessibilityEvent
import android.view.accessibility.AccessibilityNodeInfo

class MauseAccessibilityService : AccessibilityService() {
    private lateinit var windowManager: WindowManager
    private lateinit var cursor: CursorView
    val screenWidth get() = resources.displayMetrics.widthPixels
    val screenHeight get() = resources.displayMetrics.heightPixels

    override fun onServiceConnected() {
        KeyboardInput.load(this)
        windowManager = getSystemService(WINDOW_SERVICE) as WindowManager
        cursor = CursorView()
        val params = WindowManager.LayoutParams(
            WindowManager.LayoutParams.MATCH_PARENT,
            WindowManager.LayoutParams.MATCH_PARENT,
            WindowManager.LayoutParams.TYPE_ACCESSIBILITY_OVERLAY,
            WindowManager.LayoutParams.FLAG_NOT_FOCUSABLE or
                WindowManager.LayoutParams.FLAG_NOT_TOUCHABLE or
                WindowManager.LayoutParams.FLAG_LAYOUT_IN_SCREEN,
            PixelFormat.TRANSLUCENT,
        )
        windowManager.addView(cursor, params)
        InputController.attach(this)
    }

    override fun onDestroy() {
        InputController.detach(this)
        if (::cursor.isInitialized) windowManager.removeView(cursor)
        super.onDestroy()
    }

    override fun onAccessibilityEvent(event: AccessibilityEvent?) = Unit

    override fun onInterrupt() = Unit

    fun clickAt(x: Float, y: Float, long: Boolean): Boolean {
        val target = windows
            .sortedByDescending { it.layer }
            .firstNotNullOfOrNull { window -> window.root?.let { hit(it, x.toInt(), y.toInt(), long) } }
            ?: return false
        val action = if (long) AccessibilityNodeInfo.ACTION_LONG_CLICK else AccessibilityNodeInfo.ACTION_CLICK
        return target.performAction(action)
    }

    private fun hit(node: AccessibilityNodeInfo, x: Int, y: Int, long: Boolean): AccessibilityNodeInfo? {
        val bounds = Rect()
        node.getBoundsInScreen(bounds)
        if (!node.isVisibleToUser || !bounds.contains(x, y)) return null
        for (index in node.childCount - 1 downTo 0) {
            val child = node.getChild(index) ?: continue
            hit(child, x, y, long)?.let { return it }
        }
        val usable = if (long) node.isLongClickable else node.isClickable
        return node.takeIf { usable && node.isEnabled }
    }

    fun showCursor(x: Float, y: Float) {
        if (cursor.visible && cursor.xPosition == x && cursor.yPosition == y) return
        cursor.xPosition = x
        cursor.yPosition = y
        cursor.visible = true
        cursor.invalidate()
    }

    fun hideCursor() {
        cursor.visible = false
        cursor.invalidate()
    }

    private inner class CursorView : View(this) {
        private val fill = Paint(Paint.ANTI_ALIAS_FLAG).apply { color = Color.rgb(255, 152, 0) }
        private val edge = Paint(Paint.ANTI_ALIAS_FLAG).apply {
            color = Color.WHITE
            style = Paint.Style.STROKE
            strokeWidth = 3f
        }
        private val arrow = Path().apply {
            moveTo(0f, 0f)
            lineTo(0f, 26f)
            lineTo(6.5f, 19.5f)
            lineTo(11f, 29f)
            lineTo(15f, 27f)
            lineTo(10.5f, 18.5f)
            lineTo(19f, 18.5f)
            close()
        }
        var xPosition = 0f
        var yPosition = 0f
        var visible = false

        override fun onDraw(canvas: Canvas) {
            if (!visible) return
            canvas.save()
            canvas.translate(xPosition, yPosition)
            canvas.drawPath(arrow, fill)
            canvas.drawPath(arrow, edge)
            canvas.restore()
        }
    }
}
