//! Bridge auth token handling.
//!
//! The token is generated on first run and written to `~/.config/openmesh/bridge-token`
//! (mode 0600 on Unix). Clients present it as `Authorization: Bearer <token>` for REST or
//! `?token=<token>` on the WebSocket URL; `GET /api/health` is public.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rand::RngCore;

/// Default token location: `$XDG_CONFIG_HOME/openmesh/bridge-token` or `~/.config/...`.
pub fn default_token_path() -> PathBuf {
    if let Some(dir) = std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()) {
        PathBuf::from(dir).join("openmesh").join("bridge-token")
    } else if let Some(home) = std::env::var_os("HOME").filter(|v| !v.is_empty()) {
        PathBuf::from(home)
            .join(".config")
            .join("openmesh")
            .join("bridge-token")
    } else {
        PathBuf::from(".config")
            .join("openmesh")
            .join("bridge-token")
    }
}

/// Default data directory: `$XDG_DATA_HOME/openmesh` or `~/.local/share/openmesh`.
pub fn default_data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("XDG_DATA_HOME").filter(|v| !v.is_empty()) {
        PathBuf::from(dir).join("openmesh")
    } else if let Some(home) = std::env::var_os("HOME").filter(|v| !v.is_empty()) {
        PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("openmesh")
    } else {
        PathBuf::from(".openmesh")
    }
}

/// Generate a fresh 32-byte token, hex encoded (64 characters).
pub fn generate_token() -> String {
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    hex::encode(bytes)
}

/// Load the token at `path`, creating it (and its parent directory) on first run.
pub fn load_or_create(path: &Path) -> Result<String> {
    match std::fs::read_to_string(path) {
        Ok(content) => {
            let token = content.trim().to_string();
            if token.len() >= 32 {
                return Ok(token);
            }
            tracing::warn!(path = %path.display(), "token file too short, regenerating");
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => {
            return Err(e).with_context(|| format!("reading token file {}", path.display()));
        }
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating token dir {}", parent.display()))?;
    }
    let token = generate_token();
    std::fs::write(path, format!("{token}\n"))
        .with_context(|| format!("writing token file {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    tracing::info!(path = %path.display(), "generated new bridge API token");
    Ok(token)
}

/// Load a 32-byte identity seed from `path`, creating one on first run.
pub fn load_or_create_seed(path: &Path) -> Result<[u8; 32]> {
    if let Ok(content) = std::fs::read_to_string(path) {
        let trimmed = content.trim();
        if let Ok(bytes) = hex::decode(trimmed) {
            if let Ok(seed) = <[u8; 32]>::try_from(bytes.as_slice()) {
                return Ok(seed);
            }
        }
        tracing::warn!(path = %path.display(), "invalid identity seed file, generating a new one");
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating identity dir {}", parent.display()))?;
    }
    let mut seed = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut seed);
    std::fs::write(path, format!("{}\n", hex::encode(seed)))
        .with_context(|| format!("writing identity seed {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(seed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn creates_and_reloads_token() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("nested/bridge-token");
        let first = load_or_create(&path).unwrap();
        assert_eq!(first.len(), 64);
        assert!(first.chars().all(|c| c.is_ascii_hexdigit()));
        let second = load_or_create(&path).unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn short_token_is_regenerated() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("bridge-token");
        std::fs::write(&path, "short\n").unwrap();
        let token = load_or_create(&path).unwrap();
        assert_eq!(token.len(), 64);
    }

    #[test]
    fn creates_and_reloads_identity_seed() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("identity.key");
        let a = load_or_create_seed(&path).unwrap();
        let b = load_or_create_seed(&path).unwrap();
        assert_eq!(a, b);
        assert_eq!(a, openmesh_core::Identity::from_seed(&a).seed());
    }

    #[test]
    fn generate_token_is_unique() {
        assert_ne!(generate_token(), generate_token());
    }
}
