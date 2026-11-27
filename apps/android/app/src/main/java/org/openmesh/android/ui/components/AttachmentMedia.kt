package org.openmesh.android.ui.components

import android.graphics.Bitmap
import android.graphics.BitmapFactory
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.BrokenImage
import androidx.compose.material.icons.filled.Close
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.produceState
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.openmesh.android.media.AttachmentUiPolicy
import org.openmesh.android.ui.theme.MeshOnSurfaceVariant
import org.openmesh.android.ui.theme.MeshOutline
import org.openmesh.android.ui.theme.MeshPrimary
import org.openmesh.android.ui.theme.MeshSurfaceHigh

/** Async decode state for one attachment image. */
sealed interface AttachmentImageState {
    data object Loading : AttachmentImageState
    data class Ready(val bitmap: Bitmap) : AttachmentImageState
    data class Failed(val message: String) : AttachmentImageState
}

/**
 * Decodes a stored attachment image (WebP/JPEG/PNG) off the UI thread, downsampled so a large
 * camera photo cannot exhaust memory. Returns null when the file is missing/undecodable or the
 * platform has no codec for the format.
 */
suspend fun decodeAttachmentBitmap(
    file: java.io.File,
    maxEdge: Int = AttachmentUiPolicy.IMAGE_MAX_EDGE_PX,
): Bitmap? = withContext(Dispatchers.IO) {
    if (!file.isFile) return@withContext null
    val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
    BitmapFactory.decodeFile(file.absolutePath, bounds)
    if (bounds.outWidth <= 0 || bounds.outHeight <= 0) return@withContext null
    val options = BitmapFactory.Options().apply {
        inSampleSize = AttachmentUiPolicy.inSampleSize(bounds.outWidth, bounds.outHeight, maxEdge)
        inPreferredConfig = Bitmap.Config.ARGB_8888
    }
    runCatching { BitmapFactory.decodeFile(file.absolutePath, options) }.getOrNull()
}

/**
 * Inline image preview inside a message bubble. Shows a spinner while decoding and a readable
 * fallback (never a dead end) when the file is missing or cannot be decoded. Tapping a decoded
 * image opens the full-screen [AttachmentImageViewerDialog].
 */
@Composable
fun AttachmentThumbnail(
    file: java.io.File?,
    contentDescription: String,
    onOpen: (Bitmap) -> Unit,
    modifier: Modifier = Modifier,
) {
    val state by produceState<AttachmentImageState>(
        initialValue = AttachmentImageState.Loading,
        key1 = file?.absolutePath,
        key2 = file?.lastModified(),
    ) {
        value = if (file == null || !file.isFile) {
            AttachmentImageState.Failed(AttachmentUiPolicy.imageFallbackText(fileExists = false))
        } else {
            val bitmap = runCatching { decodeAttachmentBitmap(file) }.getOrNull()
            if (bitmap != null) {
                AttachmentImageState.Ready(bitmap)
            } else {
                AttachmentImageState.Failed(AttachmentUiPolicy.imageFallbackText(fileExists = true))
            }
        }
    }
    when (val current = state) {
        AttachmentImageState.Loading -> Box(
            modifier
                .fillMaxWidth()
                .height(140.dp)
                .clip(RoundedCornerShape(12.dp))
                .background(MeshOutline.copy(alpha = 0.35f)),
            contentAlignment = Alignment.Center,
        ) {
            CircularProgressIndicator(
                modifier = Modifier.size(24.dp),
                color = MeshPrimary,
                strokeWidth = 2.dp,
            )
        }
        is AttachmentImageState.Failed -> Row(
            modifier
                .fillMaxWidth()
                .clip(RoundedCornerShape(12.dp))
                .background(MeshOutline.copy(alpha = 0.35f))
                .padding(horizontal = 12.dp, vertical = 14.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(
                Icons.Filled.BrokenImage,
                contentDescription = null,
                tint = MeshOnSurfaceVariant,
                modifier = Modifier.size(22.dp),
            )
            Spacer(Modifier.width(10.dp))
            Text(
                current.message,
                style = MaterialTheme.typography.bodyMedium,
                color = MeshOnSurfaceVariant,
            )
        }
        is AttachmentImageState.Ready -> BoxWithConstraints(
            modifier = modifier.fillMaxWidth(),
            contentAlignment = Alignment.Center,
        ) {
            // Scale the thumbnail up to the bubble width, capped by the max height, so a small
            // 240x320 WebP still reads as a photo instead of a postage stamp.
            val maxHeight = AttachmentUiPolicy.THUMBNAIL_MAX_HEIGHT_DP.dp
            val ratio = current.bitmap.width.toFloat() / current.bitmap.height.coerceAtLeast(1)
            val width = if (maxWidth / ratio > maxHeight) maxHeight * ratio else maxWidth
            val height = if (maxWidth / ratio > maxHeight) maxHeight else maxWidth / ratio
            Image(
                bitmap = current.bitmap.asImageBitmap(),
                contentDescription = contentDescription,
                contentScale = ContentScale.Fit,
                modifier = Modifier
                    .width(width)
                    .height(height)
                    .clip(RoundedCornerShape(12.dp))
                    .clickable { onOpen(current.bitmap) },
            )
        }
    }
}

/**
 * Full-screen image viewer opened by tapping a thumbnail: black background, fit-to-screen image,
 * explicit close action and back-press dismissal.
 */
@Composable
fun AttachmentImageViewerDialog(
    bitmap: Bitmap,
    contentDescription: String,
    onDismiss: () -> Unit,
) {
    Dialog(
        onDismissRequest = onDismiss,
        properties = DialogProperties(usePlatformDefaultWidth = false),
    ) {
        Box(
            Modifier
                .fillMaxSize()
                .background(Color.Black)
                .padding(12.dp),
        ) {
            Image(
                bitmap = bitmap.asImageBitmap(),
                contentDescription = contentDescription,
                contentScale = ContentScale.Fit,
                modifier = Modifier.fillMaxSize(),
            )
            IconButton(
                onClick = onDismiss,
                modifier = Modifier
                    .align(Alignment.TopEnd)
                    .background(MeshSurfaceHigh.copy(alpha = 0.85f), RoundedCornerShape(50)),
            ) {
                Icon(Icons.Filled.Close, contentDescription = "Close image", tint = Color.White)
            }
        }
    }
}

/** Small row used by non-image, non-audio attachments in a bubble. */
@Composable
fun FileAttachmentRow(name: String, detail: String, modifier: Modifier = Modifier) {
    Row(
        modifier = modifier.fillMaxWidth(),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        Text(
            name,
            style = MaterialTheme.typography.titleMedium,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.weight(1f),
        )
        Text(detail, style = MaterialTheme.typography.labelMedium, color = MeshOnSurfaceVariant)
    }
}
