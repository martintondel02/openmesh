//! Persistent bridge storage on redb (pure-Rust embedded database, no C dependencies).
//!
//! Tables:
//! * `history`     — ordered index of every message by (scope, peer/channel, sent_at, msg_id)
//! * `messages`    — message body by msg_id (fast ack/retry/read lookups)
//! * `contacts`    — known peers with their 72-byte identity
//! * `channels`    — channel name + PSK (so restarts keep group keys)
//! * `nodes`       — last announced node metadata for the UI
//! * `attachments` — attachment metadata (bytes live in the data dir)
//! * `meta`        — bridge configuration and small counters
//!
//! Message records are stored as JSON: the bridge API is JSON anyway and this keeps the
//! on-disk format debuggable with standard tools.

use std::collections::HashSet;
use std::path::Path;

use anyhow::{Context, Result};
use redb::{Database, ReadableTableMetadata, TableDefinition};
use serde::{Deserialize, Serialize};

use openmesh_core::{MessageKind, NodeId, Position};

use crate::json::{AttachmentJson, GroupMessageJson, MessageJson, NodeJson, b64, b64_decode};

const HISTORY: TableDefinition<(u8, u64, u64, u32), &[u8]> = TableDefinition::new("history");
const MESSAGES: TableDefinition<u32, &[u8]> = TableDefinition::new("messages");
const CONTACTS: TableDefinition<u64, &[u8]> = TableDefinition::new("contacts");
const CHANNELS: TableDefinition<u64, &[u8]> = TableDefinition::new("channels");
const NODES: TableDefinition<u64, &[u8]> = TableDefinition::new("nodes");
const ATTACHMENTS: TableDefinition<&str, &[u8]> = TableDefinition::new("attachments");
const META: TableDefinition<&str, &[u8]> = TableDefinition::new("meta");

/// Scope discriminator used in the `history` index.
const SCOPE_DIRECT: u8 = 0;
const SCOPE_CHANNEL: u8 = 1;

#[inline]
fn id_key(id: NodeId) -> u64 {
    u64::from_be_bytes(id.0)
}

#[inline]
fn key_id(key: u64) -> NodeId {
    NodeId(key.to_be_bytes())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StoredAttachment {
    pub id: String,
    pub mime: String,
    pub size: u32,
    pub sha256: String,
    pub duration_ms: Option<u32>,
    pub name: Option<String>,
    /// 0.0..=1.0; 1.0 once the resource transfer completed.
    pub progress: f32,
}

impl StoredAttachment {
    pub fn to_json(&self) -> AttachmentJson {
        AttachmentJson {
            id: self.id.clone(),
            mime: self.mime.clone(),
            size: self.size,
            sha256: Some(self.sha256.clone()),
            duration_ms: self.duration_ms,
            name: self.name.clone(),
            progress: self.progress as f64,
        }
    }
}

/// One stored direct or group message.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StoredMessage {
    pub msg_id: u32,
    /// Direct conversation partner (recipient for outgoing, sender for incoming).
    #[serde(default)]
    pub peer: Option<NodeId>,
    #[serde(default)]
    pub channel_id: Option<NodeId>,
    #[serde(default)]
    pub channel_name: Option<String>,
    pub from: NodeId,
    pub to: NodeId,
    #[serde(default)]
    pub from_name: Option<String>,
    pub kind: MessageKind,
    #[serde(default)]
    pub text: Option<String>,
    pub sent_at_ms: u64,
    pub received_at_ms: u64,
    pub outgoing: bool,
    pub delivered: bool,
    pub read: bool,
    #[serde(default)]
    pub reply_to: Option<u32>,
    #[serde(default)]
    pub thread: Option<u32>,
    #[serde(default)]
    pub attachment: Option<StoredAttachment>,
    #[serde(default)]
    pub position: Option<Position>,
    /// Group messages only: whether the sender signature verified.
    #[serde(default)]
    pub signature_valid: bool,
    /// Group messages only: base64 72-byte sender identity from the inner envelope.
    #[serde(default)]
    pub sender_key: Option<String>,
    /// True when the envelope was replayed from a relay's store-and-forward queue.
    #[serde(default)]
    pub via_store: bool,
}

impl StoredMessage {
    fn index_key(&self) -> (u8, u64, u64, u32) {
        if let Some(channel) = self.channel_id {
            (SCOPE_CHANNEL, id_key(channel), self.sent_at_ms, self.msg_id)
        } else {
            let peer = self.peer.unwrap_or(self.from);
            (SCOPE_DIRECT, id_key(peer), self.sent_at_ms, self.msg_id)
        }
    }

