package io.github.bytaymur.mause

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Home
import androidx.compose.material.icons.rounded.Settings
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Icon
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.NavigationBarItemDefaults
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

enum class Tab(val title: String, val icon: ImageVector) {
    Home("Ana sayfa", Icons.Rounded.Home),
    Settings("Ayarlar", Icons.Rounded.Settings),
}

@Composable
fun MauseApp(
    version: String,
    onStart: () -> Unit,
    onStop: () -> Unit,
    onPairAnswer: (Boolean) -> Unit,
    share: ShareActions,
    system: SystemActions,
) {
    var tab by rememberSaveable { mutableStateOf(Tab.Home) }
    var section by rememberSaveable { mutableStateOf<SettingsSection?>(null) }

    val openSection: (SettingsSection) -> Unit = {
        section = it
        tab = Tab.Settings
    }

    BackHandler(enabled = section != null || tab != Tab.Home) {
        if (section != null) section = null else tab = Tab.Home
    }

    Scaffold(
        containerColor = MauseColors.Ground,
        bottomBar = {
            NavigationBar(containerColor = MauseColors.Surface, tonalElevation = 0.dp) {
                Tab.entries.forEach { item ->
                    NavigationBarItem(
                        selected = tab == item,
                        onClick = {
                            if (tab == item) section = null
                            tab = item
                        },
                        icon = { Icon(item.icon, contentDescription = null) },
                        label = { Text(item.title, fontWeight = FontWeight.Medium) },
                        colors = NavigationBarItemDefaults.colors(
                            selectedIconColor = MauseColors.OnAccent,
                            selectedTextColor = MauseColors.Ink,
                            indicatorColor = MauseColors.Accent,
                            unselectedIconColor = MauseColors.Muted,
                            unselectedTextColor = MauseColors.Muted,
                        ),
                    )
                }
            }
        },
    ) { padding ->
        Column(
            Modifier
                .fillMaxSize()
                .padding(padding)
                .verticalScroll(rememberScrollState())
                .padding(horizontal = 20.dp, vertical = 20.dp),
            verticalArrangement = Arrangement.spacedBy(14.dp),
        ) {
            val current = section
            when {
                tab == Tab.Home -> HomeScreen(version, share, onStart, onStop, openSection)
                current != null -> SettingsDetail(current, version, share, system) { section = null }
                else -> SettingsMenu(version, openSection)
            }
        }
    }

    AgentState.pairRequest?.let { request ->
        AlertDialog(
            onDismissRequest = {},
            containerColor = MauseColors.Surface,
            shape = RoundedCornerShape(20.dp),
            title = { Text("Bilgisayarı eşleştir", color = MauseColors.Ink) },
            text = {
                Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                    Text("${request.computer} bu cihazı kontrol etmek istiyor.", color = MauseColors.Ink2)
                    Text(request.code, fontSize = 32.sp, fontWeight = FontWeight.SemiBold, color = MauseColors.Accent)
                    Text("Aynı kod bilgisayarda görünüyorsa onaylayın.", color = MauseColors.Muted)
                }
            },
            confirmButton = { PrimaryButton("Eşleştir", { onPairAnswer(true) }) },
            dismissButton = { SecondaryButton("Reddet", { onPairAnswer(false) }) },
        )
    }
}
