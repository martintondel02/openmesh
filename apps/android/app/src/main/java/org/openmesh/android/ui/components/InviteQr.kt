package org.openmesh.android.ui.components

import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.runtime.Composable
import org.openmesh.android.media.QrRender

/** Renders an `openmesh://…` invite as a scannable QR image. */
@Composable
fun InviteQr(content: String, size: Dp = 220.dp) {
    val bitmap = remember(content) { QrRender.bitmap(content, 720) }
    Image(
        bitmap = bitmap.asImageBitmap(),
        contentDescription = "Invite QR code",
        modifier = Modifier
            .size(size)
            .clip(RoundedCornerShape(12.dp))
            .background(Color.White)
            .padding(6.dp),
    )
}