    pub fn is_group(&self) -> bool {
        self.channel_id.is_some()
    }

    pub fn to_message_json(&self) -> MessageJson {
        MessageJson {
            msg_id: self.msg_id,
            peer: self
                .peer
                .map(|p| p.to_hex())
                .unwrap_or_else(|| self.to.to_hex()),
            from: self.from.to_hex(),
            to: self.to.to_hex(),
            kind: crate::json::message_kind_str(self.kind).to_string(),
            text: self.text.clone(),
            sent_at_ms: self.sent_at_ms,
            received_at_ms: self.received_at_ms,
            outgoing: self.outgoing,
            delivered: self.delivered,
            read: self.read,
            reply_to: self.reply_to,
            thread: self.thread,
            attachment: self.attachment.as_ref().map(StoredAttachment::to_json),
            position: self
                .position
                .as_ref()
                .map(crate::json::PositionJson::from_core),
        }
    }

    pub fn to_group_message_json(&self) -> GroupMessageJson {
        GroupMessageJson {
            msg_id: self.msg_id,
            channel_id: self.channel_id.map(|c| c.to_hex()).unwrap_or_default(),
            channel_name: self.channel_name.clone().unwrap_or_default(),
            from: self.from.to_hex(),
            from_name: self.from_name.clone().unwrap_or_else(|| self.from.to_hex()),
            sender_key: self.sender_key.clone().unwrap_or_default(),
            kind: crate::json::message_kind_str(self.kind).to_string(),
            text: self.text.clone(),
            sent_at_ms: self.sent_at_ms,
            outgoing: self.outgoing,
            signature_valid: self.signature_valid,
            attachment: self.attachment.as_ref().map(StoredAttachment::to_json),
            position: self
                .position
                .as_ref()
                .map(crate::json::PositionJson::from_core),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ContactRecord {
    pub node_id: NodeId,
    pub name: Option<String>,
    /// Base64 of the 72-byte public identity.
    pub identity: String,
    pub last_seen_ms: u64,
    pub added_at_ms: u64,
}

impl ContactRecord {
    pub fn ident(&self) -> Option<openmesh_core::PublicIdentity> {
        let bytes = b64_decode(&self.identity)?;
        openmesh_core::PublicIdentity::from_bytes(&bytes).ok()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChannelRecord {
    pub channel_id: NodeId,
    pub name: String,
    /// Base64 32-byte PSK; never leaves the daemon except on `create_channel`.
    pub psk: String,
    pub created_at_ms: u64,
}

impl ChannelRecord {
    pub fn psk_bytes(&self) -> Option<[u8; 32]> {
        let raw = b64_decode(&self.psk)?;
        raw.try_into().ok()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AttachmentMeta {
    pub id: String,
    pub mime: String,
    pub size: u64,
    pub sha256: String,
    pub name: Option<String>,
    pub duration_ms: Option<u32>,
    pub created_at_ms: u64,
}

/// The redb-backed bridge store.
pub struct Store {
    db: Database,
    path: std::path::PathBuf,
}

impl Store {
    /// Open (or create) the database at `path`.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating data dir {}", parent.display()))?;
        }
        let db = Database::create(path)
            .with_context(|| format!("opening redb database {}", path.display()))?;
        // Create all tables up front so read-only transactions never miss one.
        let txn = db.begin_write()?;
        {
            txn.open_table(HISTORY)?;
            txn.open_table(MESSAGES)?;
            txn.open_table(CONTACTS)?;
            txn.open_table(CHANNELS)?;
            txn.open_table(NODES)?;
            txn.open_table(ATTACHMENTS)?;
            txn.open_table(META)?;
        }
        txn.commit()?;
        Ok(Self {
            db,
            path: path.to_path_buf(),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// On-disk size of the database file, used by the `storage.used_bytes` status field.
    pub fn db_bytes(&self) -> u64 {
        std::fs::metadata(&self.path).map(|m| m.len()).unwrap_or(0)
    }

    // -----------------------------------------------------------------------------------------
    // Messages
    // -----------------------------------------------------------------------------------------

    pub fn put_message(&self, msg: &StoredMessage) -> Result<()> {
        let body = serde_json::to_vec(msg)?;
        let key = msg.index_key();
        let txn = self.db.begin_write()?;
        {
            let mut history = txn.open_table(HISTORY)?;
            history.insert(key, body.as_slice())?;
            let mut messages = txn.open_table(MESSAGES)?;
            messages.insert(msg.msg_id, body.as_slice())?;
        }
        txn.commit()?;
        Ok(())
    }

    pub fn get_message(&self, msg_id: u32) -> Result<Option<StoredMessage>> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(MESSAGES)?;
        Ok(match table.get(msg_id)? {
            Some(value) => Some(serde_json::from_slice(value.value())?),
            None => None,
        })
    }

    /// Delete a message from both indexes.
    pub fn delete_message(&self, msg_id: u32) -> Result<bool> {
        let txn = self.db.begin_write()?;
        let mut removed = false;
        {
            let mut messages = txn.open_table(MESSAGES)?;
            if let Some(value) = messages.remove(msg_id)? {
                let msg: StoredMessage = serde_json::from_slice(value.value())?;
                removed = true;
                let mut history = txn.open_table(HISTORY)?;
                history.remove(msg.index_key())?;
            }
        }
        txn.commit()?;
        Ok(removed)
    }

    fn collect_range(
        &self,
        lo: (u8, u64, u64, u32),
        hi: (u8, u64, u64, u32),
        limit: usize,
    ) -> Result<Vec<StoredMessage>> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(HISTORY)?;
        let range = table.range(lo..=hi)?;
        // Newest first, then reverse so callers get chronological order.
        let mut out: Vec<StoredMessage> = Vec::new();
        for item in range.rev().take(limit) {
            let (_, value) = item?;
            out.push(serde_json::from_slice(value.value())?);
        }
        out.reverse();
        Ok(out)
    }

    /// Direct-message history with one peer, newest `limit` messages, chronological order.
    pub fn direct_history(
        &self,
        peer: NodeId,
        before_ms: Option<u64>,
        limit: usize,
    ) -> Result<Vec<StoredMessage>> {
        let upper = before_ms.map(|b| b.saturating_sub(1)).unwrap_or(u64::MAX);
        self.collect_range(
            (SCOPE_DIRECT, id_key(peer), 0, 0),
            (SCOPE_DIRECT, id_key(peer), upper, u32::MAX),
            limit.clamp(1, 1000),
        )
    }

    /// Channel history, newest `limit` messages, chronological order.
    pub fn channel_history(
        &self,
        channel: NodeId,
        before_ms: Option<u64>,
        limit: usize,
    ) -> Result<Vec<StoredMessage>> {
        let upper = before_ms.map(|b| b.saturating_sub(1)).unwrap_or(u64::MAX);
        self.collect_range(
            (SCOPE_CHANNEL, id_key(channel), 0, 0),
            (SCOPE_CHANNEL, id_key(channel), upper, u32::MAX),
            limit.clamp(1, 1000),
        )
    }

    /// All messages sent at or after `since_ms` (capped), oldest first. Used for `sync`.
    pub fn messages_since(&self, since_ms: u64, cap: usize) -> Result<Vec<StoredMessage>> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(HISTORY)?;
        let mut out = Vec::new();
        for item in table.range((0u8, 0u64, 0u64, 0u32)..=(255u8, u64::MAX, u64::MAX, u32::MAX))? {
            let (_, value) = item?;
            let msg: StoredMessage = serde_json::from_slice(value.value())?;
            if msg.sent_at_ms >= since_ms {
                out.push(msg);
                if out.len() >= cap {
                    break;
                }
            }
        }
        out.sort_by_key(|m| (m.sent_at_ms, m.msg_id));
        Ok(out)
    }

    pub fn message_count(&self) -> Result<usize> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(MESSAGES)?;
        Ok(table.len()? as usize)
    }

    /// Distinct direct peers that have messages, most recent first.
    pub fn direct_peers(&self) -> Result<Vec<NodeId>> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(HISTORY)?;
        let mut seen = HashSet::new();
        let mut peers = Vec::new();
        for item in table.range(
            (SCOPE_DIRECT, 0u64, 0u64, 0u32)..=(SCOPE_DIRECT, u64::MAX, u64::MAX, u32::MAX),
        )? {
            let (key, _) = item?;
            let peer = key.value().1;
            if seen.insert(peer) {
                peers.push(key_id(peer));
            }
        }
        Ok(peers)
    }

    // -----------------------------------------------------------------------------------------
    // Contacts
    // -----------------------------------------------------------------------------------------

    pub fn put_contact(&self, contact: &ContactRecord) -> Result<()> {
        let body = serde_json::to_vec(contact)?;
        let txn = self.db.begin_write()?;
        {
            let mut table = txn.open_table(CONTACTS)?;
            table.insert(id_key(contact.node_id), body.as_slice())?;
        }
        txn.commit()?;
        Ok(())
    }

    pub fn get_contact(&self, node: NodeId) -> Result<Option<ContactRecord>> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(CONTACTS)?;
        Ok(match table.get(id_key(node))? {
            Some(v) => Some(serde_json::from_slice(v.value())?),
            None => None,
        })
    }

    pub fn list_contacts(&self) -> Result<Vec<ContactRecord>> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(CONTACTS)?;
        let mut out = Vec::new();
        for item in table.range::<u64>(..)? {
            let (_, value) = item?;
            out.push(serde_json::from_slice(value.value())?);
        }
        out.sort_by_key(|c: &ContactRecord| c.added_at_ms);
        Ok(out)
    }

    pub fn remove_contact(&self, node: NodeId) -> Result<bool> {
        let txn = self.db.begin_write()?;
        let removed;
        {
            let mut table = txn.open_table(CONTACTS)?;
            removed = table.remove(id_key(node))?.is_some();
        }
        txn.commit()?;
        Ok(removed)
    }

    pub fn contact_count(&self) -> Result<usize> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(CONTACTS)?;
        Ok(table.len()? as usize)
    }

