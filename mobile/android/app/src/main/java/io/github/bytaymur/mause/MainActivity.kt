package io.github.bytaymur.mause

import android.Manifest
import android.accessibilityservice.AccessibilityServiceInfo
import android.content.Intent
import android.content.pm.PackageManager
import android.media.projection.MediaProjectionManager
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.Environment
import android.provider.Settings
import android.view.accessibility.AccessibilityManager
import android.view.inputmethod.InputMethodManager
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts

class MainActivity : ComponentActivity() {
    private val notificationPermission = registerForActivityResult(
        ActivityResultContracts.RequestPermission(),
    ) {}

    private val microphonePermission = registerForActivityResult(
        ActivityResultContracts.RequestPermission(),
    ) { granted ->
        AgentState.micReady = granted
        AgentService.permissionsChanged()
    }

    private val cameraPermission = registerForActivityResult(
        ActivityResultContracts.RequestPermission(),
    ) { granted ->
        AgentState.cameraReady = granted
        AgentService.permissionsChanged()
    }

    private val locationPermission = registerForActivityResult(
        ActivityResultContracts.RequestPermission(),
    ) { granted ->
        AgentState.locationReady = granted
        AgentService.permissionsChanged()
    }

    private val storagePermission = registerForActivityResult(
        ActivityResultContracts.RequestPermission(),
    ) {
        refreshCapabilities()
        AgentService.sharingChanged()
    }

    private val captureConsent = registerForActivityResult(
        ActivityResultContracts.StartActivityForResult(),
    ) { result ->
        val data = result.data
        if (result.resultCode == RESULT_OK && data != null) {
            AgentService.grantCapture(result.resultCode, data)
        }
    }

    private val filePicker = registerForActivityResult(
        ActivityResultContracts.GetMultipleContents(),
    ) { uris ->
        if (uris.isNotEmpty()) AgentService.sendFiles(uris)
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        KeyboardInput.load(this)
        SharePrefs.load(this)
        refreshCapabilities()
        setContent {
            MauseTheme {
                MauseApp(
                    version = appVersion(),
                    onStart = ::startAgent,
                    onStop = ::stopAgent,
                    onPairAnswer = AgentService::answerPair,
                    share = shareActions(),
                    system = SystemActions(
                        onAccessibility = {
                            startActivity(Intent(Settings.ACTION_ACCESSIBILITY_SETTINGS))
                        },
                        onKeyboard = {
                            startActivity(Intent(Settings.ACTION_INPUT_METHOD_SETTINGS))
                        },
                        onAppInfo = {
                            startActivity(
                                Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS)
                                    .setData(Uri.fromParts("package", packageName, null)),
                            )
                        },
                        onLayout = { KeyboardInput.choose(this, it) },
                    ),
                )
            }
        }
    }

    private fun appVersion(): String =
        runCatching { packageManager.getPackageInfo(packageName, 0).versionName }.getOrNull() ?: ""

    override fun onResume() {
        super.onResume()
        refreshCapabilities()
        AgentService.sharingChanged()
    }

    private fun shareActions() = ShareActions(
        onFiles = { change { AgentState.filesAllowed = it } },
        onStorage = ::requestStorage,
        onSendFiles = { filePicker.launch("*/*") },
        onShareAudio = { on ->
            change { AgentState.shareAudio = on }
            if (on) requireMicrophone()
        },
        onShareMic = { on ->
            change { AgentState.shareMic = on }
            if (on) requireMicrophone()
        },
        onPlayAudio = { change { AgentState.playPcAudio = it } },
        onShareScreen = { change { AgentState.shareScreen = it } },
        onRemotePrefs = { change { AgentState.remotePrefs = it } },
        onShareCamera = { on ->
            change { AgentState.shareCamera = on }
            if (on && !AgentState.cameraReady) cameraPermission.launch(Manifest.permission.CAMERA)
        },
        onShareDevice = { on ->
            change { AgentState.shareDevice = on }
            if (on && !AgentState.locationReady) {
                locationPermission.launch(Manifest.permission.ACCESS_FINE_LOCATION)
            }
        },
        onViewScreen = { extend ->
            startActivity(Intent(this, ViewActivity::class.java).putExtra(ViewActivity.EXTEND, extend))
        },
        onPrepareCapture = {
            val manager = getSystemService(MediaProjectionManager::class.java)
            captureConsent.launch(manager.createScreenCaptureIntent())
        },
    )

    private fun change(update: () -> Unit) {
        update()
        SharePrefs.save(this)
        AgentService.sharingChanged()
    }

    private fun requireMicrophone() {
        if (!AgentState.micReady) microphonePermission.launch(Manifest.permission.RECORD_AUDIO)
    }

    private fun requestStorage() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            startActivity(
                Intent(
                    Settings.ACTION_MANAGE_APP_ALL_FILES_ACCESS_PERMISSION,
                    Uri.parse("package:$packageName"),
                ),
            )
        } else {
            storagePermission.launch(Manifest.permission.WRITE_EXTERNAL_STORAGE)
        }
    }

    private fun startAgent() {
        if (Build.VERSION.SDK_INT >= 33 && checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED) {
            notificationPermission.launch(Manifest.permission.POST_NOTIFICATIONS)
        }
        val intent = Intent(this, AgentService::class.java)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            startForegroundService(intent)
        } else {
            startService(intent)
        }
    }

    private fun stopAgent() {
        stopService(Intent(this, AgentService::class.java))
    }

    private fun refreshCapabilities() {
        val services = getSystemService(AccessibilityManager::class.java)
            .getEnabledAccessibilityServiceList(AccessibilityServiceInfo.FEEDBACK_ALL_MASK)
        AgentState.accessibilityReady = services.any {
            it.resolveInfo.serviceInfo.packageName == packageName
        }
        val keyboards = getSystemService(InputMethodManager::class.java).enabledInputMethodList
        AgentState.keyboardReady = keyboards.any { it.packageName == packageName }
        AgentState.micReady =
            checkSelfPermission(Manifest.permission.RECORD_AUDIO) == PackageManager.PERMISSION_GRANTED
        AgentState.cameraReady = hasPermission(this, Manifest.permission.CAMERA)
        AgentState.locationReady = hasPermission(this, Manifest.permission.ACCESS_FINE_LOCATION)
        AgentState.storageReady = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            Environment.isExternalStorageManager()
        } else {
            checkSelfPermission(Manifest.permission.WRITE_EXTERNAL_STORAGE) ==
                PackageManager.PERMISSION_GRANTED
        }
    }
}
