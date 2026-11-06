package io.github.bytaymur.mause

import android.accessibilityservice.GestureDescription
import android.graphics.Path
import android.graphics.PointF
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import java.util.concurrent.ConcurrentLinkedQueue
import java.util.concurrent.atomic.AtomicBoolean
import kotlin.math.abs

object InputController {
    private val main = Handler(Looper.getMainLooper())
    private var service: MauseAccessibilityService? = null
    private var live: LiveStroke? = null
    private var x = 0f
    private var y = 0f
    private var side = 0
    private var inside = false
    private var leftDown = false
    private var downAt = 0L
    private var downX = 0f
    private var downY = 0f
    private var liveStarted = false
    private val startLive = Runnable { beginLive() }
    private val stroke = mutableListOf<PointF>()
    private var wheel = 0
    private var hwheel = 0
    private val flushWheel = Runnable { service?.let(::scrollPending) }
    private val pending = ConcurrentLinkedQueue<IntArray>()
    private val draining = AtomicBoolean(false)
    private val drain = Runnable { drainPending() }

    fun attach(value: MauseAccessibilityService) {
        main.post {
            service = value
            live = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) LiveStroke(value) else null
            AgentState.accessibilityReady = true
        }
    }

    fun detach(value: MauseAccessibilityService) {
        main.post {
            if (service === value) {
                service = null
                live = null
            }
            AgentState.accessibilityReady = false
        }
    }

    fun enter(position: Int, side: Int) = main.post {
        val current = service ?: return@post
        val width = current.screenWidth.toFloat()
        val height = current.screenHeight.toFloat()
        val along = position / 65535f
        this.side = side
        inside = true
        when (side) {
            0 -> {
                x = width - 2f
                y = along * height
            }
            1 -> {
                x = 2f
                y = along * height
            }
            2 -> {
                x = along * width
                y = height - 2f
            }
            else -> {
                x = along * width
                y = 2f
            }
        }
        current.showCursor(x, y)
    }

    fun release() = main.post {
        inside = false
        endLive()
        leftDown = false
        stroke.clear()
        service?.hideCursor()
        KeyboardInput.reset()
    }

    fun input(events: IntArray) {
        pending.add(events)
        if (draining.compareAndSet(false, true)) main.post(drain)
    }

    private fun drainPending() {
        draining.set(false)
        val current = service
        while (true) {
            val events = pending.poll() ?: break
            if (current != null && inside) apply(current, events)
        }
        if (current == null || !inside) return
        current.showCursor(x, y)
        if (wheel != 0 || hwheel != 0) {
            main.removeCallbacks(flushWheel)
            main.postDelayed(flushWheel, WHEEL_SETTLE_MS)
        }
    }

    private fun apply(current: MauseAccessibilityService, events: IntArray) {
        val width = current.screenWidth.toFloat()
        val height = current.screenHeight.toFloat()
        var index = 0
        while (index + 2 < events.size) {
            val type = events[index]
            val code = events[index + 1]
            val value = events[index + 2]
            if (type == 2) {
                when (code) {
                    0 -> {
                        val next = x + value
                        if (crossesBack(next, y, width, height)) return leave(current, width, height)
                        x = next.coerceIn(0f, width - 1f)
                        track()
                    }
                    1 -> {
                        val next = y + value
                        if (crossesBack(x, next, width, height)) return leave(current, width, height)
                        y = next.coerceIn(0f, height - 1f)
                        track()
                    }
                    8 -> wheel += value
                    6 -> hwheel += value
                }
            } else if (type == 1) {
                button(current, code, value)
            }
            index += 3
        }
    }

    fun screenInput(events: IntArray) = main.post {
        val current = service ?: return@post
        val width = current.screenWidth.toFloat()
        val height = current.screenHeight.toFloat()
        var index = 0
        while (index + 2 < events.size) {
            val type = events[index]
            val code = events[index + 1]
            val value = events[index + 2]
            when (type) {
                3 -> when (code) {
                    0 -> {
                        x = value.coerceIn(0, ABSOLUTE_RANGE) / ABSOLUTE_RANGE.toFloat() * (width - 1f)
                        track()
                    }
                    1 -> {
                        y = value.coerceIn(0, ABSOLUTE_RANGE) / ABSOLUTE_RANGE.toFloat() * (height - 1f)
                        track()
                    }
                }
                2 -> when (code) {
                    8 -> wheel += value
                    6 -> hwheel += value
                }
                1 -> button(current, code, value)
            }
            index += 3
        }
        current.showCursor(x, y)
        if (wheel != 0 || hwheel != 0) {
            main.removeCallbacks(flushWheel)
            main.postDelayed(flushWheel, WHEEL_SETTLE_MS)
        }
    }

    private fun button(current: MauseAccessibilityService, code: Int, value: Int) {
        when (code) {
            272 -> {
                if (value == 1 && !leftDown) press()
                if (value == 0 && leftDown) lift(current)
            }
            273 -> if (value == 1 && !current.clickAt(x, y, true)) {
                current.press(x, y, CONTEXT_PRESS_MS)
            }
            274, 275 -> if (value == 1) current.performGlobalAction(GLOBAL_ACTION_BACK)
            276 -> if (value == 1) current.performGlobalAction(GLOBAL_ACTION_RECENTS)
            in 277..279 -> Unit
            else -> KeyboardInput.key(current, code, value)
        }
    }

    private fun press() {
        leftDown = true
        downAt = SystemClock.uptimeMillis()
        downX = x
        downY = y
        liveStarted = false
        stroke.clear()
        stroke.add(PointF(x, y))
        if (live != null) main.postDelayed(startLive, CLICK_WINDOW_MS)
    }

    private fun beginLive() {
        val session = live ?: return
        if (!leftDown || liveStarted) return
        liveStarted = true
        session.down(downX, downY)
        if (x != downX || y != downY) session.move(x, y)
    }

    private fun endLive() {
        main.removeCallbacks(startLive)
        if (leftDown && liveStarted) live?.up(x, y)
        liveStarted = false
    }

    private fun track() {
        if (!leftDown) return
        live?.let {
            if (!liveStarted) {
                if (abs(downX - x) + abs(downY - y) >= DRAG_START) beginLive()
                return
            }
            it.move(x, y)
            return
        }
        val last = stroke.last()
        if (abs(last.x - x) + abs(last.y - y) >= TRACK_STEP) stroke.add(PointF(x, y))
    }

    private fun lift(current: MauseAccessibilityService) {
        leftDown = false
        main.removeCallbacks(startLive)
        live?.let {
            if (liveStarted) {
                it.up(x, y)
                liveStarted = false
            } else if (!current.clickAt(downX, downY, false)) {
                current.tap(downX, downY)
            }
            stroke.clear()
            return
        }
        val held = SystemClock.uptimeMillis() - downAt
        val start = stroke.firstOrNull() ?: PointF(x, y)
        val moved = abs(start.x - x) + abs(start.y - y) >= DRAG_THRESHOLD
        when {
            moved -> {
                if (stroke.last().let { it.x != x || it.y != y }) stroke.add(PointF(x, y))
                current.swipe(stroke.toList(), held.coerceIn(MIN_SWIPE_MS, MAX_SWIPE_MS))
            }
            held >= LONG_PRESS_MS -> current.press(x, y, held.coerceAtMost(MAX_SWIPE_MS))
            else -> current.tap(x, y)
        }
        stroke.clear()
    }

    private fun scrollPending(current: MauseAccessibilityService) {
        val (vertical, horizontal) = wheel to hwheel
        wheel = 0
        hwheel = 0
        current.scroll(x, y, vertical, horizontal)
    }

    private fun crossesBack(nextX: Float, nextY: Float, width: Float, height: Float) = when (side) {
        0 -> nextX > width - 1f
        1 -> nextX < 0f
        2 -> nextY > height - 1f
        else -> nextY < 0f
    }

    private fun leave(current: MauseAccessibilityService, width: Float, height: Float) {
        val along = when (side) {
            0, 1 -> y / height
            else -> x / width
        }
        inside = false
        endLive()
        leftDown = false
        current.hideCursor()
        KeyboardInput.reset()
        stroke.clear()
        AgentService.leave((along.coerceIn(0f, 1f) * 65535f).toInt())
    }

    private const val GLOBAL_ACTION_BACK = 1
    private const val GLOBAL_ACTION_RECENTS = 3
    private const val CONTEXT_PRESS_MS = 650L
    private const val CLICK_WINDOW_MS = 220L
    private const val DRAG_START = 12f
    private const val ABSOLUTE_RANGE = 65535
    private const val TRACK_STEP = 6f
    private const val DRAG_THRESHOLD = 14f
    private const val LONG_PRESS_MS = 450L
    private const val MIN_SWIPE_MS = 60L
    private const val MAX_SWIPE_MS = 2500L
    private const val WHEEL_SETTLE_MS = 45L
}

