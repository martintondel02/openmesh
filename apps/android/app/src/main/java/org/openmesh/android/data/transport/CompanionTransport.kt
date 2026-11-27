package org.openmesh.android.data.transport

import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.StateFlow

/**
 * A byte-stream transport to an OpenMesh radio running the companion API
 * (docs/architecture/COMPANION-API.md).
 *
 * Implementations emit raw stream chunks; [FramedCompanionClient] owns SLIP de-framing,
 * CRC validation and CBOR decoding. Transports never interpret frames.
 */
interface CompanionTransport {
    /** Human-readable name shown in the UI ("USB serial", "BLE", "demo"). */
    val label: String

    /** True while the underlying link is usable for writes. */
    val connected: StateFlow<Boolean>

    /**
     * Human-readable progress of the current link attempt (scan → connect → handshake), or the
     * persistent failure reason. READY is only set by the repository after the companion HelloAck.
     */
    val progress: StateFlow<LinkProgress>

    /** Raw bytes as they arrive; may split or coalesce frames arbitrarily. */
    val incoming: Flow<ByteArray>

    /** Connect (may prompt for permissions). Returns a failure with a user-readable reason. */
    suspend fun connect(): Result<Unit>

    /** Close the link; safe to call repeatedly. */
    suspend fun disconnect()

    /** Write raw bytes. Throws on a closed/failed link. */
    suspend fun write(bytes: ByteArray)
}
