package org.openmesh.android.data.db

import android.content.Context
import androidx.room.Dao
import androidx.room.Database
import androidx.room.Entity
import androidx.room.Insert
import androidx.room.OnConflictStrategy
import androidx.room.PrimaryKey
import androidx.room.Query
import androidx.room.Room
import androidx.room.RoomDatabase
import org.openmesh.android.data.model.Attachment
import org.openmesh.android.data.model.Contact
import org.openmesh.android.data.model.DeliveryState
import org.openmesh.android.data.model.Message
import org.openmesh.android.data.model.MessageKind
import org.openmesh.android.data.model.NodeClass
import org.openmesh.android.data.model.NodeInfo
import org.openmesh.android.data.model.Position

@Entity(tableName = "messages")
data class MessageEntity(
    @PrimaryKey val id: Long,
    val peerId: String,
    val fromId: String,
    val toId: String,
    val kind: String,
    val text: String?,
    val sentAtMs: Long,
    val receivedAtMs: Long?,
    val outgoing: Boolean,
    val delivery: String,
    val read: Boolean,
    val attachmentId: String?,
    val attachmentMime: String?,
    val attachmentSize: Int?,
    val attachmentName: String?,
    val attachmentDurationMs: Int?,
    val positionLat: Double?,
    val positionLon: Double?,
    val viaStore: Boolean,
) {
    fun toModel(): Message = Message(
        id = id,
        peerId = peerId,
        fromId = fromId,
        toId = toId,
        kind = runCatching { MessageKind.valueOf(kind) }.getOrDefault(MessageKind.TEXT),
        text = text,
        sentAtMs = sentAtMs,
        receivedAtMs = receivedAtMs,
        outgoing = outgoing,
        delivery = runCatching { DeliveryState.valueOf(delivery) }.getOrDefault(DeliveryState.QUEUED),
        read = read,
        attachment = if (attachmentId != null) {
            Attachment(
                id = attachmentId,
                mime = attachmentMime ?: "application/octet-stream",
                sizeBytes = attachmentSize ?: 0,
                durationMs = attachmentDurationMs,
                name = attachmentName,
                progress = 1f,
            )
        } else {
            null
        },
        position = if (positionLat != null && positionLon != null) {
            Position(lat = positionLat, lon = positionLon, timestampMs = sentAtMs)
        } else {
            null
        },
        viaStore = viaStore,
    )

    companion object {
        fun fromModel(m: Message): MessageEntity = MessageEntity(
            id = m.id,
            peerId = m.peerId,
            fromId = m.fromId,
            toId = m.toId,
            kind = m.kind.name,
            text = m.text,
            sentAtMs = m.sentAtMs,
            receivedAtMs = m.receivedAtMs,
            outgoing = m.outgoing,
            delivery = m.delivery.name,
            read = m.read,
            attachmentId = m.attachment?.id,
            attachmentMime = m.attachment?.mime,
            attachmentSize = m.attachment?.sizeBytes,
            attachmentName = m.attachment?.name,
            attachmentDurationMs = m.attachment?.durationMs,
            positionLat = m.position?.lat,
            positionLon = m.position?.lon,
            viaStore = m.viaStore,
        )
    }
}

@Entity(tableName = "contacts")
data class ContactEntity(
    @PrimaryKey val nodeId: String,
    val name: String,
    val identityBase64: String,
    val lastSeenMs: Long,
    val verified: Boolean,
) {
    fun toModel(): Contact = Contact(nodeId, name, identityBase64, lastSeenMs, verified)
}

@Entity(tableName = "channels")
data class ChannelEntity(
    @PrimaryKey val id: String,
    val name: String,
    val pskSet: Boolean,
    val inviteUrl: String?,
)