fun MauseAccessibilityService.tap(x: Float, y: Float) = press(x, y, 60)

fun MauseAccessibilityService.press(x: Float, y: Float, duration: Long) {
    val path = Path().apply { moveTo(x, y) }
    dispatch(path, duration)
}

fun MauseAccessibilityService.swipe(points: List<PointF>, duration: Long) {
    if (points.size < 2) return
    val path = Path().apply {
        moveTo(points.first().x, points.first().y)
        points.drop(1).forEach { lineTo(it.x, it.y) }
    }
    dispatch(path, duration)
}

fun MauseAccessibilityService.scroll(x: Float, y: Float, vertical: Int, horizontal: Int) {
    val step = 160f
    val dy = (vertical * step).coerceIn(-screenHeight * 0.7f, screenHeight * 0.7f)
    val dx = (-horizontal * step).coerceIn(-screenWidth * 0.7f, screenWidth * 0.7f)
    val startX = (x - dx / 2).coerceIn(20f, screenWidth - 20f)
    val startY = (y - dy / 2).coerceIn(20f, screenHeight - 20f)
    val path = Path().apply {
        moveTo(startX, startY)
        lineTo((startX + dx).coerceIn(20f, screenWidth - 20f), (startY + dy).coerceIn(20f, screenHeight - 20f))
    }
    dispatch(path, 220)
}

private fun MauseAccessibilityService.dispatch(path: Path, duration: Long) {
    dispatchGesture(
        GestureDescription.Builder()
            .addStroke(GestureDescription.StrokeDescription(path, 0, duration.coerceAtLeast(1)))
            .build(),
        null,
        null,
    )
}
