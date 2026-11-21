package io.github.bytaymur.mause

import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Typography
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.Font
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight

object MauseColors {
    val Ground = Color(0xFF0D111C)
    val Surface = Color(0xFF121826)
    val Inset = Color(0xFF182133)
    val Selected = Color(0xFF1B273C)
    val Rule = Color(0xFF232D40)
    val RuleStrong = Color(0xFF313D54)
    val Ink = Color(0xFFF5F7FB)
    val Ink2 = Color(0xFFC2CBDA)
    val Muted = Color(0xFFA7B1C2)
    val Accent = Color(0xFFFF9500)
    val OnAccent = Color(0xFF121214)
    val Ok = Color(0xFFC6F432)
    val Off = Color(0xFFFF6A3D)
    val Bad = Color(0xFFFF6B6B)
}

val Poppins = FontFamily(
    Font(R.font.poppins_regular, FontWeight.Normal),
    Font(R.font.poppins_medium, FontWeight.Medium),
    Font(R.font.poppins_semibold, FontWeight.SemiBold),
    Font(R.font.poppins_semibold, FontWeight.Bold),
)

private fun TextStyle.poppins() = copy(fontFamily = Poppins)

private fun mauseTypography(): Typography {
    val base = Typography()
    return Typography(
        displayLarge = base.displayLarge.poppins(),
        displayMedium = base.displayMedium.poppins(),
        displaySmall = base.displaySmall.poppins(),
        headlineLarge = base.headlineLarge.poppins(),
        headlineMedium = base.headlineMedium.poppins(),
        headlineSmall = base.headlineSmall.poppins(),
        titleLarge = base.titleLarge.poppins(),
        titleMedium = base.titleMedium.poppins(),
        titleSmall = base.titleSmall.poppins(),
        bodyLarge = base.bodyLarge.poppins(),
        bodyMedium = base.bodyMedium.poppins(),
        bodySmall = base.bodySmall.poppins(),
        labelLarge = base.labelLarge.poppins(),
        labelMedium = base.labelMedium.poppins(),
        labelSmall = base.labelSmall.poppins(),
    )
}

@Composable
fun MauseTheme(content: @Composable () -> Unit) {
    MaterialTheme(
        colorScheme = darkColorScheme(
            primary = MauseColors.Accent,
            onPrimary = MauseColors.OnAccent,
            secondary = MauseColors.Accent,
            onSecondary = MauseColors.OnAccent,
            background = MauseColors.Ground,
            onBackground = MauseColors.Ink,
            surface = MauseColors.Surface,
            onSurface = MauseColors.Ink,
            surfaceVariant = MauseColors.Inset,
            onSurfaceVariant = MauseColors.Muted,
            surfaceContainer = MauseColors.Surface,
            surfaceContainerHigh = MauseColors.Inset,
            surfaceContainerHighest = MauseColors.Inset,
            outline = MauseColors.RuleStrong,
            outlineVariant = MauseColors.Rule,
            error = MauseColors.Bad,
        ),
        typography = mauseTypography(),
        content = content,
    )
}
