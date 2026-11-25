//! Attachment storage: metadata in redb, bytes on disk under `<data_dir>/attachments/`.
//!
//! Uploads are raw bytes (`POST /api/attachments`), addressed by an opaque id and served back
//! with their stored content type. The 32 MiB cap lives here so every entry point enforces it.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};

use crate::store::{AttachmentMeta, Store};

/// Maximum attachment size accepted by the bridge (32 MiB).
pub const MAX_ATTACHMENT_BYTES: usize = 32 * 1024 * 1024;

/// On-disk attachment store.
pub struct AttachmentStore {
    dir: PathBuf,
    store: Arc<Store>,
}

impl AttachmentStore {
    pub fn open(data_dir: &Path, store: Arc<Store>) -> Result<Self> {
        let dir = data_dir.join("attachments");
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("creating attachment dir {}", dir.display()))?;
        Ok(Self { dir, store })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn file_path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.bin"))
    }

    /// Store `bytes`; returns the existing metadata when the same content is already stored.
    pub async fn put(
        &self,
        mime: &str,
        name: Option<&str>,
        duration_ms: Option<u32>,
        bytes: &[u8],
    ) -> Result<AttachmentMeta> {
        if bytes.len() > MAX_ATTACHMENT_BYTES {
            anyhow::bail!(
                "attachment too large: {} > {} bytes",
                bytes.len(),
                MAX_ATTACHMENT_BYTES
            );
        }
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        let digest: [u8; 32] = hasher.finalize().into();
        let sha_hex = hex::encode(digest);
        let id = format!("att_{}", &sha_hex[..12]);

        if let Some(existing) = self.store.get_attachment(&id)? {
            if existing.sha256 == sha_hex {
                return Ok(existing);
            }
        }

        let path = self.file_path(&id);
        let tmp = self.dir.join(format!(
            ".{id}.tmp-{}-{}",
            std::process::id(),
            rand::random::<u32>()
        ));
        tokio::fs::write(&tmp, bytes)
            .await
            .with_context(|| format!("writing attachment temp file {}", tmp.display()))?;
        tokio::fs::rename(&tmp, &path)
            .await
            .with_context(|| format!("renaming attachment into place {}", path.display()))?;

        let meta = AttachmentMeta {
            id,
            mime: if mime.is_empty() {
                "application/octet-stream".to_string()
            } else {
                mime.to_string()
            },
            size: bytes.len() as u64,
            sha256: sha_hex,
            name: name.map(|n| n.to_string()),
            duration_ms,
            created_at_ms: openmesh_core::util::now_ms(),
        };
        self.store.put_attachment(&meta)?;
        Ok(meta)
    }

    pub fn meta(&self, id: &str) -> Result<Option<AttachmentMeta>> {
        self.store.get_attachment(id)
    }

    /// Read metadata and raw bytes for `id`.
    pub async fn read(&self, id: &str) -> Result<Option<(AttachmentMeta, Vec<u8>)>> {
        let Some(meta) = self.store.get_attachment(id)? else {
            return Ok(None);
        };
        let path = self.file_path(id);
        match tokio::fs::read(&path).await {
            Ok(bytes) => Ok(Some((meta, bytes))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e).with_context(|| format!("reading attachment {}", path.display())),
        }
    }

    /// Total bytes of attachment payloads on disk.
    pub fn total_bytes(&self) -> u64 {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return 0;
        };
        entries
            .filter_map(|e| e.ok())
            .filter_map(|e| e.metadata().ok())
            .filter(|m| m.is_file())
            .map(|m| m.len())
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn temp_store() -> (TempDir, Arc<Store>, AttachmentStore) {
        let dir = TempDir::new().unwrap();
        let store = Arc::new(Store::open(&dir.path().join("test.redb")).unwrap());
        let attachments = AttachmentStore::open(dir.path(), store.clone()).unwrap();
        (dir, store, attachments)
    }

    #[tokio::test]
    async fn put_read_roundtrip_with_content_type() {
        let (_dir, _store, attachments) = temp_store();
        let bytes = b"fake jpeg bytes".to_vec();
        let meta = attachments
            .put("image/jpeg", Some("trail.jpg"), None, &bytes)
            .await
            .unwrap();
        assert!(meta.id.starts_with("att_"));
        assert_eq!(meta.id.len(), 4 + 12);
        assert_eq!(meta.size, bytes.len() as u64);
        assert_eq!(meta.sha256.len(), 64);
        let (got_meta, got) = attachments.read(&meta.id).await.unwrap().unwrap();
        assert_eq!(got_meta.mime, "image/jpeg");
        assert_eq!(got, bytes);
    }

    #[tokio::test]
    async fn duplicate_content_reuses_id() {
        let (_dir, store, attachments) = temp_store();
        let a = attachments
            .put("image/png", None, None, b"same")
            .await
            .unwrap();
        let b = attachments
            .put("image/png", Some("other.png"), None, b"same")
            .await
            .unwrap();
        assert_eq!(a.id, b.id);
        assert_eq!(store.attachment_count().unwrap(), 1);
    }

    #[tokio::test]
    async fn missing_attachment_is_none() {
        let (_dir, _store, attachments) = temp_store();
        assert!(
            attachments
                .read("att_ffffffffffff")
                .await
                .unwrap()
                .is_none()
        );
        assert!(attachments.meta("att_ffffffffffff").unwrap().is_none());
    }

    #[tokio::test]
    async fn size_cap_is_enforced() {
        let (_dir, _store, attachments) = temp_store();
        let too_big = vec![0u8; MAX_ATTACHMENT_BYTES + 1];
        assert!(
            attachments
                .put("application/octet-stream", None, None, &too_big)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn total_bytes_counts_payloads() {
        let (_dir, _store, attachments) = temp_store();
        attachments
            .put("text/plain", None, None, b"abc")
            .await
            .unwrap();
        attachments
            .put("text/plain", None, None, b"defgh")
            .await
            .unwrap();
        assert_eq!(attachments.total_bytes(), 8);
    }
}