@Entity(tableName = "nodes")
data class NodeEntity(
    @PrimaryKey val nodeId: String,
    val name: String,
    val nodeClass: String,
    val caps: String,
    val hops: Int,
    val lastSeenMs: Long,
    val rssi: Int?,
    val snr: Float?,
    val batteryPct: Int?,
    val lat: Double?,
    val lon: Double?,
    val isContact: Boolean,
) {
    fun toModel(): NodeInfo = NodeInfo(
        nodeId = nodeId,
        name = name,
        nodeClass = NodeClass.fromWire(nodeClass),
        caps = caps.split(',').filter { it.isNotBlank() }.toSet(),
        hops = hops,
        lastSeenMs = lastSeenMs,
        rssi = rssi,
        snr = snr,
        batteryPct = batteryPct,
        position = if (lat != null && lon != null) Position(lat = lat, lon = lon, timestampMs = lastSeenMs) else null,
        isContact = isContact,
    )

    companion object {
        fun fromModel(n: NodeInfo): NodeEntity = NodeEntity(
            nodeId = n.nodeId,
            name = n.name,
            nodeClass = n.nodeClass.name.lowercase(),
            caps = n.caps.joinToString(","),
            hops = n.hops,
            lastSeenMs = n.lastSeenMs,
            rssi = n.rssi,
            snr = n.snr,
            batteryPct = n.batteryPct,
            lat = n.position?.lat,
            lon = n.position?.lon,
            isContact = n.isContact,
        )
    }
}

@Dao
interface MeshDao {
    @Query("SELECT * FROM messages ORDER BY sentAtMs ASC")
    suspend fun messages(): List<MessageEntity>

    @Query("SELECT * FROM messages WHERE peerId = :peerId ORDER BY sentAtMs ASC")
    suspend fun thread(peerId: String): List<MessageEntity>

    @Insert(onConflict = OnConflictStrategy.REPLACE)
    suspend fun upsertMessage(message: MessageEntity)

    @Query("UPDATE messages SET delivery = :delivery, read = :read WHERE id = :id")
    suspend fun updateDelivery(id: Long, delivery: String, read: Boolean)

    @Query("DELETE FROM messages WHERE id = :id")
    suspend fun deleteMessage(id: Long)

    @Query("UPDATE messages SET read = 1 WHERE peerId = :peerId")
    suspend fun markThreadRead(peerId: String)

    @Query("SELECT * FROM contacts ORDER BY name")
    suspend fun contacts(): List<ContactEntity>

    @Insert(onConflict = OnConflictStrategy.REPLACE)
    suspend fun upsertContact(contact: ContactEntity)

    @Query("DELETE FROM contacts WHERE nodeId = :nodeId")
    suspend fun deleteContact(nodeId: String)

    @Query("SELECT * FROM channels ORDER BY name")
    suspend fun channels(): List<ChannelEntity>

    @Insert(onConflict = OnConflictStrategy.REPLACE)
    suspend fun upsertChannel(channel: ChannelEntity)

    @Query("DELETE FROM channels WHERE id = :id")
    suspend fun deleteChannel(id: String)

    @Query("SELECT * FROM nodes ORDER BY lastSeenMs DESC")
    suspend fun nodes(): List<NodeEntity>

    @Insert(onConflict = OnConflictStrategy.REPLACE)
    suspend fun upsertNode(node: NodeEntity)
}

@Database(
    entities = [MessageEntity::class, ContactEntity::class, ChannelEntity::class, NodeEntity::class],
    version = 1,
    exportSchema = false,
)
abstract class OpenMeshDatabase : RoomDatabase() {
    abstract fun dao(): MeshDao

    companion object {
        @Volatile
        private var instance: OpenMeshDatabase? = null

        fun get(context: Context): OpenMeshDatabase =
            instance ?: synchronized(this) {
                instance ?: Room.databaseBuilder(
                    context.applicationContext,
                    OpenMeshDatabase::class.java,
                    "openmesh.db",
                ).fallbackToDestructiveMigration().build().also { instance = it }
            }
    }
}
