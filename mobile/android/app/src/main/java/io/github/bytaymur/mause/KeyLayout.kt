package io.github.bytaymur.mause

import java.util.Locale

enum class KeyLayout(val title: String, val locale: Locale) {
    TurkishQ("Türkçe Q", Locale.forLanguageTag("tr")),
    English("İngilizce", Locale.US);

    fun character(code: Int, shift: Boolean, altGr: Boolean, caps: Boolean): String? {
        val keys = if (this == TurkishQ) TURKISH else ENGLISH
        val key = keys[code] ?: NUMPAD[code]?.let { return it } ?: return null
        if (altGr) return key.altGr
        val letter = key.normal.length == 1 && key.normal[0].isLetter()
        val upper = if (letter) shift != caps else shift
        if (!upper) return key.normal
        return key.shifted ?: key.normal.uppercase(locale)
    }

    private class Key(val normal: String, val shifted: String? = null, val altGr: String? = null)

    companion object {
        fun fromName(name: String?) = entries.firstOrNull { it.name == name } ?: TurkishQ

        private val NUMPAD = mapOf(
            71 to "7", 72 to "8", 73 to "9", 74 to "-", 75 to "4", 76 to "5", 77 to "6",
            78 to "+", 79 to "1", 80 to "2", 81 to "3", 82 to "0", 83 to ",", 98 to "/", 55 to "*",
        )

        private fun letters(codes: IntRange, text: String) =
            codes.zip(text.map { Key(it.toString()) })

        private val ENGLISH: Map<Int, Key> = buildMap {
            "1234567890".zip("!@#$%^&*()").forEachIndexed { index, (normal, shifted) ->
                put(2 + index, Key(normal.toString(), shifted.toString()))
            }
            put(12, Key("-", "_"))
            put(13, Key("=", "+"))
            putAll(letters(16..25, "qwertyuiop"))
            put(26, Key("[", "{"))
            put(27, Key("]", "}"))
            putAll(letters(30..38, "asdfghjkl"))
            put(39, Key(";", ":"))
            put(40, Key("'", "\""))
            put(41, Key("`", "~"))
            put(43, Key("\\", "|"))
            putAll(letters(44..50, "zxcvbnm"))
            put(51, Key(",", "<"))
            put(52, Key(".", ">"))
            put(53, Key("/", "?"))
            put(86, Key("\\", "|"))
            put(57, Key(" ", " ", " "))
        }

        private val TURKISH: Map<Int, Key> = buildMap {
            val shifted = "!'^+%&/()="
            val altGr = listOf(">", "£", "#", "$", "½", null, "{", "[", "]", "}")
            "1234567890".forEachIndexed { index, normal ->
                put(2 + index, Key(normal.toString(), shifted[index].toString(), altGr[index]))
            }
            put(12, Key("*", "?", "\\"))
            put(13, Key("-", "_", "|"))
            putAll(letters(16..25, "qwertyuıop"))
            put(16, Key("q", altGr = "@"))
            put(18, Key("e", altGr = "€"))
            put(20, Key("t", altGr = "₺"))
            put(26, Key("ğ", altGr = "¨"))
            put(27, Key("ü", altGr = "~"))
            putAll(letters(30..38, "asdfghjkl"))
            put(39, Key("ş", altGr = "´"))
            put(40, Key("i", "İ"))
            put(41, Key("\"", "é", "<"))
            put(43, Key(",", ";", "`"))
            putAll(letters(44..50, "zxcvbnm"))
            put(51, Key("ö"))
            put(52, Key("ç"))
            put(53, Key(".", ":"))
            put(86, Key("<", ">", "|"))
            put(57, Key(" ", " ", " "))
        }
    }
}