    // -----------------------------------------------------------------------------------------
    // Channels
    // -----------------------------------------------------------------------------------------

    pub fn put_channel(&self, channel: &ChannelRecord) -> Result<()> {
        let body = serde_json::to_vec(channel)?;
        let txn = self.db.begin_write()?;
        {
            let mut table = txn.open_table(CHANNELS)?;
            table.insert(id_key(channel.channel_id), body.as_slice())?;
        }
        txn.commit()?;
        Ok(())
    }

    pub fn get_channel(&self, channel: NodeId) -> Result<Option<ChannelRecord>> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(CHANNELS)?;
        Ok(match table.get(id_key(channel))? {
            Some(v) => Some(serde_json::from_slice(v.value())?),
            None => None,
        })
    }

    pub fn list_channels(&self) -> Result<Vec<ChannelRecord>> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(CHANNELS)?;
        let mut out = Vec::new();
        for item in table.range::<u64>(..)? {
            let (_, value) = item?;
            out.push(serde_json::from_slice(value.value())?);
        }
        out.sort_by_key(|c: &ChannelRecord| c.created_at_ms);
        Ok(out)
    }

    pub fn remove_channel(&self, channel: NodeId) -> Result<bool> {
        let txn = self.db.begin_write()?;
        let removed;
        {
            let mut table = txn.open_table(CHANNELS)?;
            removed = table.remove(id_key(channel))?.is_some();
        }
        txn.commit()?;
        Ok(removed)
    }

    pub fn channel_count(&self) -> Result<usize> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(CHANNELS)?;
        Ok(table.len()? as usize)
    }

    // -----------------------------------------------------------------------------------------
    // Nodes
    // -----------------------------------------------------------------------------------------

    pub fn put_node(&self, node: &NodeJson) -> Result<()> {
        let Some(id) = NodeId::from_hex(&node.node_id).ok() else {
            anyhow::bail!("invalid node id {}", node.node_id);
        };
        let body = serde_json::to_vec(node)?;
        let txn = self.db.begin_write()?;
        {
            let mut table = txn.open_table(NODES)?;
            table.insert(id_key(id), body.as_slice())?;
        }
        txn.commit()?;
        Ok(())
    }

    pub fn list_nodes(&self) -> Result<Vec<NodeJson>> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(NODES)?;
        let mut out = Vec::new();
        for item in table.range::<u64>(..)? {
            let (_, value) = item?;
            out.push(serde_json::from_slice(value.value())?);
        }
        out.sort_by_key(|n: &NodeJson| n.last_seen_ms);
        out.reverse();
        Ok(out)
    }

    pub fn node_count(&self) -> Result<usize> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(NODES)?;
        Ok(table.len()? as usize)
    }

    // -----------------------------------------------------------------------------------------
    // Attachments
    // -----------------------------------------------------------------------------------------

    pub fn put_attachment(&self, meta: &AttachmentMeta) -> Result<()> {
        let body = serde_json::to_vec(meta)?;
        let txn = self.db.begin_write()?;
        {
            let mut table = txn.open_table(ATTACHMENTS)?;
            table.insert(meta.id.as_str(), body.as_slice())?;
        }
        txn.commit()?;
        Ok(())
    }

    pub fn get_attachment(&self, id: &str) -> Result<Option<AttachmentMeta>> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(ATTACHMENTS)?;
        Ok(match table.get(id)? {
            Some(v) => Some(serde_json::from_slice(v.value())?),
            None => None,
        })
    }

    pub fn attachment_count(&self) -> Result<usize> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(ATTACHMENTS)?;
        Ok(table.len()? as usize)
    }

    // -----------------------------------------------------------------------------------------
    // Meta
    // -----------------------------------------------------------------------------------------

    pub fn get_meta(&self, key: &str) -> Result<Option<Vec<u8>>> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(META)?;
        Ok(table.get(key)?.map(|v| v.value().to_vec()))
    }

    pub fn set_meta(&self, key: &str, value: &[u8]) -> Result<()> {
        let txn = self.db.begin_write()?;
        {
            let mut table = txn.open_table(META)?;
            table.insert(key, value)?;
        }
        txn.commit()?;
        Ok(())
    }

    pub fn get_meta_json<T: for<'de> Deserialize<'de>>(&self, key: &str) -> Result<Option<T>> {
        Ok(match self.get_meta(key)? {
            Some(bytes) => Some(serde_json::from_slice(&bytes)?),
            None => None,
        })
    }

    pub fn set_meta_json<T: Serialize>(&self, key: &str, value: &T) -> Result<()> {
        self.set_meta(key, &serde_json::to_vec(value)?)
    }
}

