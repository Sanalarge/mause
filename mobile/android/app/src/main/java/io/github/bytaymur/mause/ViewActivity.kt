package io.github.bytaymur.mause

import android.content.pm.ActivityInfo
import android.os.Bundle
import android.view.SurfaceHolder
import android.view.SurfaceView
import android.view.WindowManager
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.detectDragGestures
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat

class ViewActivity : ComponentActivity() {
    private var extend = false
    private var started = false

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        extend = intent.getBooleanExtra(EXTEND, false)
        requestedOrientation = ActivityInfo.SCREEN_ORIENTATION_SENSOR_LANDSCAPE
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        WindowCompat.setDecorFitsSystemWindows(window, false)
        WindowInsetsControllerCompat(window, window.decorView).apply {
            systemBarsBehavior = WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
            hide(WindowInsetsCompat.Type.systemBars())
        }
        AgentState.viewNote = null
        AgentState.viewFrame = null
        AgentState.viewVideo = false
        AgentState.viewWidth = 0
        AgentState.viewHeight = 0
        setContent {
            MaterialTheme(colorScheme = darkColorScheme()) {
                ViewScreen(onSurface = ::begin, onClose = ::finish)
            }
        }
    }

    private fun begin() {
        if (started) return
        started = true
        AgentService.watchScreen(true, extend)
    }

    override fun onDestroy() {
        AgentService.watchScreen(false, extend)
        Viewer.decoder.release()
        AgentState.viewFrame = null
        AgentState.viewVideo = false
        super.onDestroy()
    }

    companion object {
        const val EXTEND = "extend"
    }
}

private const val LEFT = 272
private const val RIGHT = 273

@Composable
private fun ViewScreen(onSurface: () -> Unit, onClose: () -> Unit) {
    val frame = AgentState.viewFrame
    val note = AgentState.viewNote
    val source = IntSize(AgentState.viewWidth, AgentState.viewHeight)
    var area by remember { mutableStateOf(IntSize.Zero) }
    Box(
        Modifier
            .fillMaxSize()
            .background(Color.Black)
            .onSizeChanged { area = it },
    ) {
        AndroidView(
            factory = { context ->
                SurfaceView(context).apply {
                    holder.addCallback(
                        object : SurfaceHolder.Callback {
                            override fun surfaceCreated(holder: SurfaceHolder) {
                                Viewer.decoder.setSurface(holder.surface)
                                onSurface()
                            }

                            override fun surfaceChanged(
                                holder: SurfaceHolder,
                                format: Int,
                                width: Int,
                                height: Int,
                            ) = Unit

                            override fun surfaceDestroyed(holder: SurfaceHolder) {
                                Viewer.decoder.setSurface(null)
                            }
                        },
                    )
                }
            },
            modifier = Modifier
                .align(Alignment.Center)
                .then(
                    if (source.width > 0 && source.height > 0) {
                        Modifier.aspectRatio(source.width / source.height.toFloat())
                    } else {
                        Modifier.fillMaxSize()
                    },
                ),
        )
        if (frame != null) {
            Image(
                bitmap = frame.asImageBitmap(),
                contentDescription = null,
                contentScale = ContentScale.Fit,
                modifier = Modifier.fillMaxSize(),
            )
        }
        Box(
            Modifier
                .fillMaxSize()
                .pointerInput(source, area) {
                    detectTapGestures(
                        onTap = { click(it, area, source, LEFT) },
                        onLongPress = { click(it, area, source, RIGHT) },
                    )
                }
                .pointerInput(source, area) {
                    detectDragGestures(
                        onDragStart = { send(pointer(it, area, source) + key(LEFT, 1)) },
                        onDrag = { change, _ ->
                            change.consume()
                            send(pointer(change.position, area, source))
                        },
                        onDragEnd = { send(key(LEFT, 0)) },
                        onDragCancel = { send(key(LEFT, 0)) },
                    )
                },
        )
        if (frame == null && !AgentState.viewVideo) {
            Text(
                note ?: "Bilgisayarın ekranı bekleniyor…",
                color = Color.White,
                modifier = Modifier
                    .align(Alignment.Center)
                    .padding(24.dp),
            )
        } else if (!AgentState.viewControl) {
            Text(
                "Bilgisayar uzaktan kontrole izin vermiyor",
                color = Color.White,
                modifier = Modifier
                    .align(Alignment.BottomCenter)
                    .padding(12.dp),
            )
        }
        Row(
            Modifier
                .align(Alignment.TopEnd)
                .padding(8.dp),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            OutlinedButton(onClick = { send(wheel(3)) }) { Text("↑") }
            OutlinedButton(onClick = { send(wheel(-3)) }) { Text("↓") }
            Button(onClick = onClose) { Text("Kapat") }
        }
    }
}

private fun pointer(offset: Offset, area: IntSize, source: IntSize): IntArray {
    if (area.width == 0 || area.height == 0 || source.width == 0 || source.height == 0) {
        return intArrayOf(3, 0, 0, 3, 1, 0)
    }
    val scale = minOf(area.width / source.width.toFloat(), area.height / source.height.toFloat())
    val shownWidth = source.width * scale
    val shownHeight = source.height * scale
    val left = (area.width - shownWidth) / 2f
    val top = (area.height - shownHeight) / 2f
    val x = ((offset.x - left) / shownWidth).coerceIn(0f, 1f)
    val y = ((offset.y - top) / shownHeight).coerceIn(0f, 1f)
    return intArrayOf(3, 0, (x * 65535).toInt(), 3, 1, (y * 65535).toInt())
}

private fun key(code: Int, value: Int) = intArrayOf(1, code, value)

private fun wheel(amount: Int) = intArrayOf(2, 8, amount)

private fun click(offset: Offset, area: IntSize, source: IntSize, button: Int) =
    send(pointer(offset, area, source) + key(button, 1) + key(button, 0))

private fun send(events: IntArray) {
    if (AgentState.viewControl) AgentService.sendScreenInput(events)
}
