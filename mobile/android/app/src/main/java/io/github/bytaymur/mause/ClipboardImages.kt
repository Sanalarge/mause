package io.github.bytaymur.mause

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.net.Uri
import androidx.core.content.FileProvider
import java.io.ByteArrayOutputStream
import java.io.File

data class ClipboardPreview(val uri: Uri, val bitmap: Bitmap)

object ClipboardImages {
    private const val MAX_BYTES = 48 * 1024 * 1024
    private const val MAX_PIXELS = 36_000_000L
    private const val MAX_DIMENSION = 16_384

    fun copy(context: Context, uri: Uri) {
        val clip = ClipData.newUri(context.contentResolver, "Mause", uri)
        context.getSystemService(ClipboardManager::class.java).setPrimaryClip(clip)
    }

    fun preview(context: Context, uri: Uri): ClipboardPreview {
        val resolver = context.contentResolver
        val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
        resolver.openInputStream(uri)?.use { BitmapFactory.decodeStream(it, null, bounds) }
        validate(bounds)
        val bitmap = resolver.openInputStream(uri)?.use {
            BitmapFactory.decodeStream(it, null, previewOptions(bounds))
        } ?: error("Panodaki görsel okunamadı")
        return ClipboardPreview(uri, bitmap)
    }

    fun read(context: Context, uri: Uri): ByteArray {
        val resolver = context.contentResolver
        val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
        resolver.openInputStream(uri)?.use { BitmapFactory.decodeStream(it, null, bounds) }
        validate(bounds)
        if (bounds.outMimeType == "image/png") {
            return resolver.openInputStream(uri)?.use { source ->
                val output = LimitedBuffer()
                source.copyTo(output)
                output.toByteArray()
            } ?: error("Panodaki görsel açılamadı")
        }
        val bitmap = resolver.openInputStream(uri)?.use { BitmapFactory.decodeStream(it) }
            ?: error("Görsel okunamadı")
        try {
            val output = LimitedBuffer()
            check(bitmap.compress(Bitmap.CompressFormat.PNG, 100, output)) { "Görsel dönüştürülemedi" }
            return output.toByteArray()
        } finally {
            bitmap.recycle()
        }
    }

    @Synchronized
    fun store(context: Context, png: ByteArray): ClipboardPreview {
        require(png.size <= MAX_BYTES) { "Görsel çok büyük" }
        val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
        BitmapFactory.decodeByteArray(png, 0, png.size, bounds)
        validate(bounds)
        require(bounds.outMimeType == "image/png") { "Geçersiz pano görseli" }
        val bitmap = BitmapFactory.decodeByteArray(png, 0, png.size, previewOptions(bounds))
            ?: error("Görsel okunamadı")
        val directory = File(context.cacheDir, "clipboard").apply { mkdirs() }
        val existing = directory.listFiles().orEmpty().sortedByDescending { it.lastModified() }
        val file = File.createTempFile("image-", ".png", directory)
        try {
            file.writeBytes(png)
        } catch (error: Exception) {
            file.delete()
            throw error
        }
        existing.drop(7).forEach { it.delete() }
        val uri = FileProvider.getUriForFile(context, "${context.packageName}.clipboard", file)
        return ClipboardPreview(uri, bitmap)
    }

    private fun previewOptions(bounds: BitmapFactory.Options) = BitmapFactory.Options().apply {
        inSampleSize = 1
        while (maxOf(bounds.outWidth, bounds.outHeight) / inSampleSize > 640) {
            inSampleSize *= 2
        }
    }

    private fun validate(bounds: BitmapFactory.Options) {
        require(
            bounds.outWidth in 1..MAX_DIMENSION &&
                bounds.outHeight in 1..MAX_DIMENSION &&
                bounds.outWidth.toLong() * bounds.outHeight <= MAX_PIXELS,
        ) { "Geçersiz veya çok büyük pano görseli" }
    }

    private class LimitedBuffer : ByteArrayOutputStream() {
        override fun write(bytes: ByteArray, start: Int, length: Int) {
            require(length <= MAX_BYTES - size()) { "Görsel çok büyük" }
            super.write(bytes, start, length)
        }

        override fun write(byte: Int) {
            require(size() < MAX_BYTES) { "Görsel çok büyük" }
            super.write(byte)
        }
    }
}