/// Encode a public identity as the base64 form used in contact records.
pub fn identity_b64(ident: &openmesh_core::PublicIdentity) -> String {
    b64(&ident.to_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn temp_store() -> (TempDir, Store) {
        let dir = TempDir::new().unwrap();
        let store = Store::open(&dir.path().join("test.redb")).unwrap();
        (dir, store)
    }

    fn msg(id: u32, peer: NodeId, sent: u64, outgoing: bool) -> StoredMessage {
        StoredMessage {
            msg_id: id,
            peer: Some(peer),
            channel_id: None,
            channel_name: None,
            from: if outgoing { NodeId::new([9; 8]) } else { peer },
            to: if outgoing { peer } else { NodeId::new([9; 8]) },
            from_name: None,
            kind: MessageKind::Text,
            text: Some(format!("message {id}")),
            sent_at_ms: sent,
            received_at_ms: sent,
            outgoing,
            delivered: false,
            read: false,
            reply_to: None,
            thread: None,
            attachment: None,
            position: None,
            signature_valid: false,
            sender_key: None,
            via_store: false,
        }
    }

    #[test]
    fn message_roundtrip_and_lookup() {
        let (_dir, store) = temp_store();
        let peer = NodeId::new([7; 8]);
        let mut m = msg(1, peer, 1_000, true);
        m.delivered = true;
        store.put_message(&m).unwrap();
        let found = store.get_message(1).unwrap().unwrap();
        assert_eq!(found.text.as_deref(), Some("message 1"));
        assert_eq!(found.peer, Some(peer));
        assert!(found.delivered);
        assert_eq!(store.message_count().unwrap(), 1);
    }

    #[test]
    fn history_is_chronological_and_limited() {
        let (_dir, store) = temp_store();
        let peer = NodeId::new([7; 8]);
        for i in 1..=10u32 {
            store
                .put_message(&msg(i, peer, i as u64 * 100, i % 2 == 0))
                .unwrap();
        }
        let all = store.direct_history(peer, None, 100).unwrap();
        assert_eq!(all.len(), 10);
        assert_eq!(all.first().unwrap().msg_id, 1);
        assert_eq!(all.last().unwrap().msg_id, 10);
        let last3 = store.direct_history(peer, None, 3).unwrap();
        assert_eq!(
            last3.iter().map(|m| m.msg_id).collect::<Vec<_>>(),
            vec![8, 9, 10]
        );
        let before = store.direct_history(peer, Some(550), 100).unwrap();
        assert_eq!(
            before.iter().map(|m| m.msg_id).collect::<Vec<_>>(),
            vec![1, 2, 3, 4, 5]
        );
    }

    #[test]
    fn group_history_is_separate_from_direct() {
        let (_dir, store) = temp_store();
        let channel = NodeId::new([4; 8]);
        let peer = NodeId::new([7; 8]);
        store.put_message(&msg(1, peer, 100, false)).unwrap();
        let mut group = msg(2, peer, 200, false);
        group.peer = None;
        group.channel_id = Some(channel);
        group.channel_name = Some("family".into());
        store.put_message(&group).unwrap();
        assert_eq!(store.direct_history(peer, None, 100).unwrap().len(), 1);
        let g = store.channel_history(channel, None, 100).unwrap();
        assert_eq!(g.len(), 1);
        assert!(g[0].is_group());
        let json = g[0].to_group_message_json();
        assert_eq!(json.channel_name, "family");
    }

    #[test]
    fn delete_removes_from_both_indexes() {
        let (_dir, store) = temp_store();
        let peer = NodeId::new([7; 8]);
        store.put_message(&msg(1, peer, 100, true)).unwrap();
        assert!(store.delete_message(1).unwrap());
        assert!(store.get_message(1).unwrap().is_none());
        assert!(store.direct_history(peer, None, 100).unwrap().is_empty());
        assert!(!store.delete_message(1).unwrap());
    }

    #[test]
    fn messages_since_returns_chronological() {
        let (_dir, store) = temp_store();
        let a = NodeId::new([1; 8]);
        let b = NodeId::new([2; 8]);
        store.put_message(&msg(1, a, 100, false)).unwrap();
        store.put_message(&msg(2, b, 300, false)).unwrap();
        store.put_message(&msg(3, a, 200, false)).unwrap();
        let since = store.messages_since(150, 100).unwrap();
        assert_eq!(
            since.iter().map(|m| m.msg_id).collect::<Vec<_>>(),
            vec![3, 2]
        );
        assert_eq!(store.messages_since(0, 2).unwrap().len(), 2);
    }

    #[test]
    fn persistence_survives_reopen() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("persist.redb");
        let peer = NodeId::new([7; 8]);
        {
            let store = Store::open(&path).unwrap();
            store.put_message(&msg(1, peer, 100, true)).unwrap();
            store
                .put_channel(&ChannelRecord {
                    channel_id: NodeId::new([3; 8]),
                    name: "family".into(),
                    psk: b64(&[7u8; 32]),
                    created_at_ms: 1,
                })
                .unwrap();
            let ident = openmesh_core::Identity::from_seed(&[2u8; 32]).public();
            store
                .put_contact(&ContactRecord {
                    node_id: ident.node_id,
                    name: Some("bob".into()),
                    identity: identity_b64(&ident),
                    last_seen_ms: 5,
                    added_at_ms: 5,
                })
                .unwrap();
        }
        let store = Store::open(&path).unwrap();
        assert_eq!(store.message_count().unwrap(), 1);
        let channels = store.list_channels().unwrap();
        assert_eq!(channels.len(), 1);
        assert_eq!(channels[0].psk_bytes().unwrap(), [7u8; 32]);
        let contacts = store.list_contacts().unwrap();
        assert_eq!(contacts.len(), 1);
        assert!(contacts[0].ident().is_some());
    }

    #[test]
    fn nodes_and_meta_roundtrip() {
        let (_dir, store) = temp_store();
        let ident = openmesh_core::Identity::from_seed(&[3u8; 32]).public();
        let node = NodeJson::from_identity(&ident, "solar-ridge", false);
        store.put_node(&node).unwrap();
        let nodes = store.list_nodes().unwrap();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].name, "solar-ridge");
        store
            .set_meta_json("config", &serde_json::json!({"name":"x"}))
            .unwrap();
        let config: Option<serde_json::Value> = store.get_meta_json("config").unwrap();
        assert_eq!(config.unwrap()["name"], "x");
    }

    #[test]
    fn attachment_meta_roundtrip() {
        let (_dir, store) = temp_store();
        let meta = AttachmentMeta {
            id: "att_0123456789ab".into(),
            mime: "image/jpeg".into(),
            size: 5120,
            sha256: "ab".repeat(32),
            name: Some("trail.jpg".into()),
            duration_ms: None,
            created_at_ms: 1,
        };
        store.put_attachment(&meta).unwrap();
        let got = store.get_attachment(&meta.id).unwrap().unwrap();
        assert_eq!(got.mime, "image/jpeg");
        assert_eq!(store.attachment_count().unwrap(), 1);
        assert!(store.get_attachment("att_missing").unwrap().is_none());
    }
}
